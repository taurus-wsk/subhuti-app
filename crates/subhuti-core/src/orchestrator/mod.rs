pub mod rule_engine;
pub mod strategies;

// ──────────────────────────────────────────────────────────────
// 架构概念澄清
// ──────────────────────────────────────────────────────────────
//
// 本模块命名："Orchestrator"（编排者）
// 隐喻定位：命运编织者（Destiny Weaver）
//
// 为什么不是调度器？
//   - 调度器（Scheduler）的职责是：决定"何时执行"、"执行什么"
//   - 真正的调度者是外部调用方（HTTP Handler / CLI / 用户）
//
// 为什么不是引擎？
//   - 引擎（Engine）是执行器，直接做具体工作
//   - Graph 和 RuleEngine 才是真正的执行引擎
//
// 编排者的职责（命运编织者）：
//   - 接到主题（用户问题）后，决定走哪条命运之路
//   - 不自己执行，只决定执行的方向和路径
//
// 三层职责（命运的三个阶段）：
//   Layer 1: TaskUnderstanding（命运占卜）
//     输入：用户问题
//     输出：任务画像（领域标签、任务类型、复杂度）
//   Layer 2: ExecutionPlanning（命运抉择）
//     输入：任务画像 + 可用专家/图
//     输出：执行计划（执行策略、专家链、步骤顺序）
//   Layer 3: ExecutionMonitoring（命运守护）
//     输入：执行计划
//     输出：执行结果（含超时、重试、失败处理）
//
// 两种命运之路：
//   - 规则之路：RuleEngine 驱动的专家链顺序执行
//   - 图之路：Graph 驱动的节点流程执行
//
// 代码标识符：`Orchestrator`（英文原意：编排者）
// 概念隐喻：命运编织者（Destiny Weaver）
//
// 为何不改名？
//   - `Orchestrator` 是 AI Agent 领域的行业标准术语
//   - 中文翻译"调度器"曾造成误解，但英文词本身是准确的
//   - 保持代码稳定，通过注释传递概念意图即可

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use self::strategies::{SemanticCandidate, SemanticRouter};
use crate::event::{AgentEventData, EventBus};
use crate::graph::{Graph, GraphOutput, GraphState};
use crate::memory::Memory;
use crate::runtime::{llm::LLM, session::Session};
use crate::vertical::{AssetLibrary, ProjectMemory, ToolRegistry, WorkflowStore};
pub use rule_engine::{
    DefaultDispatchRule, DefaultExecutionRule, DefaultTaskAnalysisRule, DispatchPlan, DispatchRule,
    DispatchStrategy, ExecutionResult, ExecutionRule, ResultStrategy, RuleConfig, RuleEngine, Step,
    TaskAnalysisRule, TaskProfile,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OrchestrationResult {
    pub strategy: String,
    pub expert_chain: Vec<String>,
    pub output: String,
    pub tokens: TokenUsage,
    pub expert_outputs: Vec<String>,
    pub success: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub input: String,
    pub ctx_id: String,
    pub session: Session,
    pub metadata: HashMap<String, String>,
}

impl AgentContext {
    pub fn new(input: &str, ctx_id: &str) -> Self {
        Self {
            input: input.to_string(),
            ctx_id: ctx_id.to_string(),
            session: Session::new(ctx_id),
            metadata: HashMap::new(),
        }
    }

    pub fn set_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
}

/// 专家快照（强类型 DTO）——替代原先的 serde_json::Value 中转
///
/// 从 `Arc<dyn ExpertAgent>` 提取只读元数据，供框架门面层和适配器层使用。
/// 不持有 trait 对象，避免把框架内部类型泄漏到更高层。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameworkExpertInfo {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub skills: Vec<SkillInfo>,
}

impl FrameworkExpertInfo {
    /// 从 ExpertAgent trait 对象提取快照
    pub fn from_agent(agent: &dyn ExpertAgent) -> Self {
        Self {
            id: agent.id().to_string(),
            name: agent.name().to_string(),
            tags: agent.tags().to_vec(),
            skills: agent.skills().to_vec(),
        }
    }
}

