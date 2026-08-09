//! # 事件驱动调度器
//!
//! 豆包设计哲学的核心实现：
//!
//! - **图是骨架**：调度器持有图结构定义，决定节点顺序和路由
//! - **事件是血液**：节点完成 → 发事件 → 调度器订阅 → 驱动下一节点
//! - **Actor 是细胞**：每个节点是一个 Actor，靠事件驱动执行
//!
//! ## 事件流
//!
//! ```text
//! GraphStarted ──► NodeExecuteRequested ──► Actor 执行
//!                                               │
//!                          ┌────────────────────┘
//!                          ▼
//!                    NodeCompleted ──► 调度器订阅
//!                          │
//!                          ▼
//!                    确定下一节点（图路由）
//!                          │
//!                          ▼
//!                    NodeExecuteRequested ──► 下一个 Actor
//!                          ...
//!                    GraphCompleted
//! ```

use super::super::engine::{Graph, GraphError, GraphOutput};
use super::super::state::GraphState;
use crate::event::{AgentEventData, Event, EventBus, EventHandler};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::mpsc;

/// 节点事件（内部传递，通过通道避免锁竞争）
enum NodeEvent {
    Completed {
        node_name: String,
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

/// 事件驱动调度器
///
/// 订阅 NodeCompleted/NodeFailed 事件，根据图定义驱动节点执行。
/// 调度器本身不直接调用 Actor，而是通过事件总线发布 NodeExecuteRequested。
pub struct EventDrivenScheduler {
    /// 关联的图
    graph: Arc<Graph>,
    /// 事件总线
    event_bus: Arc<EventBus>,
}

impl EventDrivenScheduler {
    /// 创建事件驱动调度器
    pub fn new(graph: Arc<Graph>, event_bus: Arc<EventBus>) -> Self {
        Self { graph, event_bus }
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

        // 发布 GraphStarted 事件（带 trace_id 上下文，供 TraceEventBridge 桥接）
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

        // 调度器本地状态（单任务独占，无锁）
        let mut current_state = state;
        let mut execution_path = Vec::new();
        let mut node_visit_count: HashMap<String, usize> = HashMap::new();
        let mut final_output = String::new();
        let mut success = true;
        let mut last_error: Option<String> = None;
        let mut step = 0;
        let mut pending_count = 1usize; // 入口节点
        let mut completed_nodes: HashSet<String> = HashSet::new();

        // 发布入口节点的执行请求（携带初始状态 + trace 上下文）
        self.emit_execute_request(
            &run_id,
            &entry,
            1,
            &current_state,
            trace_id.as_deref(),
            session_id.as_deref(),
        )
        .await;

        let start = std::time::Instant::now();

        // 主循环：接收节点事件，驱动下一节点
        loop {
            match node_event_rx.recv().await {
                Some(NodeEvent::Completed {
                    node_name,
                    output,
                    success: completed_success,
                    duration_ms,
                    state_updates,
                }) => {
                    pending_count -= 1;
                    execution_path.push(node_name.clone());
                    step += 1;
                    completed_nodes.insert(node_name.clone());

                    tracing::debug!(
                        "📥 节点完成: {} (ok={}, {}ms), pending={}",
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
                        // 合并状态更新到调度器状态（使用图的 reducer 策略）
                        current_state.merge_with_reducers(state_updates, &self.graph.reducers);
                    }

                    // 确定下一批节点（只在成功时，基于合并后的状态判断路由）
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

                    pending_count += valid_next.len();

                    // 如果没有待完成节点，结束
                    if pending_count == 0 {
                        break;
                    }

                    // 发布下一批节点的执行请求（携带当前合并后的状态 + trace 上下文）
                    for next in valid_next {
                        self.emit_execute_request(
                            &run_id,
                            &next,
                            step + 1,
                            &current_state,
                            trace_id.as_deref(),
                            session_id.as_deref(),
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

        // 发布 GraphCompleted 事件（带 trace_id 上下文）
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

    /// 发布节点执行请求事件（携带当前状态 + trace 上下文）
    async fn emit_execute_request(
        &self,
        run_id: &str,
        node_name: &str,
        step: usize,
        state: &GraphState,
        trace_id: Option<&str>,
        session_id: Option<&str>,
    ) {
        tracing::debug!(
            "📤 请求节点执行: run={}, node={}, step={}, state_keys={}, trace_id={:?}",
            run_id,
            node_name,
            step,
            state.data().len(),
            trace_id,
        );
        self.emit_with_trace(
            AgentEventData::NodeExecuteRequested {
                run_id: run_id.to_string(),
                node_name: node_name.to_string(),
                step,
                state: state.data().clone(),
            },
            trace_id,
            session_id,
        )
        .await;
    }

    /// 带 trace_id 上下文发布事件（trace_id 为空时降级为普通 emit）
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
