pub mod actor;
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

pub use self::actor::{Actor, ActorRegistry, ExpertAgentActorAdapter};
use self::strategies::{SemanticCandidate, SemanticRouter};
use crate::event::{AgentEventData, EventBus};
use crate::graph::{Graph, GraphOutput, GraphState};
use crate::memory::Memory;
use crate::runtime::llm::{Role, LLM};
use crate::runtime::session::Session;
use crate::sutra_library::SutraLibraryPort;
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

    /// 使用已有的 Session 创建 AgentContext（用于会话历史持久化）
    pub fn with_session(input: &str, ctx_id: &str, session: Session) -> Self {
        Self {
            input: input.to_string(),
            ctx_id: ctx_id.to_string(),
            session,
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
    sutra_library: Option<Arc<dyn SutraLibraryPort>>,
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

    pub fn sutra_library(&self) -> Option<&Arc<dyn SutraLibraryPort>> {
        self.sutra_library.as_ref()
    }

    pub fn sutra_library_cloned(&self) -> Option<Arc<dyn SutraLibraryPort>> {
        self.sutra_library.clone()
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
    sutra_library: Option<Arc<dyn SutraLibraryPort>>,
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
            sutra_library: None,
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

    pub fn sutra_library(mut self, lib: Arc<dyn SutraLibraryPort>) -> Self {
        self.sutra_library = Some(lib);
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
            sutra_library: self.sutra_library,
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
    /// 全局演员池（Actor 竞标制）
    actor_registry: ActorRegistry,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            graph_registry: GraphRegistry::new(),
            event_bus: None,
            semantic_router: SemanticRouter::new_disabled(),
            rule_engine: RuleEngine::with_defaults(),
            actor_registry: ActorRegistry::new(),
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

    /// 注册 Actor（演员）到全局演员池
    pub fn register_actor(&mut self, actor: Arc<dyn actor::Actor>) {
        self.actor_registry.register(actor);
    }

    /// 获取 Actor 注册表引用（用于图调度器）
    pub fn actor_registry(&self) -> &ActorRegistry {
        &self.actor_registry
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

    pub fn set_default_graph(&self, name: &str) {
        self.graph_registry.set_default(name);
    }

    pub async fn dispatch(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
    ) -> OrchestrationResult {
        let input = &ctx.input.clone();

        // 将用户消息添加到会话历史
        ctx.session.add_message(Role::User, input);

        // 只保留图匹配 + 事件发布
        // Actor 竞标制：图节点发布任务要求，Actor 池自评竞标
        self.emit_event(
            ctx,
            AgentEventData::UserMessage {
                message: input.clone(),
            },
        )
        .await;

        // 优先使用指定图（从 ctx.metadata 中获取）
        let result = if let Some(graph_name) = ctx.metadata.get("graph_name") {
            if let Some(graph) = self.graph_registry.get(graph_name) {
                tracing::debug!("指定图: {}", graph_name);
                self.dispatch_via_graph(ctx, state, &graph).await
            } else {
                tracing::warn!("指定的图不存在: {}，尝试按专家 ID/标签匹配", graph_name);

                // 图名不存在 → 尝试按专家 ID/标签匹配（graph_name 可以是专家 ID 或标签）
                // 1. 尝试按专家 ID 精确匹配
                if let Some(actor) = self.actor_registry.get_by_id(graph_name) {
                    tracing::debug!("按专家 ID 匹配: {} → {}", graph_name, actor.name());
                    self.dispatch_via_actor(ctx, state, actor).await
                }
                // 2. 尝试按专家标签匹配（标签包含 graph_name 的专家）
                else {
                    let actors = self.actor_registry.list();
                    let mut matched_actor = None;
                    for actor in actors.iter() {
                        if actor
                            .tags()
                            .iter()
                            .any(|t| t.eq_ignore_ascii_case(graph_name))
                        {
                            tracing::debug!("按专家标签匹配: {} → {}", graph_name, actor.name());
                            matched_actor = Some(actor.clone());
                            break;
                        }
                    }
                    // 3. 如果有 expert_id metadata，直接按 expert_id 匹配
                    if matched_actor.is_none() {
                        if let Some(expert_id) = ctx.metadata.get("expert_id") {
                            if let Some(actor) = self.actor_registry.get_by_id(expert_id) {
                                tracing::debug!(
                                    "按 expert_id 匹配: {} → {}",
                                    expert_id,
                                    actor.name()
                                );
                                matched_actor = Some(actor);
                            }
                        }
                    }
                    if let Some(actor) = matched_actor {
                        self.dispatch_via_actor(ctx, state, actor).await
                    } else {
                        // 都找不到则继续走图匹配流程
                        self.dispatch_without_graph(ctx, state).await
                    }
                }
            }
        } else {
            // 未指定图名，走关键词匹配
            self.dispatch_without_graph(ctx, state).await
        };

        // 将助手回复添加到会话历史
        if result.success {
            ctx.session.add_message(Role::Assistant, &result.output);
        }

        result
    }

    /// 无指定图时的 dispatch 流程：先尝试语义路由，再尝试关键词匹配
    async fn dispatch_without_graph(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
    ) -> OrchestrationResult {
        let input = &ctx.input.clone();

        // 图匹配：优先语义路由，其次关键词匹配
        let matched_graph = self.try_semantic_graph_routing(input).await;
        let matched_graph = match matched_graph {
            Some(graph) => Some(graph),
            None => self.graph_registry.find_matching_graph(input).await,
        };

        match matched_graph {
            Some(graph) => {
                tracing::debug!("图匹配成功: {}", graph.name());
                self.dispatch_via_graph(ctx, state, &graph).await
            }
            None => {
                // 无匹配图时，尝试按标签匹配专家（Actor）
                tracing::debug!("无匹配图，尝试按专家标签匹配: input={}", input);
                let actors = self.actor_registry.list();

                // 尝试找到第一个能处理该输入的专家
                // 优先匹配标签包含 "code" 或 "rust" 的专家
                let mut matched_actor = None;
                for actor in actors.iter() {
                    let tags = actor.tags();
                    if tags.iter().any(|t| {
                        let tl = t.to_lowercase();
                        tl.contains("code") || tl.contains("rust") || tl.contains("programming")
                    }) {
                        tracing::debug!("按标签匹配到专家: {}", actor.name());
                        matched_actor = Some(actor.clone());
                        break;
                    }
                }

                // 如果没有匹配到特定专家，尝试使用第一个可用专家
                if matched_actor.is_none() {
                    matched_actor = actors.first().cloned();
                }

                if let Some(actor) = matched_actor {
                    tracing::debug!("使用专家: {}", actor.name());
                    self.dispatch_via_actor(ctx, state, actor).await
                } else {
                    // 没有可用专家
                    tracing::warn!("未匹配到任何图或专家");
                    OrchestrationResult {
                        strategy: "fallback".to_string(),
                        expert_chain: Vec::new(),
                        output: "未匹配到合适的专家，请检查专家注册或输入内容".to_string(),
                        tokens: TokenUsage::default(),
                        expert_outputs: Vec::new(),
                        success: false,
                    }
                }
            }
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
        if let Some(ws) = ctx.metadata.get("workspace_folder") {
            graph_state.set("workspace_folder", ws.clone());
        }

        match self
            .execute_graph(graph, graph_state, &self.actor_registry, _state)
            .await
        {
            Ok(output) => {
                tracing::debug!(
                    "📊 dispatch_via_graph 完成: graph={}, success={}, output_len={}, error={:?}",
                    graph.name(),
                    output.success,
                    output.output.len(),
                    output.error,
                );
                let final_output = if output.success || output.error.is_none() {
                    output.output
                } else {
                    output.error.clone().unwrap_or(output.output)
                };
                OrchestrationResult {
                    strategy: format!("graph:{}", graph.name()),
                    expert_chain: output.execution_path,
                    output: final_output,
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: output.success,
                }
            }
            Err(e) => {
                tracing::warn!(
                    "📊 dispatch_via_graph 失败: graph={}, error={}",
                    graph.name(),
                    e
                );
                OrchestrationResult {
                    strategy: format!("graph:{}", graph.name()),
                    expert_chain: Vec::new(),
                    output: e.to_string(),
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: false,
                }
            }
        }
    }

    /// 直接通过专家（Actor）执行
    ///
    /// 当 graph_name 不匹配任何图时，按专家 ID/标签找到 Actor 并直接执行。
    async fn dispatch_via_actor(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
        actor: Arc<dyn Actor>,
    ) -> OrchestrationResult {
        let actor_name = actor.name().to_string();
        let actor_id = actor.id().to_string();

        self.emit_event(
            ctx,
            AgentEventData::ChainSelected {
                chain_name: actor_name.clone(),
                strategy: "expert".to_string(),
            },
        )
        .await;

        // 发布专家开始执行事件
        self.emit_event(
            ctx,
            AgentEventData::AgentStarted {
                agent_id: actor_id.clone(),
                input: ctx.input.clone(),
            },
        )
        .await;

        match actor.perform(ctx, state).await {
            Ok(output) => {
                self.emit_event(
                    ctx,
                    AgentEventData::AgentCompleted {
                        agent_id: actor_id.clone(),
                        output: output.clone(),
                        duration_ms: 0,
                    },
                )
                .await;
                tracing::debug!(
                    "dispatch_via_actor 完成: actor={}, output_len={}",
                    actor_name,
                    output.len()
                );
                OrchestrationResult {
                    strategy: format!("expert:{}", actor_id),
                    expert_chain: vec![actor_name],
                    output,
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: true,
                }
            }
            Err(e) => {
                self.emit_event(
                    ctx,
                    AgentEventData::AgentFailed {
                        agent_id: actor_id.clone(),
                        error: e.to_string(),
                        duration_ms: 0,
                    },
                )
                .await;
                tracing::warn!("dispatch_via_actor 失败: actor={}, error={}", actor_name, e);
                OrchestrationResult {
                    strategy: format!("expert:{}", actor_id),
                    expert_chain: vec![actor_name],
                    output: e.to_string(),
                    tokens: TokenUsage::default(),
                    expert_outputs: Vec::new(),
                    success: false,
                }
            }
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
    default_graph_name: RwLock<Option<String>>,
}

impl GraphRegistry {
    pub fn new() -> Self {
        Self {
            graphs: RwLock::new(HashMap::new()),
            default_graph_name: RwLock::new(None),
        }
    }

    pub fn register(&self, graph: Graph) {
        let name = graph.name().to_string();
        let arc_graph = Arc::new(graph);
        self.graphs.write().unwrap().insert(name, arc_graph);
    }

    /// 设置默认图（当无图匹配时使用）
    pub fn set_default(&self, name: &str) {
        *self.default_graph_name.write().unwrap() = Some(name.to_string());
    }

    /// 获取默认图
    pub fn get_default(&self) -> Option<Arc<Graph>> {
        let name = self.default_graph_name.read().unwrap().clone()?;
        self.graphs.read().unwrap().get(&name).cloned()
    }

    pub fn get(&self, name: &str) -> Option<Arc<Graph>> {
        self.graphs.read().unwrap().get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.graphs.read().unwrap().keys().cloned().collect()
    }

    pub async fn find_matching_graph(&self, input: &str) -> Option<Arc<Graph>> {
        let graphs = self.graphs.read().unwrap();
        let available: Vec<String> = graphs.keys().cloned().collect();
        tracing::debug!(
            "find_matching_graph: 可用的图={:?}, input={}",
            available,
            input
        );
        if graphs.is_empty() {
            return None;
        }

        // 编程相关关键词（命中这些关键词才路由到编程图）
        let code_keywords: &[&str] = &[
            "代码",
            "编程",
            "开发",
            "写代码",
            "code",
            "programming",
            "rust",
            "项目",
            "创建",
            "编译",
            "运行",
            "构建",
            "测试",
            "实现",
            "函数",
            "bug",
            "错误",
            "安装",
            "配置",
            "依赖",
            "库",
            "框架",
            "接口",
            "api",
            "模块",
            "struct",
            "fn",
            "cargo",
            "src",
            "main.rs",
            "lib.rs",
            "package",
            "toml",
            "项目",
            "工程",
            "应用",
            "程序",
            "脚本",
            "命令行",
            "cli",
        ];
        let input_lower = input.to_lowercase();
        let is_code_query = code_keywords.iter().any(|kw| input_lower.contains(kw));

        // 只有一个图时：如果是编程相关查询则返回，否则返回 None 走普通对话
        if graphs.len() == 1 {
            if is_code_query {
                return graphs.values().next().cloned();
            }
            tracing::debug!("单图模式但输入非编程相关，跳过图匹配: {}", input);
            return None;
        }

        // 排除默认图（它只作为兜底，不参与关键词匹配）
        let default_name = self.default_graph_name.read().unwrap().clone();
        let non_default: Vec<&Arc<Graph>> = graphs
            .values()
            .filter(|g| Some(g.name().to_string()) != default_name)
            .collect();

        if non_default.is_empty() {
            return None;
        }

        for graph in &non_default {
            if input_lower.contains(&graph.name().to_lowercase()) {
                return Some((*graph).clone());
            }
        }

        let review_keywords = ["评审", "审查", "审核", "review", "audit"];
        let edit_keywords = ["修改", "编辑", "添加", "增加", "重构", "改造", "改", "edit"];

        for graph in &non_default {
            let name = graph.name().to_lowercase();
            if name.contains("code")
                || name.contains("dev")
                || name.contains("programming")
                || name.contains("rust")
            {
                for kw in code_keywords {
                    if input_lower.contains(kw) {
                        return Some((*graph).clone());
                    }
                }
            }
            if name.contains("review") || name.contains("audit") {
                for kw in &review_keywords {
                    if input_lower.contains(kw) {
                        return Some((*graph).clone());
                    }
                }
            }
            if name.contains("edit") {
                for kw in &edit_keywords {
                    if input_lower.contains(kw) {
                        return Some((*graph).clone());
                    }
                }
            }
        }

        // 避免无关键词匹配时错误兜底到第一个图
        None
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
    async fn execute_graph(
        &self,
        graph: &Graph,
        state: GraphState,
        actor_registry: &ActorRegistry,
        expert_state: &ExpertState,
    ) -> crate::Result<GraphOutput>;
    fn register_graph(&self, graph: Graph);
    fn get_graph(&self, name: &str) -> Option<Arc<Graph>>;
    fn list_graphs(&self) -> Vec<String>;
}

#[async_trait]
impl GraphOrchestrator for Orchestrator {
    async fn route_by_graph(&self, input: &str) -> Option<Arc<Graph>> {
        self.graph_registry.find_matching_graph(input).await
    }

    async fn execute_graph(
        &self,
        graph: &Graph,
        state: GraphState,
        actor_registry: &ActorRegistry,
        expert_state: &ExpertState,
    ) -> crate::Result<GraphOutput> {
        // GraphStarted/GraphCompleted 由 scheduler 内部负责 emit（带 trace_id）
        let result = graph
            .run_event_driven(state, actor_registry, expert_state)
            .await;

        if let Ok(ref output) = result {
            tracing::debug!(
                "📊 execute_graph 完成: graph={}, success={}, execution_path={:?}",
                graph.name(),
                output.success,
                output.execution_path,
            );
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