#[async_trait]
pub trait ExpertAgent: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn tags(&self) -> &[String];
    fn skills(&self) -> &[SkillInfo] {
        &[]
    }
    async fn run(&self, ctx: &mut AgentContext, state: &ExpertState) -> crate::Result<String>;
}

#[derive(Clone)]
pub struct ExpertState {
    llm: Option<Arc<dyn LLM>>,
    memory: Arc<dyn Memory>,
    event_bus: Option<Arc<EventBus>>,
    asset_library: Option<Arc<dyn AssetLibrary>>,
    project_memory: Option<Arc<dyn ProjectMemory>>,
    tool_registry: Option<Arc<dyn ToolRegistry>>,
    workflow_store: Option<Arc<dyn WorkflowStore>>,
}

impl ExpertState {
    pub fn builder(memory: Arc<dyn Memory>) -> ExpertStateBuilder {
        ExpertStateBuilder::new(memory)
    }

    pub fn llm(&self) -> Option<&Arc<dyn LLM>> {
        self.llm.as_ref()
    }

    pub fn memory(&self) -> &Arc<dyn Memory> {
        &self.memory
    }

    pub fn event_bus(&self) -> Option<&Arc<EventBus>> {
        self.event_bus.as_ref()
    }

    pub fn asset_library(&self) -> Option<&Arc<dyn AssetLibrary>> {
        self.asset_library.as_ref()
    }

    pub fn project_memory(&self) -> Option<&Arc<dyn ProjectMemory>> {
        self.project_memory.as_ref()
    }

    pub fn tool_registry(&self) -> Option<&Arc<dyn ToolRegistry>> {
        self.tool_registry.as_ref()
    }

    pub fn workflow_store(&self) -> Option<&Arc<dyn WorkflowStore>> {
        self.workflow_store.as_ref()
    }

    pub fn llm_cloned(&self) -> Option<Arc<dyn LLM>> {
        self.llm.clone()
    }

    pub fn memory_cloned(&self) -> Arc<dyn Memory> {
        self.memory.clone()
    }

    pub fn event_bus_cloned(&self) -> Option<Arc<EventBus>> {
        self.event_bus.clone()
    }

    pub fn asset_library_cloned(&self) -> Option<Arc<dyn AssetLibrary>> {
        self.asset_library.clone()
    }

    pub fn project_memory_cloned(&self) -> Option<Arc<dyn ProjectMemory>> {
        self.project_memory.clone()
    }

    pub fn tool_registry_cloned(&self) -> Option<Arc<dyn ToolRegistry>> {
        self.tool_registry.clone()
    }

    pub fn workflow_store_cloned(&self) -> Option<Arc<dyn WorkflowStore>> {
        self.workflow_store.clone()
    }
}

pub struct ExpertStateBuilder {
    llm: Option<Arc<dyn LLM>>,
    memory: Arc<dyn Memory>,
    event_bus: Option<Arc<EventBus>>,
    asset_library: Option<Arc<dyn AssetLibrary>>,
    project_memory: Option<Arc<dyn ProjectMemory>>,
    tool_registry: Option<Arc<dyn ToolRegistry>>,
    workflow_store: Option<Arc<dyn WorkflowStore>>,
}

impl ExpertStateBuilder {
    pub fn new(memory: Arc<dyn Memory>) -> Self {
        Self {
            llm: None,
            memory,
            event_bus: None,
            asset_library: None,
            project_memory: None,
            tool_registry: None,
            workflow_store: None,
        }
    }

