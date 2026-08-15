//! # 图引擎 - 执行核心
//!
//! DAG + 事件驱动 + Actor 三层混合编排引擎。
//!
//! ## 三层架构
//!
//! - **Graph 层**：拓扑结构、路由、循环检测（本文件）
//! - **Actor 层**：并发执行、状态隔离、故障恢复（`actor/`）
//! - **Event 层**：解耦通信、可观测性（`event/`）
//!
//! ## 执行模式
//!
//! | 方法 | 模式 | 特点 |
//! |------|------|------|
//! | `run_event_driven()` | 事件驱动 | 竞标制调度，Actor 池执行，可观测 |
//! | `run_with_actors()` | Actor 执行 | 并行 fan-out，故障恢复 |
//! | `run_with_id()` | 直接执行 | 串行，支持检查点恢复 |

use super::actor::{NodeMessage, SupervisionStrategy, Supervisor};
use super::checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
use super::node::{ConditionalEdge, Edge, GraphNode, NodeResult, Route};
use super::state::{GraphState, StateReducer};
use super::validator::{INodeValidator, NodeFixRunner};
use crate::event::{AgentEventData, EventBus};
use crate::guardrails::IGuardrail;
use crate::orchestrator::actor::ActorRegistry;
use crate::orchestrator::ExpertState;
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 图执行输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphOutput {
    /// 最终输出内容
    pub output: String,
    /// 最终状态
    pub state: GraphState,
    /// 是否成功
    pub success: bool,
    /// 执行的节点序列
    pub execution_path: Vec<String>,
    /// 总步数
    pub total_steps: usize,
    /// 总耗时（毫秒）
    pub duration_ms: u64,
    /// 错误信息
    pub error: Option<String>,
}

/// 图错误
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("节点未找到: {0}")]
    NodeNotFound(String),
    #[error("入口节点未设置")]
    NoEntryNode,
    #[error("节点 {0} 没有出口边")]
    NoOutgoingEdge(String),
    #[error("循环检测：节点 {0} 已执行 {1} 次，超过最大循环次数 {2}")]
    LoopDetected(String, usize, usize),
    #[error("节点执行失败: {0}")]
    NodeFailed(String),
    #[error("构建错误: {0}")]
    BuildError(String),
}

/// 图定义
pub struct Graph {
    /// 图名称
    pub(crate) name: String,
    /// 节点列表
    pub(crate) nodes: HashMap<String, GraphNode>,
    /// 节点标签（竞标制用：Actor 根据标签自评分数）
    pub(crate) node_tags: HashMap<String, Vec<String>>,
    /// 静态边：from -> [to]
    pub(crate) edges: HashMap<String, Vec<String>>,
    /// 条件边：from -> ConditionalEdge
    pub(crate) conditional_edges: HashMap<String, ConditionalEdge>,
    /// 入口节点
    pub(crate) entry: Option<String>,
    /// 结束节点（无出口边的节点自动为结束节点）
    pub(crate) end_nodes: Vec<String>,
    /// 状态 Reducer
    pub(crate) reducers: HashMap<String, StateReducer>,
    /// 事件总线（可选）
    pub(crate) event_bus: Option<Arc<EventBus>>,
    /// 检查点存储
    pub(crate) checkpoint_store: Arc<dyn CheckpointStore>,
    /// 最大循环次数
    pub(crate) max_iterations: usize,
    /// 补偿节点：节点失败时执行的补偿节点
    pub(crate) compensation_nodes: HashMap<String, String>,
    /// 降级分支：节点失败时的备用节点
    pub(crate) fallback_nodes: HashMap<String, String>,
    /// 节点校验器：每个节点对应的输出校验器
    pub(crate) node_validators: HashMap<String, Arc<dyn INodeValidator>>,
    /// 自动修复配置：最大修复次数
    pub(crate) max_fix_attempts: u32,
    /// 安全护栏（可选）
    pub(crate) guardrail: Option<Arc<dyn IGuardrail>>,
}

impl Clone for Graph {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            nodes: self.nodes.clone(),
            node_tags: self.node_tags.clone(),
            edges: self.edges.clone(),
            conditional_edges: self.conditional_edges.clone(),
            entry: self.entry.clone(),
            end_nodes: self.end_nodes.clone(),
            reducers: self.reducers.clone(),
            event_bus: self.event_bus.clone(),
            checkpoint_store: self.checkpoint_store.clone(),
            max_iterations: self.max_iterations,
            compensation_nodes: self.compensation_nodes.clone(),
            fallback_nodes: self.fallback_nodes.clone(),
            node_validators: self.node_validators.clone(),
            max_fix_attempts: self.max_fix_attempts,
            guardrail: self.guardrail.clone(),
        }
    }
}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Graph")
            .field("name", &self.name)
            .field("nodes", &self.nodes.keys().collect::<Vec<_>>())
            .field("entry", &self.entry)
            .field("end_nodes", &self.end_nodes)
            .field("max_iterations", &self.max_iterations)
            .finish_non_exhaustive()
    }
}

impl Graph {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn event_bus(&self) -> Option<&Arc<EventBus>> {
        self.event_bus.as_ref()
    }

    /// 注入事件总线（用于在注册时由框架注入，使 graph 执行时能 emit 事件）
    pub fn set_event_bus(&mut self, bus: Arc<EventBus>) {
        self.event_bus = Some(bus);
    }

