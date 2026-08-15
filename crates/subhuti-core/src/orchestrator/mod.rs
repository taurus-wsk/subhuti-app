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
        if let Some(graph_name) = ctx.metadata.get("graph_name") {
            if let Some(graph) = self.graph_registry.get(graph_name) {
                tracing::debug!("指定图: {}", graph_name);
                return self.dispatch_via_graph(ctx, state, &graph).await;
            }
            tracing::warn!("指定的图不存在: {}", graph_name);
            // 不存在则继续走匹配流程
        }

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
                // 无匹配时尝试使用默认图
                if let Some(default_graph) = self.graph_registry.get_default() {
                    tracing::debug!("使用默认图: {}", default_graph.name());
                    self.dispatch_via_graph(ctx, state, &default_graph).await
                } else {
                    tracing::warn!("未匹配到任何图，且无默认图");
                    OrchestrationResult {
                        strategy: "fallback".to_string(),
                        expert_chain: Vec::new(),
                        output: "未匹配到合适的图，请检查图注册或输入内容".to_string(),
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
        if graphs.len() == 1 {
            return graphs.values().next().cloned();
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

        let input_lower = input.to_lowercase();
        for graph in &non_default {
            if input_lower.contains(&graph.name().to_lowercase()) {
                return Some((*graph).clone());
            }
        }

        let code_keywords = [
            "代码",
            "编程",
            "开发",
            "写代码",
            "code",
            "programming",
            "rust",
            "项目",
        ];
        let review_keywords = ["评审", "审查", "审核", "review", "audit"];
        let edit_keywords = ["修改", "编辑", "添加", "增加", "重构", "改造", "改", "edit"];

        for graph in &non_default {
            let name = graph.name().to_lowercase();
            if name.contains("code")
                || name.contains("dev")
                || name.contains("programming")
                || name.contains("rust")
            {
                for kw in &code_keywords {
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

        // 非默认图中取第一个
        non_default.first().map(|g| (*g).clone())
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