    pub fn llm(mut self, llm: Arc<dyn LLM>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn optional_llm(mut self, llm: Option<Arc<dyn LLM>>) -> Self {
        self.llm = llm;
        self
    }

    pub fn event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn asset_library(mut self, library: Arc<dyn AssetLibrary>) -> Self {
        self.asset_library = Some(library);
        self
    }

    pub fn project_memory(mut self, pm: Arc<dyn ProjectMemory>) -> Self {
        self.project_memory = Some(pm);
        self
    }

    pub fn tool_registry(mut self, registry: Arc<dyn ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn workflow_store(mut self, store: Arc<dyn WorkflowStore>) -> Self {
        self.workflow_store = Some(store);
        self
    }

    pub fn build(self) -> ExpertState {
        ExpertState {
            llm: self.llm,
            memory: self.memory,
            event_bus: self.event_bus,
            asset_library: self.asset_library,
            project_memory: self.project_memory,
            tool_registry: self.tool_registry,
            workflow_store: self.workflow_store,
        }
    }
}

pub trait FromState<'a>: Sized {
    fn from_state(state: &'a ExpertState) -> crate::Result<Self>;
}

pub struct Llm(pub Arc<dyn LLM>);

impl<'a> FromState<'a> for Llm {
    fn from_state(state: &'a ExpertState) -> crate::Result<Self> {
        state
            .llm_cloned()
            .map(Llm)
            .ok_or_else(|| crate::Error::Runtime("LLM 未配置".to_string()))
    }
}

pub struct MemoryRef<'a>(pub &'a dyn Memory);

impl<'a> FromState<'a> for MemoryRef<'a> {
    fn from_state(state: &'a ExpertState) -> crate::Result<Self> {
        Ok(MemoryRef(&**state.memory()))
    }
}

pub struct EventBusRef<'a>(pub &'a EventBus);

impl<'a> FromState<'a> for EventBusRef<'a> {
    fn from_state(state: &'a ExpertState) -> crate::Result<Self> {
        state
            .event_bus()
            .map(|b| EventBusRef(&**b))
            .ok_or_else(|| crate::Error::Runtime("EventBus 未配置".to_string()))
    }
}

pub struct Orchestrator {
    agent_registry: AgentRegistry,
    graph_registry: GraphRegistry,
    event_bus: Option<Arc<EventBus>>,
    semantic_router: SemanticRouter,
    rule_engine: RuleEngine,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            graph_registry: GraphRegistry::new(),
            event_bus: None,
            semantic_router: SemanticRouter::new_disabled(),
            rule_engine: RuleEngine::with_defaults(),
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_semantic_router(mut self, router: SemanticRouter) -> Self {
        self.semantic_router = router;
        self
    }

    pub fn with_rule_engine(mut self, engine: RuleEngine) -> Self {
        self.rule_engine = engine;
        self
    }

    /// 获取规则引擎引用
    pub fn rule_engine(&self) -> &RuleEngine {
        &self.rule_engine
    }

    /// 运行时替换任务分析规则（Layer 1）
    pub fn set_analysis_rule(&mut self, rule: Arc<dyn TaskAnalysisRule>) {
        self.rule_engine.set_analysis_rule(rule);
    }

    /// 运行时替换调度决策规则（Layer 2）
    pub fn set_dispatch_rule(&mut self, rule: Arc<dyn DispatchRule>) {
        self.rule_engine.set_dispatch_rule(rule);
    }

    /// 运行时替换执行监控规则（Layer 3）
    pub fn set_execution_rule(&mut self, rule: Arc<dyn ExecutionRule>) {
        self.rule_engine.set_execution_rule(rule);
    }

    pub fn register_agent(&mut self, agent: Arc<dyn ExpertAgent>) {
        self.agent_registry.register(agent);
    }

    pub fn agent_count(&self) -> usize {
        self.agent_registry.agent_count()
    }

    /// 获取真实 Agent trait 对象列表（用于内部调度/图注册）
    pub fn list_experts(&self) -> Vec<Arc<dyn ExpertAgent>> {
        self.agent_registry.list_agents()
    }

    /// 获取专家快照列表（强类型 DTO，用于适配器/上层查询）
    pub fn list_expert_snapshots(&self) -> Vec<FrameworkExpertInfo> {
        self.agent_registry.list_agent_snapshots()
    }

    pub fn register_graph(&mut self, graph: Graph) {
        self.graph_registry.register(graph);
    }

