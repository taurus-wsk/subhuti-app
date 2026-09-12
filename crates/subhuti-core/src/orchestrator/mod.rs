pub mod actor;
pub mod adaptive;
pub mod planner;
pub mod rule_engine;

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
pub use self::planner::{
    execute_plan, generate_expert_plan, generate_plan, parse_plan, parse_plan_or_ask, AskRequest,
    PlanOrAsk, PlanStep, SkillPlan,
};
use crate::event::{AgentEventData, EventBus};
use crate::memory::Memory;
use crate::runtime::llm::{Role, LLM};
use crate::runtime::session::Session;
use crate::sutra_library::SutraLibraryPort;
use crate::vertical::{AssetLibrary, ProjectMemory, ToolRegistry, WorkflowStore};
pub use adaptive::{
    execute_plan_adaptive, AdaptiveOptions, BoxFuture, LlmToolFallback, StepFallback, ToolExecutor,
};
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
            .map(|b| EventBusRef(b))
            .ok_or_else(|| crate::Error::Runtime("EventBus 未配置".to_string()))
    }
}

pub struct Orchestrator {
    agent_registry: AgentRegistry,
    event_bus: Option<Arc<EventBus>>,
    rule_engine: RuleEngine,
    /// 全局演员池（Actor 竞标制）
    actor_registry: ActorRegistry,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            event_bus: None,
            rule_engine: RuleEngine::with_defaults(),
            actor_registry: ActorRegistry::new(),
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
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
    ///
    /// 内部走写锁，无需 `&mut self`：编排主链路不会被注册/配置动作阻塞。
    pub fn set_analysis_rule(&self, rule: Arc<dyn TaskAnalysisRule>) {
        self.rule_engine.set_analysis_rule(rule);
    }

    /// 运行时替换调度决策规则（Layer 2）
    pub fn set_dispatch_rule(&self, rule: Arc<dyn DispatchRule>) {
        self.rule_engine.set_dispatch_rule(rule);
    }

    /// 运行时替换执行监控规则（Layer 3）
    pub fn set_execution_rule(&self, rule: Arc<dyn ExecutionRule>) {
        self.rule_engine.set_execution_rule(rule);
    }

    /// 注册专家（内部写锁，无需 `&mut self`）
    pub fn register_agent(&self, agent: Arc<dyn ExpertAgent>) {
        self.agent_registry.register(agent);
    }

    /// 注册 Actor（演员）到全局演员池（内部写锁，无需 `&mut self`）
    pub fn register_actor(&self, actor: Arc<dyn actor::Actor>) {
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

        // 用户显式指定了专家：优先直接调度该专家（计划+技能执行），
        // 避免被语义图路由截获后，图的每个节点又对「原始输入」整体重跑整个专家
        //，导致 N 次递归重规划（plan_and_execute 叠加）与重复写文件。
        if let Some(expert_id) = ctx.metadata.get("expert_id") {
            if let Some(actor) = self.actor_registry.get_by_id(expert_id) {
                tracing::debug!("用户指定专家: {} → 直接调度 Actor（跳过图路由）", expert_id);
                let _ = self
                    .emit_event(
                        ctx,
                        AgentEventData::UserMessage {
                            message: input.clone(),
                        },
                    )
                    .await;
                return self.dispatch_via_actor(ctx, state, actor).await;
            }
        }

        // graph_name 元数据：框架已无图路由（Workflow 下沉为专家内部），
        // 这里仅把它当作「按专家 ID / 标签精确指定」的别名，等价于 expert_id。
        // 找不到对应专家则回退到主管编排（单域快速路径 / 多域规划路径）。
        let result = if let Some(graph_name) = ctx.metadata.get("graph_name") {
            if let Some(actor) = self.actor_registry.get_by_id(graph_name) {
                tracing::debug!("graph_name 命中专家 ID: {} → {}", graph_name, actor.name());
                self.dispatch_via_actor(ctx, state, actor).await
            } else if let Some(actor) = self
                .actor_registry
                .list()
                .iter()
                .find(|a| a.tags().iter().any(|t| t.eq_ignore_ascii_case(graph_name)))
            {
                tracing::debug!("graph_name 命中专家标签: {} → {}", graph_name, actor.name());
                self.dispatch_via_actor(ctx, state, actor.clone()).await
            } else {
                tracing::warn!("graph_name 未命中任何专家，回退主管编排: {}", graph_name);
                self.dispatch_without_graph(ctx, state).await
            }
        } else {
            self.dispatch_without_graph(ctx, state).await
        };

        // 将助手回复添加到会话历史
        if result.success {
            ctx.session.add_message(Role::Assistant, &result.output);
        }

        result
    }

