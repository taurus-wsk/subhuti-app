//! # 事件驱动调度器（竞标制）
//!
//! 豆包设计哲学的核心实现：
//!
//! - **图是骨架**：调度器持有图结构定义，决定节点顺序和路由
//! - **Actor 是演员**：全局 Actor 池通过竞标制竞争节点
//! - **竞标是核心**：节点发布任务要求 → 所有 Actor 自评分数 → 最高分上台
//!
//! ## 竞标流程
//!
//! ```text
//! 图节点需要执行
//!   → 调度器获取节点任务标签
//!   → 调度器从 ActorRegistry 竞标（find_best）
//!   → 调度器发布 ActorTaskRequested（可观测性）
//!   → 调度器发布 NodeTaskAssigned（可观测性）
//!   → 调度器直接调用 actor.perform()
//!   → 调度器发布 NodeCompleted/NodeFailed
//!   → 调度器事件循环接收 → 驱动下一节点
//! ```
//!
//! ## 与旧架构的区别
//!
//! | 旧架构（预分配） | 新架构（竞标制） |
//! |-----------------|-----------------|
//! | 每个节点创建固定 EventDrivenActor | 全局 Actor 池竞标上岗 |
//! | 节点预绑定 ExpertAgent | 节点只定义任务标签，Actor 自评匹配 |
//! | 通过 EventBus 间接执行 | 调度器直接调用 actor.perform() |
//! | 调度器 ←→ Actor 双向事件通信 | 调度器单向发布事件（可观测性） |

use super::super::engine::{Graph, GraphError, GraphOutput};
use super::super::state::GraphState;
use crate::event::{AgentEventData, Event, EventBus, EventHandler};
use crate::orchestrator::actor::ActorRegistry;
use crate::orchestrator::{AgentContext, ExpertState};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

/// 节点事件（内部传递，通过通道避免锁竞争）
enum NodeEvent {
    Completed {
        node_name: String,
        actor_name: String,
        output: String,
        success: bool,
        duration_ms: u64,
        state_updates: HashMap<String, serde_json::Value>,
    },
    Failed {
        node_name: String,
        error: String,
    },
}

/// 事件处理器：订阅 NodeCompleted/NodeFailed，转发给调度器主循环
struct NodeEventHandler {
    run_id: String,
    sender: mpsc::Sender<NodeEvent>,
}

#[async_trait::async_trait]
impl EventHandler for NodeEventHandler {
    async fn handle(&self, event: &Event) {
        match &event.data {
            AgentEventData::NodeCompleted {
                run_id,
                node_name,
                actor_name,
                output,
                success,
                duration_ms,
                state_updates,
                ..
            } if run_id == &self.run_id => {
                let _ = self
                    .sender
                    .send(NodeEvent::Completed {
                        node_name: node_name.clone(),
                        actor_name: actor_name.clone(),
                        output: output.clone(),
                        success: *success,
                        duration_ms: *duration_ms,
                        state_updates: state_updates.clone(),
                    })
                    .await;
            }
            AgentEventData::NodeFailed {
                run_id,
                node_name,
                error,
                ..
            } if run_id == &self.run_id => {
                let _ = self
                    .sender
                    .send(NodeEvent::Failed {
                        node_name: node_name.clone(),
                        error: error.clone(),
                    })
                    .await;
            }
            _ => {}
        }
    }

    fn name(&self) -> &str {
        "EventDrivenScheduler"
    }
}

/// 事件驱动调度器（竞标制）
///
/// 核心变化：
/// - 不再为每个节点创建 EventDrivenActor
/// - 全局 Actor 池（ActorRegistry）中的 Actor 竞标每个节点
/// - 节点定义任务标签，Actor 自评分数，最高分上台
/// - 调度器直接执行 Actor，通过 EventBus 发布事件（可观测性）
pub struct EventDrivenScheduler {
    /// 关联的图
    graph: Arc<Graph>,
    /// 事件总线
    event_bus: Arc<EventBus>,
    /// 全局演员池（竞标用）
    actor_registry: Arc<ActorRegistry>,
    /// 专家共享状态（Actor 执行时需要）
    expert_state: ExpertState,
}