    pub async fn dispatch(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
    ) -> OrchestrationResult {
        let input = &ctx.input.clone();

        self.emit_event(
            ctx,
            AgentEventData::UserMessage {
                message: input.clone(),
            },
        )
        .await;

        // 优先尝试图路由（语义路由 + 关键词匹配）
        if let Some(graph) = self.try_semantic_graph_routing(input).await {
            tracing::debug!("语义路由匹配到图: {}", graph.name());
            return self.dispatch_via_graph(ctx, state, &graph).await;
        }

        let graph = self.graph_registry.find_matching_graph(input).await;
        if let Some(graph) = graph {
            return self.dispatch_via_graph(ctx, state, &graph).await;
        }

        // 无图匹配，使用 RuleEngine 三层调度
        let agents = self.agent_registry.list_agents();

        // Layer 1: 任务分析
        let profile = match self.rule_engine.analyze_task(input) {
            Ok(p) => p,
            Err(e) => {
                return OrchestrationResult {
                    strategy: "rule_engine_layer1_failed".into(),
                    expert_chain: Vec::new(),
                    output: e.to_string(),
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: false,
                };
            }
        };

        // Layer 2: 调度决策
        let plan = match self.rule_engine.decide_strategy(&profile, &agents) {
            Ok(p) => p,
            Err(e) => {
                return OrchestrationResult {
                    strategy: "rule_engine_layer2_failed".into(),
                    expert_chain: Vec::new(),
                    output: e.to_string(),
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: false,
                };
            }
        };

        if plan.steps.is_empty() {
            // 尝试语义路由作为兜底
            if let Some(agent) = self.try_semantic_agent_routing(input).await {
                return self.execute_agent(ctx, state, agent).await;
            }
            return OrchestrationResult {
                strategy: "fallback".to_string(),
                expert_chain: Vec::new(),
                output: "未找到匹配的专家".to_string(),
                tokens: TokenUsage::default(),
                expert_outputs: Vec::new(),
                success: false,
            };
        }

        // 发布匹配事件
        self.emit_event(
            ctx,
            AgentEventData::ChainSelected {
                chain_name: format!("rule_engine:{:?}", plan.strategy),
                strategy: format!("{:?}", plan.strategy),
            },
        )
        .await;

        // Layer 3: 执行监控
        self.execute_with_rule_engine(ctx, state, &plan).await
    }

    /// 通过 RuleEngine 执行调度计划（Layer 3）
    async fn execute_with_rule_engine(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
        plan: &DispatchPlan,
    ) -> OrchestrationResult {
        let start = Instant::now();
        let mut results: Vec<String> = Vec::new();
        let mut expert_chain: Vec<String> = Vec::new();
        let mut step_index = 0usize;

        for step in &plan.steps {
            // 检查步骤数限制
            if let Err(e) = self.rule_engine.check_max_steps(step_index) {
                tracing::warn!("[执行监控·Layer3] {}", e);
                break;
            }

            // 检查总超时
            if let Err(e) = self.rule_engine.check_timeout(start.elapsed()) {
                tracing::warn!("[执行监控·Layer3] {}", e);
                break;
            }

            let agent = match self.agent_registry.get_by_id(&step.agent_id) {
                Some(a) => a,
                None => {
                    tracing::warn!("[执行监控·Layer3] 专家 {} 未找到", step.agent_id);
                    if !self.rule_engine.should_continue() {
                        break;
                    }
                    continue;
                }
            };

            // 设置输入
            if step.use_previous_output && !results.is_empty() {
                ctx.input = results.last().cloned().unwrap_or_default();
            }

            // 单步超时
            let step_timeout = self.rule_engine.per_step_timeout();

            self.emit_event(
                ctx,
                AgentEventData::AgentStarted {
                    agent_id: agent.id().to_string(),
                    input: ctx.input.clone(),
                },
            )
            .await;

            let agent_id = agent.id().to_string();
            let exec_result = tokio::time::timeout(step_timeout, agent.run(ctx, state)).await;

            let duration_ms = start.elapsed().as_millis() as u64;

            match exec_result {
                Ok(Ok(output)) => {
                    results.push(output.clone());
                    expert_chain.push(agent_id.clone());
                    self.emit_event(
                        ctx,
                        AgentEventData::AgentCompleted {
                            agent_id,
                            output,
                            duration_ms,
                        },
                    )
                    .await;
                }
                Ok(Err(e)) => {
                    tracing::warn!("[执行监控·Layer3] 专家 {} 执行失败: {}", agent_id, e);
                    self.emit_event(
                        ctx,
                        AgentEventData::AgentFailed {
                            agent_id,
                            error: e.to_string(),
                            duration_ms,
                        },
                    )
                    .await;
                    if !self.rule_engine.should_continue() {
                        break;
                    }
                }
                Err(_) => {
                    tracing::warn!("[执行监控·Layer3] 专家 {} 执行超时", agent_id);
                    self.emit_event(
                        ctx,
                        AgentEventData::AgentFailed {
                            agent_id,
                            error: "执行超时".to_string(),
                            duration_ms: step_timeout.as_millis() as u64,
                        },
                    )
                    .await;
                    if !self.rule_engine.should_continue() {
                        break;
                    }
                }
            }

            step_index += 1;
        }

        let output = self.rule_engine.merge_results(&results);
        let success = !results.is_empty();

        OrchestrationResult {
            strategy: format!("rule_engine:{:?}", plan.strategy),
            expert_chain,
            output,
            tokens: TokenUsage::default(),
            expert_outputs: results,
            success,
        }
    }