    /// 按标签相关性从演员池挑选最匹配的专家
    ///
    /// 评分方式：专家的每个 tag 出现在输入中记 1 分，取总分最高者。
    /// 这里不能硬编码「优先 code / rust」——那会让 Blender、写作这类
    /// 非代码问题也被固定塞给编程专家。
    fn select_actor_by_relevance(&self, input: &str) -> Option<Arc<dyn Actor>> {
        let input_lower = input.to_lowercase();
        let actors = self.actor_registry.list();

        let mut best: Option<(usize, Arc<dyn Actor>)> = None;
        for actor in actors.iter() {
            let score = actor
                .tags()
                .iter()
                .filter(|t| !t.is_empty() && input_lower.contains(&t.to_lowercase()))
                .count();
            if score > 0 && best.as_ref().map(|(s, _)| score > *s).unwrap_or(true) {
                best = Some((score, actor.clone()));
            }
        }

        if let Some((score, actor)) = &best {
            tracing::debug!(
                "按标签相关性匹配到专家: {} (命中 {} 个标签)",
                actor.name(),
                score
            );
            return Some(actor.clone());
        }

        // 没有任何标签命中时，回退到第一个可用专家（保持原有兜底行为）
        actors.first().cloned()
    }

    /// 跳过图匹配，直接交给最相关的专家处理
    ///
    /// 用于调用方已显式表达「不要猜图」的场景（例如显式指定默认图）。
    pub async fn dispatch_direct(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
    ) -> OrchestrationResult {
        let input = ctx.input.clone();
        match self.select_actor_by_relevance(&input) {
            Some(actor) => {
                tracing::debug!("直接调度（跳过图匹配）: {}", actor.name());
                self.dispatch_via_actor(ctx, state, actor).await
            }
            None => {
                tracing::warn!("未匹配到任何可用专家");
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

    /// 按标签命中数从演员池挑选相关专家（降序）。
    ///
    /// 仅收录「输入中命中了至少一个标签」的专家；无任何命中返回空。
    /// 命中数用于区分「单领域（快速路径）」与「多领域（主管规划路径）」。
    fn relevant_actors(&self, input: &str) -> Vec<Arc<dyn Actor>> {
        let input_lower = input.to_lowercase();
        let actors = self.actor_registry.list();
        let mut scored: Vec<(usize, Arc<dyn Actor>)> = actors
            .iter()
            .filter_map(|a| {
                let hits = a
                    .tags()
                    .iter()
                    .filter(|t| !t.is_empty() && input_lower.contains(&t.to_lowercase()))
                    .count();
                if hits > 0 {
                    Some((hits, a.clone()))
                } else {
                    None
                }
            })
            .collect();
        scored.sort_by_key(|(h, _)| std::cmp::Reverse(*h));
        scored.into_iter().map(|(_, a)| a).collect()
    }

    /// 无指定图时的 dispatch 流程：主管编排（Planner/ReAct 主管）。
    ///
    /// 不再做自动图路由（框架已无 Graph，Workflow 下沉为专家内部，由主管 Planner 选专家）。
    /// 两级决策：
    ///   1. 单领域命中 → 直接黑盒调该专家（不额外消耗一次 LLM 规划，保留 M1a 速度）
    ///   2. 多领域命中 → 主管用框架 Planner 把请求拆给多个专家串行执行
    ///   3. 零命中   → 兜底到第一个可用专家（保持原行为）
    ///
    /// 专家内部自带 Planner/ReAct（DomainExpert::plan_and_execute），框架只负责
    /// 「选哪些专家、按什么顺序」，专家如何内部执行对框架是黑盒。
    async fn dispatch_without_graph(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
    ) -> OrchestrationResult {
        let input = &ctx.input.clone();

        tracing::debug!("未指定图，走主管编排（无图路由）: input={}", input);

        let relevant = self.relevant_actors(input);
        match relevant.len() {
            // 清晰单领域：快速路径，直接黑盒调该专家
            1 => {
                let actor = relevant.into_iter().next().unwrap();
                tracing::debug!("单领域命中，快速路径: {}", actor.name());
                self.dispatch_via_actor(ctx, state, actor).await
            }
            // 多领域：主管规划，框架 Planner 拆给多个专家串行
            n if n >= 2 => {
                tracing::debug!("多领域命中 {} 个专家，走主管规划路径", n);
                self.dispatch_with_plan(ctx, state, relevant).await
            }
            // 零命中：兜底
            _ => {
                tracing::debug!("无关键词命中，兜底选择");
                if let Some(actor) = self.select_actor_by_relevance(input) {
                    self.dispatch_via_actor(ctx, state, actor).await
                } else {
                    tracing::warn!("未匹配到任何专家");
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

    /// 主管规划路径（M1b）：框架 Planner 把多领域请求拆给多个专家串行执行。
    ///
    /// 每个计划步骤 = 调一个专家（黑盒）；框架把上一步专家的输出作为下一步专家的输入，
    /// 通过克隆 AgentContext、改写其 `input` 实现专家间的上下文传递。
    /// 主管不感知专家内部如何执行（内部 Planner/ReAct/重试对框架不可见）。
    async fn dispatch_with_plan(
        &self,
        ctx: &mut AgentContext,
        state: &ExpertState,
        roster: Vec<Arc<dyn Actor>>,
    ) -> OrchestrationResult {
        // LLM 来自 ExpertState（无需给 Orchestrator 增加字段）
        let llm = match state.llm_cloned() {
            Some(l) => l,
            None => {
                tracing::warn!("主管规划需要 LLM 但未配置，回退到首个相关专家");
                return self.dispatch_via_actor(ctx, state, roster[0].clone()).await;
            }
        };

        // 把候选专家渲染成 Planner 的「技能清单」，skill_id 即专家 id
        let experts: Vec<SkillInfo> = roster
            .iter()
            .map(|a| SkillInfo {
                id: a.id().to_string(),
                name: a.name().to_string(),
                description: format!("领域标签：{}", a.tags().join("/")),
                parameters: vec!["input".to_string()],
            })
            .collect();

        let plan =
            match generate_expert_plan(&llm, &ctx.input, &experts, "Subhuti 主管（多专家编排）")
                .await
            {
                Ok(PlanOrAsk::Plan(p)) => p,
                // 规划返回提问 → 回退到首个相关专家，避免卡住用户
                Ok(PlanOrAsk::Ask(_)) => {
                    tracing::warn!("主管规划返回提问，回退单专家");
                    return self.dispatch_via_actor(ctx, state, roster[0].clone()).await;
                }
                // 规划失败 → 回退到首个相关专家
                Err(e) => {
                    tracing::warn!("主管规划失败，回退单专家: {:?}", e.to_string());
                    return self.dispatch_via_actor(ctx, state, roster[0].clone()).await;
                }
            };

        if plan.steps.is_empty() {
            return self.dispatch_via_actor(ctx, state, roster[0].clone()).await;
        }

        // 串行执行每个专家（黑盒），上一步输出喂下一步
        let mut expert_chain: Vec<String> = Vec::new();
        let mut prev_output: Option<String> = None;
        let mut last_output = String::new();
        let mut all_ok = true;

        for (idx, step) in plan.steps.iter().enumerate() {
            let actor = match self.actor_registry.get_by_id(&step.skill_id) {
                Some(a) => a,
                None => {
                    tracing::warn!("计划引用了未知专家 id={}，跳过该步", step.skill_id);
                    continue;
                }
            };

            // 克隆一份子上下文：第一步用原始输入，后续步注入上一位专家的输出
            let mut sub = ctx.clone();
            sub.input = if idx == 0 {
                ctx.input.clone()
            } else {
                format!(
                    "{}\n\n## 上一位专家的输出（上下文）\n{}\n",
                    ctx.input,
                    prev_output.clone().unwrap_or_default()
                )
            };

            let res = self.dispatch_via_actor(&mut sub, state, actor).await;
            expert_chain.extend(res.expert_chain);
            if res.success {
                prev_output = Some(res.output.clone());
            } else {
                all_ok = false;
            }
            last_output = res.output;
        }

        OrchestrationResult {
            strategy: "plan:multi-expert".to_string(),
            expert_chain,
            output: last_output,
            tokens: TokenUsage::default(),
            expert_outputs: Vec::new(),
            success: all_ok,
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

        // 发布专家匹配事件（route 阶段）：让 ProgressEventBridge 透传到 SSE，
        // 使前端能渲染「🧭 匹配专家: XXX」的阶段分类
        self.emit_event(
            ctx,
            AgentEventData::AgentMatched {
                agent_id: actor_id.clone(),
                agent_name: actor_name.clone(),
                match_score: 1.0,
                candidates: Vec::new(),
            },
        )
        .await;

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
}

pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<String, Arc<dyn ExpertAgent>>>>,
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册专家（内部写锁，无需 `&mut self`）
    pub fn register(&self, agent: Arc<dyn ExpertAgent>) {
        match self.agents.write() {
            Ok(mut agents) => {
                agents.insert(agent.id().to_string(), agent);
            }
            Err(e) => tracing::error!("AgentRegistry 写锁中毒，专家注册失败: {}", e),
        }
    }

    pub fn find_matching_experts(&self, input: &str) -> Vec<Arc<dyn ExpertAgent>> {
        let input_lower = input.to_lowercase();
        let mut matched: Vec<(Arc<dyn ExpertAgent>, u32)> = Vec::new();

        let agents = self.agents.read().map(|a| a.clone()).unwrap_or_default();
        for agent in agents.values() {
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

    pub fn get_by_id(&self, id: &str) -> Option<Arc<dyn ExpertAgent>> {
        self.agents
            .read()
            .ok()
            .and_then(|agents| agents.get(id).cloned())
    }

    /// 列出所有已注册专家（用于 RuleEngine 调度，返回真实 Agent）
    pub fn list_agents(&self) -> Vec<Arc<dyn ExpertAgent>> {
        self.agents
            .read()
            .map(|agents| agents.values().cloned().collect())
            .unwrap_or_default()
    }

    /// 列出所有已注册专家的只读快照（用于适配器/表现层，不泄漏 trait 对象）
    pub fn list_agent_snapshots(&self) -> Vec<FrameworkExpertInfo> {
        self.agents
            .read()
            .map(|agents| {
                agents
                    .values()
                    .map(|a| FrameworkExpertInfo::from_agent(a.as_ref()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn agent_count(&self) -> usize {
        self.agents.read().map(|a| a.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::memory::{Memory, MemoryItem, SearchResult};

    struct MockActor {
        id: String,
        name: String,
        tags: Vec<String>,
    }

    #[async_trait]
    impl actor::Actor for MockActor {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn tags(&self) -> &[String] {
            &self.tags
        }
        async fn perform(
            &self,
            _ctx: &mut AgentContext,
            _state: &ExpertState,
        ) -> crate::Result<String> {
            Ok(self.name.clone())
        }
    }

    /// 测试用空 Memory 实现（ExpertState 构造需要，但本模块不依赖真实持久化）。
    struct NoopMemory;
    impl Memory for NoopMemory {
        fn write_short_term(&self, _c: &str, _t: Vec<String>) {}
        fn write_long_term(&self, _c: &str, _t: Vec<String>) {}
        fn read(&self, _id: &str) -> Option<MemoryItem> {
            None
        }
        fn delete(&self, _id: &str) {}
        fn search(&self, _q: &str, _l: usize) -> Vec<SearchResult> {
            Vec::new()
        }
        fn get_all(&self) -> Vec<MemoryItem> {
            Vec::new()
        }
        fn clear(&self) {}
    }

    fn orch_with_two_experts() -> Orchestrator {
        let orch = Orchestrator::new();
        orch.register_actor(Arc::new(MockActor {
            id: "rust".into(),
            name: "Rust 编程专家".into(),
            tags: vec!["rust".into(), "code".into(), "programming".into()],
        }));
        orch.register_actor(Arc::new(MockActor {
            id: "blender".into(),
            name: "Blender 动画专家".into(),
            tags: vec!["blender".into(), "3D".into(), "建模".into()],
        }));
        orch
    }

    fn test_state() -> ExpertState {
        ExpertState::builder(Arc::new(NoopMemory)).build()
    }

    #[test]
    fn relevance_picks_blender_expert_for_blender_question() {
        let orch = orch_with_two_experts();
        let picked = orch
            .select_actor_by_relevance("教我怎么用Blender做阵列修改器循环建模")
            .expect("应至少回退到一个专家");
        assert_eq!(picked.name(), "Blender 动画专家");
    }

    #[test]
    fn relevance_picks_rust_expert_for_code_question() {
        let orch = orch_with_two_experts();
        let picked = orch
            .select_actor_by_relevance("用 rust 写一个异步爬虫")
            .expect("应至少回退到一个专家");
        assert_eq!(picked.name(), "Rust 编程专家");
    }

    /// 回归（M1c）：graph_name 元数据作为「专家 ID 精确指定」别名，
    /// 应直接命中对应专家（等价于 expert_id），框架不再走任何图路由。
    #[tokio::test]
    async fn graph_name_routes_to_expert_by_id() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("教我怎么用Blender做阵列修改器", "default");
        ctx.set_metadata("graph_name", "blender");
        let res = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(res.success);
        assert_eq!(res.expert_chain, vec!["Blender 动画专家".to_string()]);
    }

    /// 反向回归（M1c）：graph_name 命中专家标签时同样直接路由。
    #[tokio::test]
    async fn graph_name_routes_to_expert_by_tag() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("用 rust 写点东西", "default");
        ctx.set_metadata("graph_name", "rust");
        let res = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(res.success);
        assert_eq!(res.expert_chain, vec!["Rust 编程专家".to_string()]);
    }

    #[test]
    fn relevant_actors_single_domain_returns_one() {
        let orch = orch_with_two_experts();
        let rel = orch.relevant_actors("教我怎么用Blender做阵列修改器循环建模");
        assert_eq!(rel.len(), 1, "纯 Blender 问题应只命中 1 个专家");
        assert_eq!(rel[0].id(), "blender");
    }

    #[test]
    fn relevant_actors_multi_domain_returns_many() {
        let orch = orch_with_two_experts();
        let rel = orch.relevant_actors("用 Rust 给 Blender 写个导出插件");
        assert!(
            rel.len() >= 2,
            "Rust+Blender 混合问题应命中 ≥2 个专家，实际: {}",
            rel.len()
        );
    }

    #[test]
    fn relevant_actors_no_hit_returns_empty() {
        let orch = Orchestrator::new();
        orch.register_actor(Arc::new(MockActor {
            id: "rust".into(),
            name: "Rust 编程专家".into(),
            tags: vec!["rust".into()],
        }));
        let rel = orch.relevant_actors("今天天气真好");
        assert!(rel.is_empty(), "无关键词命中应返回空");
    }
}