    pub fn compensation_node(&self, node_id: &str) -> Option<&String> {
        self.compensation_nodes.get(node_id)
    }

    pub fn fallback_node(&self, node_id: &str) -> Option<&String> {
        self.fallback_nodes.get(node_id)
    }

    /// 获取安全护栏
    pub fn guardrail(&self) -> Option<&Arc<dyn IGuardrail>> {
        self.guardrail.as_ref()
    }

    /// 执行节点函数（集成 NodeFixRunner 自动修复）
    async fn execute_node_func(
        &self,
        node_name: &str,
        node: &GraphNode,
        state: GraphState,
    ) -> NodeResult {
        // 如果节点配置了校验器且开启了修复，使用 NodeFixRunner
        if let Some(validator) = self.node_validators.get(node_name) {
            let fix_attempts = if self.max_fix_attempts > 0 {
                self.max_fix_attempts
            } else {
                3
            };
            let runner = NodeFixRunner::new(fix_attempts);
            match runner.execute_with_fix(&node.func, state, validator).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::error!("节点 {} 自动修复失败: {}", node_name, e);
                    NodeResult::err(e.to_string())
                }
            }
        } else {
            node.func.call(state).await
        }
    }

    /// 包装节点函数，集成 NodeFixRunner 自动修复（用于 Actor 模式）
    fn wrap_node_with_fix(
        &self,
        node_name: &str,
        func: super::node::NodeFn,
        validator: Arc<dyn INodeValidator>,
    ) -> super::node::NodeFn {
        let max_attempts = if self.max_fix_attempts > 0 {
            self.max_fix_attempts
        } else {
            3
        };
        let node_name = node_name.to_string();

        super::node::NodeFn::new(move |state| {
            let func = func.clone();
            let validator = validator.clone();
            let name = node_name.clone();
            async move {
                let runner = NodeFixRunner::new(max_attempts);
                match runner
                    .execute_with_fix(&func, state.clone(), &validator)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        tracing::error!("节点 {} 自动修复失败: {}", name, e);
                        NodeResult::err(e.to_string())
                    }
                }
            }
        })
    }

    /// 带执行 ID 执行（支持从检查点恢复）
    pub async fn run_with_id(&self, run_id: &str, state: &mut GraphState) -> Result<GraphOutput> {
        let start = std::time::Instant::now();
        let entry = self.entry.as_ref().ok_or(GraphError::NoEntryNode)?;

        // 检查是否有检查点
        let (mut current_node, mut step) =
            if let Some(cp) = self.checkpoint_store.get_latest(run_id).await {
                tracing::info!(
                    "🔄 Resuming from checkpoint: node={}, step={}",
                    cp.next_node.as_deref().unwrap_or(entry),
                    cp.step
                );
                *state = cp.state.clone();
                (cp.next_node.unwrap_or_else(|| entry.clone()), cp.step)
            } else {
                (entry.clone(), 0)
            };

        let mut execution_path = Vec::new();
        let mut node_visit_count: HashMap<String, usize> = HashMap::new();
        let mut final_output = String::new();
        let mut last_error: Option<String> = None;
        let mut success = true;

        loop {
            step += 1;

            // 循环检测
            let visits = node_visit_count.entry(current_node.clone()).or_insert(0);
            *visits += 1;
            if *visits > self.max_iterations {
                return Err(GraphError::LoopDetected(
                    current_node.clone(),
                    *visits,
                    self.max_iterations,
                )
                .into());
            }

            // 获取节点
            let node = self
                .nodes
                .get(&current_node)
                .ok_or_else(|| GraphError::NodeNotFound(current_node.clone()))?;

            // 发布事件：节点开始
            self.emit_event(
                state,
                AgentEventData::FlowStarted {
                    flow_type: format!("graph:{}", current_node),
                    input: state.to_json().unwrap_or_default(),
                },
            )
            .await;

            tracing::debug!("▶ Graph node: {} (step {})", current_node, step);

            // Guardrail 输入检查
            if let Some(gr) = &self.guardrail {
                let input_str = state.get("input").unwrap_or_default();
                let gr_result = gr.check_input(&input_str).await;
                if !gr_result.allowed {
                    success = false;
                    last_error = Some(format!("Guardrail 输入拦截: {}", gr_result.reason));
                    tracing::warn!("❌ Guardrail 输入拦截: {}", gr_result.reason);
                    break;
                }
                if let Some(sanitized) = gr_result.sanitized_input {
                    state.set("input", sanitized);
                }
            }

            // 执行节点（集成 NodeFixRunner 自动修复）
            let node_start = std::time::Instant::now();
            let mut result = self
                .execute_node_func(&current_node, node, state.clone())
                .await;
            result.duration_ms = node_start.elapsed().as_millis() as u64;

            execution_path.push(current_node.clone());

            // Guardrail 输出检查
            if result.success && !result.output.is_empty() {
                if let Some(gr) = &self.guardrail {
                    let gr_result = gr.check_output(&result.output).await;
                    if !gr_result.allowed {
                        success = false;
                        last_error = Some(format!("Guardrail 输出拦截: {}", gr_result.reason));
                        tracing::warn!("❌ Guardrail 输出拦截: {}", gr_result.reason);
                        break;
                    }
                    if let Some(sanitized) = gr_result.sanitized_output {
                        result.output = sanitized;
                    }
                }
            }

            // 发布事件：节点完成
            self.emit_event(
                state,
                AgentEventData::FlowStepExecuted {
                    step_index: step - 1,
                    step_name: current_node.clone(),
                    result: result.output.clone(),
                },
            )
            .await;

            if !result.success {
                success = false;
                last_error = result.error.clone();
                tracing::warn!("❌ Graph node {} failed: {:?}", current_node, last_error);
                break;
            }

            // 先提取路由（在移动 state_updates 之前）
            let route = result.route.clone();
            let output = result.output.clone();

            // 合并状态更新（条件边需要看到更新后的状态）
            if !result.state_updates.is_empty() {
                state.merge_with_reducers(result.state_updates, &self.reducers);
            }

            // 更新最终输出
            if !output.is_empty() {
                final_output = output;
            }

            // 用更新后的状态确定下一个节点
            let next_node = self.determine_next(&current_node, state, route.as_ref());

            // 保存检查点
            let checkpoint = Checkpoint {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                completed_node: current_node.clone(),
                next_node: next_node.clone(),
                state: state.clone(),
                timestamp: Utc::now(),
                step,
            };
            self.checkpoint_store.save(checkpoint).await.ok();

            // 确定下一步
            match next_node {
                Some(next) => {
                    current_node = next;
                }
                None => {
                    // 到达结束节点
                    self.emit_event(
                        state,
                        AgentEventData::FlowCompleted {
                            output: final_output.clone(),
                            iterations: step,
                        },
                    )
                    .await;

                    break;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        // 发布完成事件
        if success {
            self.emit_event(
                state,
                AgentEventData::FlowCompleted {
                    output: final_output.clone(),
                    iterations: step,
                },
            )
            .await;
        }

        Ok(GraphOutput {
            output: final_output,
            state: state.clone(),
            success,
            total_steps: step,
            duration_ms,
            execution_path,
            error: last_error,
        })
    }

    /// 确定下一个节点
    fn determine_next(
        &self,
        current: &str,
        state: &GraphState,
        route: Option<&Route>,
    ) -> Option<String> {
        // 1. 优先使用节点返回的显式路由
        if let Some(route) = route {
            return match route {
                Route::To(node) => Some(node.clone()),
                Route::End => None,
            };
        }

        // 2. 检查条件边
        if let Some(cond_edge) = self.conditional_edges.get(current) {
            return match (cond_edge.condition)(state) {
                Route::To(node) => Some(node),
                Route::End => None,
            };
        }

        // 3. 使用静态边（取第一个）
        if let Some(targets) = self.edges.get(current) {
            if !targets.is_empty() {
                return Some(targets[0].clone());
            }
        }

        // 4. 无出口边，结束
        None
    }

    /// 确定所有下一个节点（支持 fan-out 并行）
    pub fn determine_next_all(
        &self,
        current: &str,
        state: &GraphState,
        route: Option<&Route>,
    ) -> Vec<String> {
        // 1. 显式路由（单路）
        if let Some(route) = route {
            return match route {
                Route::To(node) => vec![node.clone()],
                Route::End => vec![],
            };
        }

        // 2. 条件边（单路）
        if let Some(cond_edge) = self.conditional_edges.get(current) {
            return match (cond_edge.condition)(state) {
                Route::To(node) => vec![node],
                Route::End => vec![],
            };
        }

        // 3. 静态边（全部，支持 fan-out）
        if let Some(targets) = self.edges.get(current) {
            return targets.clone();
        }

        vec![]
    }

    /// 事件驱动执行图（豆包设计哲学：图+事件+Actor 真正融合）
    ///
    /// 三种执行模式对比：
    ///
    /// | 模式 | 方法 | 特点 |
    /// |------|------|------|
    /// | 直接执行 | `run()` | 串行，轻量，调试用 |
    /// | Actor 执行 | `run_with_actors()` | 并行，容错，紧耦合 |
    /// | **事件驱动** | `run_event_driven()` | **竞标制：Actor 池竞标上岗** |
    ///
    /// 事件驱动模式的核心变化（竞标制）：
    /// - 不再为每个节点创建固定 Actor
    /// - 全局 Actor 池（ActorRegistry）中的 Actor 竞争每个节点
    /// - 节点发布任务要求 → 所有 Actor 自评分数 → 最高分上台
    /// - 图节点不再预绑定专家，实现真正的解耦
    pub async fn run_event_driven(
        &self,
        state: GraphState,
        actor_registry: &ActorRegistry,
        expert_state: &ExpertState,
    ) -> Result<GraphOutput> {
        let bus = self
            .event_bus
            .clone()
            .ok_or_else(|| GraphError::BuildError("事件驱动模式需要 EventBus".into()))?;

        let run_id = format!("run_{}", chrono::Utc::now().timestamp_millis());

        // 不再为每个节点创建 EventDrivenActor
        // 全局 Actor 池（ActorRegistry）中的 Actor 通过竞标制竞争节点
        // 参见 EventDrivenScheduler::run_with_run_id 中的竞标逻辑

        // 创建事件驱动调度器（传入 ActorRegistry 用于竞标，ExpertState 用于 Actor 执行）
        let graph_arc = Arc::new(self.clone());
        let scheduler = super::actor::EventDrivenScheduler::new(
            graph_arc,
            bus.clone(),
            actor_registry,
            expert_state,
        );

        // 执行
        let result = scheduler.run_with_run_id(state, run_id).await;

        result
    }

    /// 使用 Actor 模型执行图（默认监督策略）
    ///
    /// 相比 `run()`，此方法：
    /// - 节点封装为 NodeActor，状态隔离
    /// - 支持 fan-out 并行执行（多后继节点同时运行）
    /// - 通过 Supervisor 提供故障恢复
    pub async fn run_with_actors(&self, state: GraphState) -> Result<GraphOutput> {
        self.run_with_actors_strategy(state, SupervisionStrategy::default())
            .await
    }

    /// 使用 Actor 模型 + 自定义监督策略执行图
    pub async fn run_with_actors_strategy(
        &self,
        mut state: GraphState,
        strategy: SupervisionStrategy,
    ) -> Result<GraphOutput> {
        let start = std::time::Instant::now();
        let entry = self.entry.as_ref().ok_or(GraphError::NoEntryNode)?;

        // 构建 Supervisor 并注册所有节点 Actor
        // 如果节点配置了校验器，使用 NodeFixRunner 包装节点函数
        let mut supervisor = Supervisor::new(strategy);
        if let Some(ref bus) = self.event_bus {
            supervisor = supervisor.with_event_bus(bus.clone());
        }
        for (name, node) in &self.nodes {
            let func = if let Some(validator) = self.node_validators.get(name) {
                self.wrap_node_with_fix(&name, node.func.clone(), validator.clone())
            } else {
                node.func.clone()
            };
            supervisor.spawn(name.clone(), func);
        }

        let mut execution_path = Vec::new();
        let mut node_visit_count: HashMap<String, usize> = HashMap::new();
        let mut final_output = String::new();
        let mut success = true;
        let mut last_error: Option<String> = None;
        let mut step = 0;
        let mut current_nodes = vec![entry.clone()];

        loop {
            if current_nodes.is_empty() {
                break;
            }

            step += 1;

            // 循环检测
            for node_name in &current_nodes {
                let visits = node_visit_count.entry(node_name.clone()).or_insert(0);
                *visits += 1;
                if *visits > self.max_iterations {
                    supervisor.shutdown().await;
                    return Err(GraphError::LoopDetected(
                        node_name.clone(),
                        *visits,
                        self.max_iterations,
                    )
                    .into());
                }
            }

            // 发布事件：批次开始
            self.emit_event(
                &state,
                AgentEventData::FlowStarted {
                    flow_type: format!("actor_batch:{}", step),
                    input: current_nodes.join(","),
                },
            )
            .await;

            // Guardrail 输入检查
            if let Some(gr) = &self.guardrail {
                let input_str = state.get("input").unwrap_or_default();
                let gr_result = gr.check_input(&input_str).await;
                if !gr_result.allowed {
                    success = false;
                    last_error = Some(format!("Guardrail 输入拦截: {}", gr_result.reason));
                    tracing::warn!("❌ Guardrail 输入拦截: {}", gr_result.reason);
                    break;
                }
                if let Some(sanitized) = gr_result.sanitized_input {
                    state.set("input", sanitized);
                }
            }

            // 并行发送 Execute 消息（Actor 天然并行）
            let mut pending = Vec::new();
            for node_name in &current_nodes {
                let handle = supervisor
                    .get(node_name)
                    .ok_or_else(|| GraphError::NodeNotFound(node_name.clone()))?;
                let (tx, rx) = tokio::sync::oneshot::channel();
                if handle
                    .addr
                    .send(NodeMessage::Execute {
                        state: state.clone(),
                        reply: tx,
                    })
                    .await
                    .is_err()
                {
                    tracing::warn!("⚠️ Actor '{}' mailbox closed", node_name);
                }
                pending.push((node_name.clone(), rx));
            }

            // 收集结果（Actor 已在并行执行）
            let mut results = Vec::new();
            for (node_name, rx) in pending {
                match rx.await {
                    Ok(result) => {
                        // Guardrail 输出检查
                        let mut result = result;
                        if result.success && !result.output.is_empty() {
                            if let Some(gr) = &self.guardrail {
                                let gr_result = gr.check_output(&result.output).await;
                                if !gr_result.allowed {
                                    success = false;
                                    last_error =
                                        Some(format!("Guardrail 输出拦截: {}", gr_result.reason));
                                    tracing::warn!("❌ Guardrail 输出拦截: {}", gr_result.reason);
                                    result.success = false;
                                    result.error = last_error.clone();
                                }
                                if let Some(sanitized) = gr_result.sanitized_output {
                                    result.output = sanitized;
                                }
                            }
                        }
                        results.push((node_name, result));
                    }
                    Err(_) => {
                        success = false;
                        last_error = Some(format!("Actor '{}' dropped", node_name));
                        break;
                    }
                }
            }

            if !success {
                break;
            }

            // 处理结果
            let mut next_nodes = Vec::new();
            for (node_name, result) in results {
                execution_path.push(node_name.clone());

                if !result.success {
                    success = false;
                    last_error = result.error.clone();
                    tracing::warn!("节点 '{}' 执行失败: {:?}", node_name, last_error);

                    // 尝试执行补偿节点
                    if let Some(comp_node) = self.compensation_node(&node_name) {
                        tracing::info!("执行补偿节点 '{}' 处理 '{}' 的失败", comp_node, node_name);
                        if let Some(comp_result) = self
                            .execute_node_with_supervisor(
                                &node_name,
                                comp_node,
                                &mut supervisor,
                                &state,
                            )
                            .await
                        {
                            if comp_result.success {
                                if !comp_result.state_updates.is_empty() {
                                    state.merge_with_reducers(
                                        comp_result.state_updates,
                                        &self.reducers,
                                    );
                                }
                            } else {
                                tracing::error!(
                                    "补偿节点 '{}' 执行失败: {:?}",
                                    comp_node,
                                    comp_result.error
                                );
                            }
                        }
                    }

                    // 尝试降级分支
                    if let Some(fallback_node) = self.fallback_node(&node_name) {
                        tracing::info!("节点 '{}' 失败，降级到 '{}'", node_name, fallback_node);
                        next_nodes.push(fallback_node.clone());
                        last_error = None;
                        continue;
                    }

                    // 无降级分支，调用监督策略
                    let should_continue = supervisor
                        .handle_failure(&node_name, last_error.as_deref().unwrap_or("unknown"))
                        .await;
                    if !should_continue {
                        break;
                    }
                    continue;
                }

                // 先提取路由和输出（在移动 state_updates 之前）
                let route = result.route.clone();
                let output = result.output.clone();

                // 合并状态（条件边需要看到更新后的状态）
                if !result.state_updates.is_empty() {
                    state.merge_with_reducers(result.state_updates, &self.reducers);
                }

                // 更新输出
                if !output.is_empty() {
                    final_output = output;
                }

                // 用更新后的状态确定下一批节点（fan-out）
                let nexts = self.determine_next_all(&node_name, &state, route.as_ref());
                next_nodes.extend(nexts);
            }

            if !success {
                break;
            }

            // fan-in 去重：同一批中同一节点只执行一次
            // (DAG 中多个上游指向同一节点时，只执行一次)
            let mut seen = std::collections::HashSet::new();
            current_nodes = next_nodes
                .into_iter()
                .filter(|n| seen.insert(n.clone()))
                .collect();
        }

        // 发布完成事件
        if success {
            self.emit_event(
                &state,
                AgentEventData::FlowCompleted {
                    output: final_output.clone(),
                    iterations: step,
                },
            )
            .await;
        }

        supervisor.shutdown().await;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(GraphOutput {
            output: final_output,
            state,
            success,
            total_steps: step,
            duration_ms,
            execution_path,
            error: last_error,
        })
    }

    /// 通过 Supervisor 执行单个节点（用于补偿节点执行）
    async fn execute_node_with_supervisor(
        &self,
        _original_node: &str,
        node_name: &str,
        supervisor: &mut Supervisor,
        state: &GraphState,
    ) -> Option<NodeResult> {
        if let Some(handle) = supervisor.get(node_name) {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if handle
                .addr
                .send(NodeMessage::Execute {
                    state: state.clone(),
                    reply: tx,
                })
                .await
                .is_err()
            {
                tracing::warn!("⚠️ Actor '{}' mailbox closed (补偿节点)", node_name);
                return None;
            }
            match rx.await {
                Ok(result) => {
                    tracing::info!("补偿节点 '{}' 执行完成: {}", node_name, result.success);
                    Some(result)
                }
                Err(_) => {
                    tracing::error!("补偿节点 '{}' 执行失败", node_name);
                    None
                }
            }
        } else {
            tracing::warn!("补偿节点 '{}' 未在 Supervisor 中注册", node_name);
            None
        }
    }

    /// 发布事件（从 GraphState 读 trace_id/session_id 关联，由 Orchestrator.dispatch_via_graph 注入）
    async fn emit_event(&self, state: &GraphState, data: AgentEventData) {
        if let Some(ref bus) = self.event_bus {
            let trace_id = state.get("trace_id");
            let session_id = state.get("session_id");
            match trace_id {
                Some(tid) if !tid.is_empty() => {
                    bus.emit_with_trace(data, tid, session_id).await;
                }
                _ => bus.emit(data).await,
            }
        }
    }

    /// 获取图结构（用于可视化）
    pub fn structure(&self) -> GraphStructure {
        let mut nodes: Vec<String> = self.nodes.keys().cloned().collect();
        nodes.sort();
        let mut conditional_edges: Vec<String> = self.conditional_edges.keys().cloned().collect();
        conditional_edges.sort();
        GraphStructure {
            name: self.name.clone(),
            nodes,
            edges: self
                .edges
                .iter()
                .flat_map(|(from, tos)| {
                    tos.iter()
                        .map(move |to| Edge::new(from.clone(), to.clone()))
                })
                .collect(),
            conditional_edges,
            entry: self.entry.clone(),
            end_nodes: self.end_nodes.clone(),
        }
    }

    /// 获取检查点存储
    pub fn checkpoint_store(&self) -> &Arc<dyn CheckpointStore> {
        &self.checkpoint_store
    }
}

/// 图结构信息（用于可视化）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStructure {
    pub name: String,
    pub nodes: Vec<String>,
    pub edges: Vec<Edge>,
    pub conditional_edges: Vec<String>,
    pub entry: Option<String>,
    pub end_nodes: Vec<String>,
}

/// 图构建器
pub struct GraphBuilder {
    name: String,
    nodes: HashMap<String, GraphNode>,
    node_tags: HashMap<String, Vec<String>>,
    edges: HashMap<String, Vec<String>>,
    conditional_edges: HashMap<String, ConditionalEdge>,
    entry: Option<String>,
    end_nodes: Vec<String>,
    reducers: HashMap<String, StateReducer>,
    event_bus: Option<Arc<EventBus>>,
    checkpoint_store: Option<Arc<dyn CheckpointStore>>,
    max_iterations: usize,
    compensation_nodes: HashMap<String, String>,
    fallback_nodes: HashMap<String, String>,
    node_validators: HashMap<String, Arc<dyn INodeValidator>>,
    max_fix_attempts: u32,
    guardrail: Option<Arc<dyn IGuardrail>>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            name: "graph".to_string(),
            nodes: HashMap::new(),
            node_tags: HashMap::new(),
            edges: HashMap::new(),
            conditional_edges: HashMap::new(),
            entry: None,
            end_nodes: Vec::new(),
            reducers: HashMap::new(),
            event_bus: None,
            checkpoint_store: None,
            max_iterations: 25,
            compensation_nodes: HashMap::new(),
            fallback_nodes: HashMap::new(),
            node_validators: HashMap::new(),
            max_fix_attempts: 0,
            guardrail: None,
        }
    }

    /// 设置图名称
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// 添加节点
    pub fn node<F, Fut>(mut self, name: impl Into<String>, func: F) -> Self
    where
        F: Fn(GraphState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = NodeResult> + Send + 'static,
    {
        let name = name.into();
        self.nodes.insert(name.clone(), GraphNode::new(name, func));
        self
    }

    /// 添加节点（带描述）
    pub fn node_with_desc<F, Fut>(
        mut self,
        name: impl Into<String>,
        desc: impl Into<String>,
        func: F,
    ) -> Self
    where
        F: Fn(GraphState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = NodeResult> + Send + 'static,
    {
        let name = name.into();
        self.nodes.insert(
            name.clone(),
            GraphNode::new(name, func).with_description(desc),
        );
        self
    }

    /// 为节点设置竞标标签（Actor 根据标签自评匹配度）
    pub fn node_tag(mut self, node_name: impl Into<String>, tags: Vec<String>) -> Self {
        self.node_tags.insert(node_name.into(), tags);
        self
    }

    /// 添加静态边
    pub fn edge(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        let from = from.into();
        let to = to.into();
        self.edges.entry(from).or_default().push(to.clone());
        // 记录结束节点：有入口边但没有出口边的节点
        self.end_nodes = self.compute_end_nodes();
        self
    }

    /// 添加条件边
    pub fn conditional_edge<F>(mut self, from: impl Into<String>, condition: F) -> Self
    where
        F: Fn(&GraphState) -> Route + Send + Sync + 'static,
    {
        let from = from.into();
        self.conditional_edges.insert(
            from.clone(),
            ConditionalEdge {
                from,
                condition: std::sync::Arc::new(condition),
            },
        );
        self
    }

    /// 设置入口节点
    pub fn entry(mut self, node: impl Into<String>) -> Self {
        self.entry = Some(node.into());
        self
    }

    /// 添加状态 Reducer
    pub fn reducer(mut self, key: impl Into<String>, reducer: StateReducer) -> Self {
        self.reducers.insert(key.into(), reducer);
        self
    }

    /// 设置事件总线
    pub fn event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// 设置检查点存储
    pub fn checkpoint_store(mut self, store: Arc<dyn CheckpointStore>) -> Self {
        self.checkpoint_store = Some(store);
        self
    }

    /// 设置最大循环次数
    pub fn max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    /// 设置补偿节点：节点失败时执行的补偿节点
    pub fn compensation(
        mut self,
        node: impl Into<String>,
        compensation: impl Into<String>,
    ) -> Self {
        self.compensation_nodes
            .insert(node.into(), compensation.into());
        self
    }

    /// 设置降级分支：节点失败时的备用节点
    pub fn fallback(mut self, node: impl Into<String>, fallback: impl Into<String>) -> Self {
        self.fallback_nodes.insert(node.into(), fallback.into());
        self
    }

    /// 为节点设置输出校验器
    pub fn validator(
        mut self,
        node: impl Into<String>,
        validator: Arc<dyn super::validator::INodeValidator>,
    ) -> Self {
        self.node_validators.insert(node.into(), validator);
        self
    }

    /// 设置自动修复最大尝试次数
    pub fn max_fix_attempts(mut self, max: u32) -> Self {
        self.max_fix_attempts = max;
        self
    }

    /// 设置安全护栏
    pub fn guardrail(mut self, guardrail: Arc<dyn IGuardrail>) -> Self {
        self.guardrail = Some(guardrail);
        self
    }

    /// 计算结束节点
    fn compute_end_nodes(&self) -> Vec<String> {
        let has_outgoing = |name: &str| -> bool {
            self.edges.contains_key(name) || self.conditional_edges.contains_key(name)
        };

        // 收集所有节点名
        let mut all_nodes: std::collections::HashSet<String> = self.nodes.keys().cloned().collect();

        // 收集有入口边的节点
        for targets in self.edges.values() {
            for t in targets {
                all_nodes.insert(t.clone());
            }
        }

        // 结束节点 = 有入口边但没有出口边
        all_nodes.into_iter().filter(|n| !has_outgoing(n)).collect()
    }

    /// 构建图
    pub fn build(self) -> Result<Graph> {
        let entry = self.entry.clone().ok_or(GraphError::NoEntryNode)?;

        // 验证入口节点存在
        if !self.nodes.contains_key(&entry) {
            return Err(
                GraphError::BuildError(format!("入口节点 '{}' 未在 nodes 中定义", entry)).into(),
            );
        }

        // 验证所有边的目标节点存在
        for (from, targets) in &self.edges {
            if !self.nodes.contains_key(from) {
                return Err(GraphError::BuildError(format!("边的源节点 '{}' 未定义", from)).into());
            }
            for to in targets {
                if !self.nodes.contains_key(to) {
                    return Err(GraphError::BuildError(format!(
                        "边的目标节点 '{}' 未定义（源: {}）",
                        to, from
                    ))
                    .into());
                }
            }
        }

        let end_nodes = self.compute_end_nodes();

        Ok(Graph {
            name: self.name,
            nodes: self.nodes,
            node_tags: self.node_tags,
            edges: self.edges,
            conditional_edges: self.conditional_edges,
            entry: Some(entry),
            end_nodes,
            reducers: self.reducers,
            event_bus: self.event_bus,
            checkpoint_store: self
                .checkpoint_store
                .unwrap_or_else(|| Arc::new(MemoryCheckpointStore::new())),
            max_iterations: self.max_iterations,
            compensation_nodes: self.compensation_nodes,
            fallback_nodes: self.fallback_nodes,
            node_validators: self.node_validators,
            max_fix_attempts: self.max_fix_attempts,
            guardrail: self.guardrail,
        })
    }
}

impl Default for GraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_simple_linear_graph() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("result_a") })
            .node("b", |_| async { NodeResult::ok("result_b") })
            .node("c", |_| async { NodeResult::ok("result_c") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-1", &mut GraphState::new())
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.output, "result_c");
        assert_eq!(output.execution_path, vec!["a", "b", "c"]);
        assert_eq!(output.total_steps, 3);
    }

    #[tokio::test]
    async fn test_conditional_routing() {
        let graph = GraphBuilder::new()
            .node("start", |mut state| async move {
                state.set("route", "retry");
                NodeResult::ok_with_state("started", state.data().clone())
            })
            .node("process", |_| async { NodeResult::ok("processed") })
            .node("review", |mut state| async move {
                state.set("route", "done");
                NodeResult::ok_with_state("reviewed", state.data().clone())
            })
            .edge("start", "process")
            .edge("process", "review")
            .conditional_edge("review", |state| match state.get("route").as_deref() {
                Some("retry") => Route::To("process".to_string()),
                _ => Route::End,
            })
            .entry("start")
            .max_iterations(10)
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-2", &mut GraphState::new())
            .await
            .unwrap();

        assert!(output.success);
        assert_eq!(output.output, "reviewed");
        // start → process → review (route=retry→设置为done)
        // 条件边基于执行后的状态判断: route="done" → End
        assert_eq!(output.execution_path, vec!["start", "process", "review"]);
    }

    #[tokio::test]
    async fn test_node_failure() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("ok_a") })
            .node("b", |_| async { NodeResult::err("boom") })
            .node("c", |_| async { NodeResult::ok("ok_c") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-3", &mut GraphState::new())
            .await
            .unwrap();

        assert!(!output.success);
        assert_eq!(output.execution_path, vec!["a", "b"]);
        assert!(output.error.unwrap().contains("boom"));
    }

    #[tokio::test]
    async fn test_loop_detection() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("loop") })
            .edge("a", "a")
            .entry("a")
            .max_iterations(3)
            .build()
            .unwrap();

        let result = graph.run_with_id("test-4", &mut GraphState::new()).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("LoopDetected") || err.contains("循环检测"));
    }

    #[tokio::test]
    async fn test_checkpoint_resume() {
        let store = Arc::new(MemoryCheckpointStore::new());

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = call_count.clone();

        let graph = GraphBuilder::new()
            .node("a", move |_| {
                let counter = call_count_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    NodeResult::ok("a")
                }
            })
            .node("b", |_| async { NodeResult::err("simulated failure") })
            .node("c", |_| async { NodeResult::ok("c") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .checkpoint_store(store.clone())
            .build()
            .unwrap();

        let run_id = "test-run-1";

        // 第一次执行：在 b 失败
        let output1 = graph
            .run_with_id(run_id, &mut GraphState::new())
            .await
            .unwrap();
        assert!(!output1.success);

        // 检查点应该保存了（a 完成后）
        let cp = store.get_latest(run_id).await;
        assert!(cp.is_some());

        // 模拟修复 b 节点后重新执行
        // 由于节点函数是闭包，这里只验证检查点存在
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_state_with_reducer() {
        use super::super::state::reducers;

        let graph = GraphBuilder::new()
            .node("a", |_| async {
                let mut updates = HashMap::new();
                updates.insert("messages".to_string(), serde_json::json!(["msg_a"]));
                NodeResult::ok_with_state("a", updates)
            })
            .node("b", |_| async {
                let mut updates = HashMap::new();
                updates.insert("messages".to_string(), serde_json::json!(["msg_b"]));
                NodeResult::ok_with_state("b", updates)
            })
            .edge("a", "b")
            .entry("a")
            .reducer("messages", reducers::append())
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-5", &mut GraphState::new())
            .await
            .unwrap();

        let messages = output.state.get_value("messages").unwrap();
        assert_eq!(messages, &serde_json::json!(["msg_a", "msg_b"]));
    }

    #[tokio::test]
    async fn test_event_bus_integration() {
        let bus = Arc::new(EventBus::new(64));
        let recorder = Arc::new(crate::event::EventRecorder::new());
        bus.subscribe(recorder.clone() as Arc<dyn crate::event::EventHandler>)
            .await;

        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("hello") })
            .node("b", |_| async { NodeResult::ok("world") })
            .edge("a", "b")
            .entry("a")
            .event_bus(bus.clone())
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-event-bus", &mut GraphState::new())
            .await
            .unwrap();

        assert!(output.success);

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        // 应该有 flow_started + flow_step_executed + flow_completed 事件
        assert!(
            recording.events.len() >= 2,
            "expected at least 2 events, got {}",
            recording.events.len()
        );
    }

    #[tokio::test]
    async fn test_graph_structure() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("a") })
            .node("b", |_| async { NodeResult::ok("b") })
            .node("c", |_| async { NodeResult::ok("c") })
            .edge("a", "b")
            .edge("b", "c")
            .conditional_edge("c", |_| Route::End)
            .entry("a")
            .build()
            .unwrap();

        let structure = graph.structure();

        assert_eq!(structure.nodes, vec!["a", "b", "c"]);
        assert_eq!(structure.entry, Some("a".to_string()));
        assert_eq!(structure.edges.len(), 2);
        assert_eq!(structure.conditional_edges, vec!["c"]);
    }

    #[tokio::test]
    async fn test_explicit_route() {
        let graph = GraphBuilder::new()
            .node("start", |_| async {
                NodeResult::ok("started").with_route("started", Route::To("skip".to_string()))
            })
            .node("middle", |_| async { NodeResult::ok("middle") })
            .node("skip", |_| async { NodeResult::ok("skipped") })
            .edge("start", "middle")
            .edge("middle", "skip")
            .entry("start")
            .max_iterations(10)
            .build()
            .unwrap();

        let output = graph
            .run_with_id("test-route", &mut GraphState::new())
            .await
            .unwrap();

        // start 使用显式路由跳转到 skip，跳过 middle
        assert_eq!(output.execution_path, vec!["start", "skip"]);
    }

    // ═══════════════════════════════════════════════════════════
    // Actor 模式测试
    // ═══════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_actor_linear_graph() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("result_a") })
            .node("b", |_| async { NodeResult::ok("result_b") })
            .node("c", |_| async { NodeResult::ok("result_c") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .build()
            .unwrap();

        let output = graph.run_with_actors(GraphState::new()).await.unwrap();

        assert!(output.success);
        assert_eq!(output.output, "result_c");
        assert_eq!(output.execution_path, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_actor_fan_out_parallel() {
        // fan-out: start → [b, c] → merge
        let graph = GraphBuilder::new()
            .node("start", |_| async { NodeResult::ok("started") })
            .node("b", |_| async {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                NodeResult::ok("b_done")
            })
            .node("c", |_| async {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                NodeResult::ok("c_done")
            })
            .edge("start", "b")
            .edge("start", "c")
            .entry("start")
            .build()
            .unwrap();

        let output = graph.run_with_actors(GraphState::new()).await.unwrap();

        assert!(output.success);
        // start → b, c 并行执行
        assert!(output.execution_path.contains(&"start".to_string()));
        assert!(output.execution_path.contains(&"b".to_string()));
        assert!(output.execution_path.contains(&"c".to_string()));
        assert_eq!(output.execution_path.len(), 3);
        // 并行执行总耗时应小于串行（100ms vs 串行 100ms+）
        // 这里只验证成功，不严格断言时间
    }

    #[tokio::test]
    async fn test_actor_node_failure_with_resume() {
        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("ok_a") })
            .node("b", |_| async { NodeResult::err("boom") })
            .node("c", |_| async { NodeResult::ok("ok_c") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .build()
            .unwrap();

        // Resume 策略：忽略错误继续
        let output = graph
            .run_with_actors_strategy(GraphState::new(), SupervisionStrategy::Resume)
            .await
            .unwrap();

        // b 失败但 Resume 策略继续，c 仍会执行
        assert!(!output.success);
        assert!(output.execution_path.contains(&"a".to_string()));
        assert!(output.execution_path.contains(&"b".to_string()));
    }

    #[tokio::test]
    async fn test_actor_conditional_routing() {
        let graph = GraphBuilder::new()
            .node("start", |mut state| async move {
                state.set("route", "go_b");
                NodeResult::ok_with_state("started", state.data().clone())
            })
            .node("b", |_| async { NodeResult::ok("b") })
            .node("c", |_| async { NodeResult::ok("c") })
            .conditional_edge("start", |state| match state.get("route").as_deref() {
                Some("go_b") => Route::To("b".to_string()),
                _ => Route::To("c".to_string()),
            })
            .entry("start")
            .build()
            .unwrap();

        let output = graph.run_with_actors(GraphState::new()).await.unwrap();

        assert!(output.success);
        assert_eq!(output.output, "b");
        assert_eq!(output.execution_path, vec!["start", "b"]);
    }

    #[tokio::test]
    async fn test_actor_event_bus_integration() {
        let bus = Arc::new(EventBus::new(64));
        let recorder = Arc::new(crate::event::EventRecorder::new());
        bus.subscribe(recorder.clone() as Arc<dyn crate::event::EventHandler>)
            .await;

        let graph = GraphBuilder::new()
            .node("a", |_| async { NodeResult::ok("hello") })
            .node("b", |_| async { NodeResult::ok("world") })
            .edge("a", "b")
            .entry("a")
            .event_bus(bus)
            .build()
            .unwrap();

        let output = graph.run_with_actors(GraphState::new()).await.unwrap();
        assert!(output.success);

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        assert!(
            recording.events.len() >= 2,
            "expected at least 2 events, got {}",
            recording.events.len()
        );
    }
}