    async fn dispatch_via_graph(
        &self,
        ctx: &mut AgentContext,
        _state: &ExpertState,
        graph: &Graph,
    ) -> OrchestrationResult {
        self.emit_event(
            ctx,
            AgentEventData::ChainSelected {
                chain_name: graph.name().to_string(),
                strategy: "graph".to_string(),
            },
        )
        .await;

        let mut graph_state = GraphState::new();
        graph_state.set("input", &*ctx.input);
        if let Some(tid) = ctx.metadata.get("trace_id") {
            graph_state.set("trace_id", tid.clone());
        }
        if let Some(sid) = ctx.metadata.get("session_id") {
            graph_state.set("session_id", sid.clone());
        }

        match self.execute_graph(graph, graph_state).await {
            Ok(output) => OrchestrationResult {
                strategy: format!("graph:{}", graph.name()),
                expert_chain: output.execution_path,
                output: output.output,
                tokens: TokenUsage::default(),
                expert_outputs: Vec::new(),
                success: output.success,
            },
            Err(e) => OrchestrationResult {
                strategy: format!("graph:{}", graph.name()),
                expert_chain: Vec::new(),
                output: e.to_string(),
                tokens: TokenUsage::default(),
                expert_outputs: Vec::new(),
                success: false,
            },
        }
    }

    async fn emit_event(&self, ctx: &AgentContext, data: AgentEventData) {
        if let Some(ref bus) = self.event_bus {
            let trace_id = ctx.metadata.get("trace_id").cloned();
            let session_id = ctx.metadata.get("session_id").cloned();
            match trace_id {
                Some(tid) => bus.emit_with_trace(data, tid, session_id).await,
                None => bus.emit(data).await,
            }
        }
    }