impl EventDrivenScheduler {
    /// 创建事件驱动调度器
    pub fn new(
        graph: Arc<Graph>,
        event_bus: Arc<EventBus>,
        actor_registry: &ActorRegistry,
        expert_state: &ExpertState,
    ) -> Self {
        Self {
            graph,
            event_bus,
            actor_registry: Arc::new(actor_registry.clone()),
            expert_state: expert_state.clone(),
        }
    }

    /// 事件驱动执行图
    pub async fn run(&self, state: GraphState) -> anyhow::Result<GraphOutput> {
        let run_id = format!("run_{}", chrono::Utc::now().timestamp_millis());
        self.run_with_run_id(state, run_id).await
    }

    /// 事件驱动执行图（使用外部传入的 run_id）
    pub async fn run_with_run_id(
        &self,
        state: GraphState,
        run_id: String,
    ) -> anyhow::Result<GraphOutput> {
        let entry = self
            .graph
            .entry
            .as_ref()
            .ok_or(GraphError::NoEntryNode)?
            .clone();

        // 从初始 state 提取 trace_id/session_id（由 Orchestrator.dispatch_via_graph 注入）
        let trace_id = state.get("trace_id");
        let session_id = state.get("session_id");

        // 创建节点事件通道（调度器主循环独占接收）
        let (node_event_tx, mut node_event_rx) = mpsc::channel::<NodeEvent>(128);

        // 订阅节点完成/失败事件
        let handler = Arc::new(NodeEventHandler {
            run_id: run_id.clone(),
            sender: node_event_tx,
        }) as Arc<dyn EventHandler>;
        let scheduler_subscription_id = self.event_bus.subscribe(handler).await;

        // 发布 GraphStarted 事件（带 trace_id 上下文）
        self.emit_with_trace(
            AgentEventData::GraphStarted {
                graph_name: self.graph.name.clone(),
                run_id: run_id.clone(),
                entry_node: entry.clone(),
            },
            trace_id.as_deref(),
            session_id.as_deref(),
        )
        .await;

        // 调度器本地状态
        let mut current_state = state;
        let mut execution_path = Vec::new();
        let mut node_visit_count: HashMap<String, usize> = HashMap::new();
        let mut final_output = String::new();
        let mut success = true;
        let mut last_error: Option<String> = None;
        let mut step = 0;
        let mut pending_count = 1usize; // 入口节点
        let mut completed_nodes: HashSet<String> = HashSet::new();

        // 竞标执行入口节点
        self.execute_node_with_bidding(
            &entry,
            1,
            &run_id,
            &mut current_state,
            &trace_id,
            &session_id,
            &mut pending_count,
        )
        .await;

        let start = Instant::now();

        // 主循环：接收节点事件，驱动下一节点
        loop {
            match node_event_rx.recv().await {
                Some(NodeEvent::Completed {
                    node_name,
                    actor_name,
                    output,
                    success: completed_success,
                    duration_ms,
                    state_updates,
                }) => {
                    pending_count -= 1;
                    execution_path.push(actor_name);
                    step += 1;
                    completed_nodes.insert(node_name.clone());

                    tracing::debug!(
                        "节点完成: {} (ok={}, {}ms), pending={}",
                        node_name,
                        completed_success,
                        duration_ms,
                        pending_count
                    );

                    if !completed_success {
                        success = false;
                        last_error = Some(format!("节点 {} 执行失败", node_name));
                    } else {
                        if !output.is_empty() {
                            final_output = output;
                        }
                        // 合并状态更新
                        current_state.merge_with_reducers(state_updates, &self.graph.reducers);
                    }

                    // 确定下一批节点
                    let next_nodes: Vec<String> = if completed_success {
                        self.graph
                            .determine_next_all(&node_name, &current_state, None)
                    } else {
                        vec![]
                    };

                    // 循环检测 + fan-in 去重
                    let mut valid_next = Vec::new();
                    for next in &next_nodes {
                        let visits = node_visit_count.entry(next.clone()).or_insert(0);
                        *visits += 1;
                        if *visits > self.graph.max_iterations {
                            success = false;
                            last_error =
                                Some(format!("循环检测：节点 {} 已执行 {} 次", next, visits));
                            break;
                        }
                        if !completed_nodes.contains(next) {
                            valid_next.push(next.clone());
                        }
                    }

                    // 增加待完成计数
                    pending_count += valid_next.len();

                    if pending_count == 0 {
                        break;
                    }

                    // 竞标执行下一批节点
                    for next in valid_next {
                        self.execute_node_with_bidding(
                            &next,
                            step + 1,
                            &run_id,
                            &mut current_state,
                            &trace_id,
                            &session_id,
                            &mut pending_count,
                        )
                        .await;
                    }
                }
                Some(NodeEvent::Failed { node_name, error }) => {
                    pending_count -= 1;
                    success = false;
                    last_error = Some(error);
                    execution_path.push(node_name);

                    if pending_count == 0 {
                        break;
                    }
                }
                None => {
                    tracing::warn!("事件通道关闭");
                    success = false;
                    last_error = Some("事件通道关闭".into());
                    break;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        // 发布 GraphCompleted 事件
        self.emit_with_trace(
            AgentEventData::GraphCompleted {
                run_id: run_id.clone(),
                success,
                total_steps: step,
                duration_ms,
            },
            trace_id.as_deref(),
            session_id.as_deref(),
        )
        .await;

        // 清理调度器订阅
        self.event_bus.unsubscribe(&scheduler_subscription_id).await;

        Ok(GraphOutput {
            output: final_output,
            state: current_state,
            success,
            total_steps: step,
            duration_ms,
            execution_path,
            error: last_error,
        })
    }

    /// 竞标执行节点
    ///
    /// 1. 获取节点任务标签
    /// 2. 从 ActorRegistry 竞标（所有 Actor 自评分数，最高分者中标）
    /// 3. 发布 ActorTaskRequested（可观测性）
    /// 4. 发布 NodeTaskAssigned（可观测性）
    /// 5. 直接调用 actor.perform() 执行
    /// 6. 发布 NodeCompleted/NodeFailed
    async fn execute_node_with_bidding(
        &self,
        node_name: &str,
        _step: usize,
        run_id: &str,
        current_state: &GraphState,
        trace_id: &Option<String>,
        session_id: &Option<String>,
        _pending_count: &mut usize,
    ) {
        // 1. 获取节点任务标签
        // 如果节点没有标签，使用输入消息内容作为标签（兜底匹配）
        let mut task_tags = self
            .graph
            .node_tags
            .get(node_name)
            .cloned()
            .unwrap_or_default();
        if task_tags.is_empty() {
            let input = current_state.get("input").unwrap_or_default();
            if !input.is_empty() {
                task_tags = vec![input];
                tracing::debug!("节点无标签，使用输入消息作为竞标标签");
            }
        }

        tracing::debug!(
            "竞标节点: {} (tags={:?}), 可用 Actor 数: {}",
            node_name,
            task_tags,
            self.actor_registry.count()
        );

        // 2. 发布 ActorTaskRequested（可观测性）
        self.emit_with_trace(
            AgentEventData::ActorTaskRequested {
                run_id: run_id.to_string(),
                node_name: node_name.to_string(),
                task_tags: task_tags.clone(),
                task_description: String::new(),
                step: _step,
                state: current_state.data().clone(),
            },
            trace_id.as_deref(),
            session_id.as_deref(),
        )
        .await;

        // 3. 从 ActorRegistry 竞标：取前 3 名候补（分数 >= 60）
        let candidates = self.actor_registry.find_top_candidates(&task_tags, 3).await;

        if candidates.is_empty() {
            tracing::warn!(
                "竞标失败: 无 Actor 匹配节点 {} (tags={:?})",
                node_name,
                task_tags
            );

            self.emit_with_trace(
                AgentEventData::NodeFailed {
                    run_id: run_id.to_string(),
                    node_name: node_name.to_string(),
                    error: format!("无 Actor 匹配节点 {} (tags={:?})", node_name, task_tags),
                    duration_ms: 0,
                },
                trace_id.as_deref(),
                session_id.as_deref(),
            )
            .await;
            return;
        }

        // 4. 按候补顺序尝试执行（有候补就有重试）
        let mut last_error = String::new();
        for (idx, (actor, score)) in candidates.iter().enumerate() {
            tracing::debug!(
                "候补 #{}/{}: node={}, actor={} (score={})",
                idx + 1,
                candidates.len(),
                node_name,
                actor.name(),
                score
            );

            // 发布 NodeTaskAssigned
            self.emit_with_trace(
                AgentEventData::NodeTaskAssigned {
                    run_id: run_id.to_string(),
                    node_name: node_name.to_string(),
                    actor_id: actor.id().to_string(),
                    actor_name: actor.name().to_string(),
                    score: *score,
                    state: current_state.data().clone(),
                },
                trace_id.as_deref(),
                session_id.as_deref(),
            )
            .await;

            // 执行 Actor
            let input = current_state.get("input").unwrap_or_default();
            let mut ctx = AgentContext::new(&input, run_id);
            if let Some(tid) = trace_id {
                ctx.set_metadata("trace_id", tid);
            }
            if let Some(sid) = session_id {
                ctx.set_metadata("session_id", sid);
            }

            let start = Instant::now();
            let result = actor.perform(&mut ctx, &self.expert_state).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(output) => {
                    tracing::debug!(
                        "Actor '{}' 执行成功: node={}, duration={}ms",
                        actor.name(),
                        node_name,
                        duration_ms
                    );

                    self.emit_with_trace(
                        AgentEventData::NodeCompleted {
                            run_id: run_id.to_string(),
                            node_name: node_name.to_string(),
                            actor_name: actor.name().to_string(),
                            output,
                            success: true,
                            duration_ms,
                            next_nodes: vec![],
                            state_updates: HashMap::new(),
                        },
                        trace_id.as_deref(),
                        session_id.as_deref(),
                    )
                    .await;
                    return; // 成功，结束
                }
                Err(e) => {
                    last_error = e.to_string();
                    tracing::warn!(
                        "Actor '{}' 执行失败 (候补 #{}/{}): node={}, error={}",
                        actor.name(),
                        idx + 1,
                        candidates.len(),
                        node_name,
                        last_error
                    );
                    // 继续尝试下一个候补
                }
            }
        }

        // 所有候补都失败，发布 NodeFailed
        tracing::error!(
            "节点 {} 所有候补 Actor 均失败 (共 {} 个): last_error={}",
            node_name,
            candidates.len(),
            last_error
        );
        self.emit_with_trace(
            AgentEventData::NodeFailed {
                run_id: run_id.to_string(),
                node_name: node_name.to_string(),
                error: format!("所有候补 Actor 均失败: {}", last_error),
                duration_ms: 0,
            },
            trace_id.as_deref(),
            session_id.as_deref(),
        )
        .await;
    }

    /// 带 trace_id 上下文发布事件
    async fn emit_with_trace(
        &self,
        data: AgentEventData,
        trace_id: Option<&str>,
        session_id: Option<&str>,
    ) {
        match trace_id {
            Some(tid) if !tid.is_empty() => {
                self.event_bus
                    .emit_with_trace(data, tid.to_string(), session_id.map(|s| s.to_string()))
                    .await;
            }
            _ => {
                self.event_bus.emit(data).await;
            }
        }
    }
}