    /// 从 GraphState 读 trace_id 发布事件（用于 GraphOrchestrator.execute_graph，无 AgentContext）
    async fn emit_event_with_state(&self, state: &GraphState, data: AgentEventData) {
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

    async fn try_semantic_graph_routing(&self, input: &str) -> Option<Arc<Graph>> {
        if !self.semantic_router.enabled() {
            return None;
        }
        let candidates = self.graph_registry.list_candidates();
        if candidates.is_empty() {
            return None;
        }
        if let Some(result) = self
            .semantic_router
            .match_candidate(input, candidates)
            .await
        {
            tracing::debug!(
                "语义路由匹配图: {} (置信度: {:.2}) - {}",
                result.target_id,
                result.confidence,
                result.reasoning
            );
            self.graph_registry.get_by_id(&result.target_id)
        } else {
            None
        }
    }

    async fn try_semantic_agent_routing(&self, input: &str) -> Option<Arc<dyn ExpertAgent>> {
        if !self.semantic_router.enabled() {
            return None;
        }
        let candidates = self.agent_registry.list_candidates();
        if candidates.is_empty() {
            return None;
        }
        if let Some(result) = self
            .semantic_router
            .match_candidate(input, candidates)
            .await
        {
            tracing::debug!(
                "语义路由匹配专家: {} (置信度: {:.2}) - {}",
                result.target_id,
                result.confidence,
                result.reasoning
            );
            self.agent_registry.get_by_id(&result.target_id)
        } else {
            None
        }
    }

    async fn execute_agent(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
        agent: Arc<dyn ExpertAgent>,
    ) -> OrchestrationResult {
        let input = &ctx.input;

        self.emit_event(
            ctx,
            AgentEventData::AgentStarted {
                agent_id: agent.id().to_string(),
                input: input.clone(),
            },
        )
        .await;

        let start = std::time::Instant::now();
        let output = agent
            .run(ctx, state)
            .await
            .unwrap_or_else(|e| e.to_string());
        let duration_ms = start.elapsed().as_millis() as u64;

        self.emit_event(
            ctx,
            AgentEventData::AgentCompleted {
                agent_id: agent.id().to_string(),
                output: output.clone(),
                duration_ms,
            },
        )
        .await;

        OrchestrationResult {
            strategy: "direct".to_string(),
            expert_chain: vec![agent.id().to_string()],
            output: output.clone(),
            tokens: TokenUsage::default(),
            expert_outputs: vec![output],
            success: true,
        }
    }
}

pub struct AgentRegistry {
    agents: HashMap<String, Arc<dyn ExpertAgent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn register(&mut self, agent: Arc<dyn ExpertAgent>) {
        self.agents.insert(agent.id().to_string(), agent);
    }

    pub fn find_matching_experts(&self, input: &str) -> Vec<Arc<dyn ExpertAgent>> {
        let input_lower = input.to_lowercase();
        let mut matched: Vec<(Arc<dyn ExpertAgent>, u32)> = Vec::new();

        for agent in self.agents.values() {
            let mut score = 0;
            for tag in agent.tags() {
                if input_lower.contains(&tag.to_lowercase()) {
                    score += 1;
                }
            }
            if score > 0 {
                matched.push((agent.clone(), score));
            }
        }

        matched.sort_by_key(|b| std::cmp::Reverse(b.1));
        matched.into_iter().map(|(a, _)| a).collect()
    }

    pub fn list_candidates(&self) -> Vec<SemanticCandidate> {
        self.agents
            .values()
            .map(|agent| SemanticCandidate {
                id: agent.id().to_string(),
                name: agent.name().to_string(),
                description: format!("专家: {}", agent.name()),
                tags: agent.tags().to_vec(),
            })
            .collect()
    }

    pub fn get_by_id(&self, id: &str) -> Option<Arc<dyn ExpertAgent>> {
        self.agents.get(id).cloned()
    }

    /// 列出所有已注册专家（用于 RuleEngine 调度，返回真实 Agent）
    pub fn list_agents(&self) -> Vec<Arc<dyn ExpertAgent>> {
        self.agents.values().cloned().collect()
    }

    /// 列出所有已注册专家的只读快照（用于适配器/表现层，不泄漏 trait 对象）
    pub fn list_agent_snapshots(&self) -> Vec<FrameworkExpertInfo> {
        self.agents
            .values()
            .map(|a| FrameworkExpertInfo::from_agent(a.as_ref()))
            .collect()
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
}

pub struct GraphRegistry {
    graphs: RwLock<HashMap<String, Arc<Graph>>>,
}

impl GraphRegistry {
    pub fn new() -> Self {
        Self {
            graphs: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, graph: Graph) {
        let name = graph.name().to_string();
        let arc_graph = Arc::new(graph);
        self.graphs.write().unwrap().insert(name, arc_graph);
    }

    pub fn get(&self, name: &str) -> Option<Arc<Graph>> {
        self.graphs.read().unwrap().get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.graphs.read().unwrap().keys().cloned().collect()
    }

    pub async fn find_matching_graph(&self, input: &str) -> Option<Arc<Graph>> {
        let graphs = self.graphs.read().unwrap();
        if graphs.is_empty() {
            return None;
        }
        if graphs.len() == 1 {
            return graphs.values().next().cloned();
        }

        let input_lower = input.to_lowercase();
        for (name, graph) in &*graphs {
            if input_lower.contains(&name.to_lowercase()) {
                return Some(graph.clone());
            }
        }

        let code_keywords = ["代码", "编程", "开发", "写代码", "code", "programming"];
        let review_keywords = ["评审", "审查", "审核", "review", "audit"];

        for (name, graph) in &*graphs {
            if name.to_lowercase().contains("code") || name.to_lowercase().contains("dev") {
                for kw in &code_keywords {
                    if input_lower.contains(kw) {
                        return Some(graph.clone());
                    }
                }
            }
            if name.to_lowercase().contains("review") || name.to_lowercase().contains("audit") {
                for kw in &review_keywords {
                    if input_lower.contains(kw) {
                        return Some(graph.clone());
                    }
                }
            }
        }

        graphs.values().next().cloned()
    }

    pub fn list_candidates(&self) -> Vec<SemanticCandidate> {
        let graphs = self.graphs.read().unwrap();
        graphs
            .values()
            .map(|graph| SemanticCandidate {
                id: graph.name().to_string(),
                name: graph.name().to_string(),
                description: format!("工作流: {}", graph.name()),
                tags: Vec::new(),
            })
            .collect()
    }

    pub fn get_by_id(&self, id: &str) -> Option<Arc<Graph>> {
        self.graphs.read().unwrap().get(id).cloned()
    }
}

#[async_trait]
pub trait GraphOrchestrator: Send + Sync {
    async fn route_by_graph(&self, input: &str) -> Option<Arc<Graph>>;
    async fn execute_graph(&self, graph: &Graph, state: GraphState) -> crate::Result<GraphOutput>;
    fn register_graph(&self, graph: Graph);
    fn get_graph(&self, name: &str) -> Option<Arc<Graph>>;
    fn list_graphs(&self) -> Vec<String>;
}

#[async_trait]
impl GraphOrchestrator for Orchestrator {
    async fn route_by_graph(&self, input: &str) -> Option<Arc<Graph>> {
        self.graph_registry.find_matching_graph(input).await
    }

    async fn execute_graph(&self, graph: &Graph, state: GraphState) -> crate::Result<GraphOutput> {
        // 走 run_event_driven 时 GraphStarted/GraphCompleted 由 scheduler 负责 emit（带 trace_id）
        // 走 graph.run() 时 scheduler 不参与，由这里补 emit
        let use_event_driven = graph.event_bus().is_some();

        if !use_event_driven {
            self.emit_event_with_state(
                &state,
                AgentEventData::GraphStarted {
                    graph_name: graph.name().to_string(),
                    run_id: uuid::Uuid::new_v4().to_string(),
                    entry_node: graph.entry.clone().unwrap_or_default(),
                },
            )
            .await;
        }

        let result = if use_event_driven {
            graph.run_event_driven(state).await
        } else {
            graph.run(state).await
        };

        if !use_event_driven {
            if let Ok(output) = &result {
                let trace_id = output.state.get("trace_id");
                let session_id = output.state.get("session_id");
                if let Some(ref bus) = self.event_bus {
                    match trace_id {
                        Some(tid) if !tid.is_empty() => {
                            bus.emit_with_trace(
                                AgentEventData::GraphCompleted {
                                    run_id: uuid::Uuid::new_v4().to_string(),
                                    success: output.success,
                                    total_steps: output.total_steps,
                                    duration_ms: output.duration_ms,
                                },
                                tid,
                                session_id,
                            )
                            .await;
                        }
                        _ => {
                            bus.emit(AgentEventData::GraphCompleted {
                                run_id: uuid::Uuid::new_v4().to_string(),
                                success: output.success,
                                total_steps: output.total_steps,
                                duration_ms: output.duration_ms,
                            })
                            .await;
                        }
                    }
                }
            }
        }

        Ok(result?)
    }

    fn register_graph(&self, graph: Graph) {
        self.graph_registry.register(graph);
    }

    fn get_graph(&self, name: &str) -> Option<Arc<Graph>> {
        self.graph_registry.get(name)
    }

    fn list_graphs(&self) -> Vec<String> {
        self.graph_registry.list()
    }
}
