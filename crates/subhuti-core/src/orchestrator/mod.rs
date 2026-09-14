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
//   - 具体执行下沉在各专家内部（Workflow 已由专家自选执行路径）
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
    execute_plan, generate_expert_plan, generate_plan, is_placeholder_step, parse_plan,
    parse_plan_or_ask, strip_placeholder_steps, AskRequest, PlanExecution, PlanOrAsk, PlanStep,
    SkillPlan,
};
use crate::event::{AgentEventData, EventBus};
use crate::runtime::llm::{Role, LLM};
use crate::runtime::session::Session;
use crate::sutra_library::SutraLibraryPort;
pub use adaptive::{execute_plan_adaptive, AdaptiveOptions, BoxFuture, StepFallback, ToolExecutor};
use rule_engine::{classify_task_type, extract_spo};
pub use rule_engine::{TaskAnalysisRule, TaskProfile};

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
    /// 本请求专属的进度事件通道（per-request，不经过任何全局注册表）。
    ///
    /// 由 `OrchestrationEngine`（应用层）在每次编排前注入；`DomainExpertAdapter::run`
    /// 取出后透传给专家执行上下文。用 `Option` 以便非流式调用（一次性 JSON 编排）可留空。
    #[serde(skip)]
    pub progress: Option<tokio::sync::mpsc::Sender<crate::progress::ProgressEvent>>,
}

impl AgentContext {
    pub fn new(input: &str, ctx_id: &str) -> Self {
        Self {
            input: input.to_string(),
            ctx_id: ctx_id.to_string(),
            session: Session::new(ctx_id),
            metadata: HashMap::new(),
            progress: None,
        }
    }

    /// 使用已有的 Session 创建 AgentContext（用于会话历史持久化）
    pub fn with_session(input: &str, ctx_id: &str, session: Session) -> Self {
        Self {
            input: input.to_string(),
            ctx_id: ctx_id.to_string(),
            session,
            metadata: HashMap::new(),
            progress: None,
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
    event_bus: Option<Arc<EventBus>>,
    sutra_library: Option<Arc<dyn SutraLibraryPort>>,
}

impl ExpertState {
    pub fn builder() -> ExpertStateBuilder {
        ExpertStateBuilder::new()
    }

    pub fn llm(&self) -> Option<&Arc<dyn LLM>> {
        self.llm.as_ref()
    }

    pub fn event_bus(&self) -> Option<&Arc<EventBus>> {
        self.event_bus.as_ref()
    }

    pub fn llm_cloned(&self) -> Option<Arc<dyn LLM>> {
        self.llm.clone()
    }

    pub fn event_bus_cloned(&self) -> Option<Arc<EventBus>> {
        self.event_bus.clone()
    }

    pub fn sutra_library(&self) -> Option<&Arc<dyn SutraLibraryPort>> {
        self.sutra_library.as_ref()
    }

    pub fn sutra_library_cloned(&self) -> Option<Arc<dyn SutraLibraryPort>> {
        self.sutra_library.clone()
    }
}

#[derive(Default)]
pub struct ExpertStateBuilder {
    llm: Option<Arc<dyn LLM>>,
    event_bus: Option<Arc<EventBus>>,
    sutra_library: Option<Arc<dyn SutraLibraryPort>>,
}

impl ExpertStateBuilder {
    pub fn new() -> Self {
        Self::default()
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

    pub fn sutra_library(mut self, lib: Arc<dyn SutraLibraryPort>) -> Self {
        self.sutra_library = Some(lib);
        self
    }

    pub fn build(self) -> ExpertState {
        ExpertState {
            llm: self.llm,
            event_bus: self.event_bus,
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
    /// 任务分析规则插件位（None = 用内置 tags 打分派生画像）
    analysis_rule: RwLock<Option<Arc<dyn TaskAnalysisRule>>>,
    /// 全局演员池（Actor 竞标制）
    actor_registry: ActorRegistry,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// 「延续信号」标记词——判断一句话是否表现为**对上一轮的承接/追问**。
///
/// 只收两类：**承接/追加动词**与**指代词**。二者都是「缺少独立语义、必须依赖上文
/// 才成立」的表达，这正是会话粘性应有的适用面。
const CONTINUATION_MARKERS: &[&str] = &[
    // 承接 / 追加
    "再",
    "继续",
    "接着",
    "然后",
    "补充",
    "还有",
    "另外",
    "此外",
    "追加",
    "顺便",
    "修改",
    "改成",
    "改一下",
    "调整",
    "优化",
    "换成",
    "替换",
    "加上",
    "去掉",
    "删除",
    // 指代
    "它",
    "这个",
    "那个",
    "上述",
    "上面",
    "刚才",
    "之前",
    "前面",
    "这样",
    "那样",
    "这部分",
    "这块",
    "这行",
];

/// 判断输入是否表现为「对上一轮的承接/追问」。
///
/// **为什么需要它**：零命中其实分两类，行为必须相反——
///
/// | 形态 | 例子 | 正确行为 |
/// |------|------|---------|
/// | (a) 承接上一轮，自身无独立语义 | 「再加个骨骼」「那它和 Iterator 的关系呢」 | **延续**本会话专家 |
/// | (b) 完整、独立、与本服务领域无关 | 「今天天气怎么样」「帮我写首诗赞美大海」 | 走**领域边界提示** |
///
/// 无门控时 (b) 也会被粘性吞进上一个专家，等于把「零命中不兜底」这条产品规则架空
/// （只要会话有历史，任何领域外问题都能被任意专家接管）。故只有 (a) 才放行粘性。
pub(crate) fn looks_like_continuation(input: &str) -> bool {
    let s = input.trim();
    if s.is_empty() {
        return false;
    }
    CONTINUATION_MARKERS.iter().any(|m| s.contains(m))
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            agent_registry: AgentRegistry::new(),
            event_bus: None,
            analysis_rule: RwLock::new(None),
            actor_registry: ActorRegistry::new(),
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// 运行时注入自定义任务分析规则（插件位）
    ///
    /// 内部走写锁，无需 `&mut self`：编排主链路不会被注册/配置动作阻塞。
    /// 未注入时 `analyze_task` 用内置「专家 tags × 输入匹配」派生画像。
    pub fn set_analysis_rule(&self, rule: Arc<dyn TaskAnalysisRule>) {
        if let Ok(mut guard) = self.analysis_rule.write() {
            *guard = Some(rule);
        }
    }

    /// 按标签相关性返回匹配的专家（与 dispatch 主链路同源，按命中数降序）
    ///
    /// `match_expert` 查询端点也走这里——保证「预览 = 实际路由」，杜绝两套真相。
    pub fn match_experts(&self, input: &str) -> Vec<Arc<dyn Actor>> {
        self.relevant_actors(input)
    }

    /// 任务画像分析（零 LLM）
    ///
    /// 优先使用应用层注入的 [`TaskAnalysisRule`]；未注入时用内置派生：
    /// `domain_tags` 来自专家 tags 打分（与 dispatch 同源），
    /// `task_type` / 主谓宾为输入侧关键词粗分类。
    pub fn analyze_task(&self, input: &str) -> TaskProfile {
        if let Some(rule) = self.analysis_rule.read().ok().and_then(|g| g.clone()) {
            match rule.analyze(input) {
                Ok(p) => return p,
                Err(e) => tracing::warn!("自定义任务分析规则失败，回退内置派生: {}", e),
            }
        }
        let input_lower = input.to_lowercase();
        let mut domain_tags: Vec<String> = self
            .relevant_actors(input)
            .iter()
            .flat_map(|a| {
                a.tags()
                    .iter()
                    .filter(|t| !t.is_empty() && input_lower.contains(&t.to_lowercase()))
                    .cloned()
            })
            .collect();
        domain_tags.sort();
        domain_tags.dedup();
        let (subject, predicate, object) = extract_spo(input);
        TaskProfile {
            domain_tags,
            task_type: classify_task_type(&input_lower),
            subject,
            predicate,
            object,
        }
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

        // 无任何标签命中：**不再回退到第一个专家**。
        // 产品定位是「领域深度的多软件工作流」，泛化/领域外问题不应被任意领域专家接管，
        // 故返回 None，由调用方给出「未匹配到领域」的显式结果。
        None
    }

    /// 已注册专家的名称列表（用于「未匹配」提示，明确本服务的领域边界）
    fn available_expert_names(&self) -> String {
        let names: Vec<String> = self
            .actor_registry
            .list()
            .iter()
            .map(|a| a.name().to_string())
            .collect();
        if names.is_empty() {
            "(无已注册专家)".to_string()
        } else {
            names.join(" / ")
        }
    }

    /// 「未匹配到领域」的**统一出口**：零命中且不适用会话粘性时的唯一结果构造点。
    ///
    /// - `hint`：可选的上下文引导。存在上一轮专家时用它给出「若为同一任务的延续，
    ///   请补充说明」的提示——**只提示、不越权代答**，既保住了会话上下文的价值，
    ///   又不违反「零命中不兜底」的产品定位。
    ///
    /// 之所以抽成方法：`dispatch_direct` 与 `dispatch_without_graph` 的零命中分支
    /// 此前各写了一份完全相同的长文案，属复制粘贴，容易改一处漏一处。
    fn unmatched_domain_result(&self, hint: Option<String>) -> OrchestrationResult {
        let mut output = format!(
            "未匹配到相关领域专家。本服务仅处理以下领域的任务：{}。请描述与这些领域相关的具体需求。",
            self.available_expert_names()
        );
        if let Some(h) = hint {
            output.push_str(&h);
        }
        OrchestrationResult {
            strategy: "fallback".to_string(),
            expert_chain: Vec::new(),
            output,
            tokens: TokenUsage::default(),
            expert_outputs: Vec::new(),
            success: false,
        }
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
                self.unmatched_domain_result(None)
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
    /// 三级决策（外加一层**受门控的**会话粘性兜底）：
    ///   1. 单领域命中 → 直接黑盒调该专家（不额外消耗一次 LLM 规划，保留 M1a 速度）
    ///   2. 多领域命中 → 主管用框架 Planner 把请求拆给多个专家串行执行
    ///   3. 零命中   → **仅当输入表现为对上一轮的承接**（`looks_like_continuation`）时，
    ///      延续本会话最近路由到的专家；否则返回「未匹配到领域专家」并列出本服务领域
    ///      边界（不做泛化兜底）
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
            // 零命中：**仅在输入表现为「承接上一轮」时**才启用会话粘性，延续本会话
            // 最近成功路由到的专家（如「再加个骨骼」「那它和 Iterator 的关系呢」）。
            // 显式 tag 命中的单域/多域分支始终优先。
            //
            // ⚠️ 门控是必需的：零命中还包含「今天天气怎么样」这类**完整且与领域无关**
            // 的独立需求，若也走粘性，本会话有历史时任何领域外问题都会被上一个专家吞掉，
            // 「零命中不兜底」的产品规则就被架空了。此类输入一律走领域边界提示。
            _ => {
                let sticky = ctx
                    .session
                    .get_metadata("last_expert_id")
                    .and_then(|v| v.as_str())
                    .and_then(|id| self.actor_registry.get_by_id(id))
                    .map(|a| (a.id().to_string(), a.name().to_string()));

                if let Some((id, name)) = sticky {
                    if looks_like_continuation(input) {
                        if let Some(actor) = self.actor_registry.get_by_id(&id) {
                            tracing::debug!(
                                "零命中且输入为承接表达 → 会话粘性延续专家 {}",
                                actor.name()
                            );
                            return self.dispatch_via_actor(ctx, state, actor).await;
                        }
                    }
                    tracing::info!(
                        "零命中且输入非承接表达 → 不启用粘性（上一轮专家={}），返回领域边界",
                        name
                    );
                    return self.unmatched_domain_result(Some(format!(
                        "\n（提示：本会话上一轮由「{}」处理。若这是同一任务的延续，请补充说明。）",
                        name
                    )));
                }

                tracing::warn!("无关键词命中，未匹配到领域专家");
                self.unmatched_domain_result(None)
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
        // 已执行但失败的专家名：整体成功后仍要把失败暴露给调用方
        // （成败口径与专家内部一致：**任一步骤失败 → 整体失败**）
        let mut failed_experts: Vec<String> = Vec::new();

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

            let res = self
                .dispatch_via_actor(&mut sub, state, actor.clone())
                .await;
            expert_chain.extend(res.expert_chain);
            if res.success {
                prev_output = Some(res.output.clone());
            } else {
                all_ok = false;
                failed_experts.push(actor.name().to_string());
            }
            last_output = res.output;
        }

        // 整体失败时，末位专家的输出可能是"成功的"（前面某位专家失败），
        // 单看文案会与 success=false 自相矛盾 → 显式列出失败专家，让原因可见可判别。
        if !all_ok && !failed_experts.is_empty() {
            last_output = format!(
                "{}\n\n---\n⚠️ 以下专家执行失败（共 {} 个步骤）：{}\n（任一步骤失败即整体判为失败）",
                last_output,
                failed_experts.len(),
                failed_experts.join("、")
            );
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

        // 记录本会话「最近路由到的专家」，供零命中时做**会话粘性**回落。
        // 场景：本会话先做了 Blender 任务，用户接着追问「再加个骨骼」——
        // 后者不含任何领域 tag，若只按逐条消息匹配会被判成领域外；
        // 有了粘性则延续同一专家，保住多软件工作流的会话连续性。
        // 存进 `ctx.session`（框架级会话状态）→ 随 save_session 持久化。
        //
        // ⚠️ 必须写在**路由确定处**（而非执行成功分支）：粘性要保的是「对话连续性」，
        // 即「用户刚才在跟哪位专家说话」，这个事实在路由命中的那一刻就已成定，
        // **与本次执行成败无关**。写在成功分支会有一个真实反例（实测暴露）：
        //   轮 1「用 rust 写个函数」因缺 workspace_folder 前置条件失败 → 未记录；
        //   轮 2「再补充一下错误处理」被判领域外 → 会话直接断裂。
        // 从用户视角看，他明明刚跟 Rust 专家说过话，追问理应延续。
        ctx.session.set_metadata(
            "last_expert_id",
            serde_json::Value::String(actor_id.clone()),
        );

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

    /// 永远执行失败的专家：用于验证「粘性记录不依赖执行成败」。
    struct FailingActor {
        id: String,
        name: String,
        tags: Vec<String>,
    }

    #[async_trait]
    impl actor::Actor for FailingActor {
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
            Err(crate::Error::Expert("模拟前置条件缺失".into()))
        }
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
        ExpertState::builder().build()
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

    /// 零命中且会话无历史 → 返回领域边界提示（不做泛化兜底）。
    #[tokio::test]
    async fn no_hit_without_session_sticky_returns_boundary() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("今天天气怎么样", "default");
        let res = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(!res.success);
        assert!(res.output.contains("未匹配到相关领域专家"));
    }

    /// 会话粘性：本会话已成功路由到某专家后，**无 tag 的追问**应延续同一专家，
    /// 而不是被判成领域外。修复「领域内起头 → 无 tag 追问断链」的体验问题。
    #[tokio::test]
    async fn session_sticky_routes_tagless_followup_to_last_expert() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("教我怎么用Blender做阵列修改器", "sess-sticky");

        // 第 1 轮：含 tag → 路由到 Blender，并记下粘性专家
        let first = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(first.success);
        assert_eq!(first.expert_chain, vec!["Blender 动画专家".to_string()]);
        assert_eq!(
            ctx.session
                .get_metadata("last_expert_id")
                .and_then(|v| v.as_str()),
            Some("blender"),
            "首次成功路由后应把 last_expert_id 写入框架级会话状态"
        );

        // 第 2 轮：不含任何 tag 的追问 → 应由粘性接住，延续 Blender
        ctx.input = "它叫什么名字？".to_string();
        let second = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(second.success, "无 tag 追问应被会话粘性接住，不再判领域外");
        assert_eq!(second.expert_chain, vec!["Blender 动画专家".to_string()]);
    }

    /// 粘性**门控**：完整且与领域无关的独立需求，不得被粘性吞进上一个专家。
    ///
    /// 回归用例：曾经「今天天气怎么样」在本会话有过 Blender 历史时会被粘性路由到
    /// Blender 专家并真的作答，等于把「零命中不兜底」这条产品规则架空。
    #[tokio::test]
    async fn sticky_gate_rejects_unrelated_topic_switch() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("教我怎么用Blender做阵列修改器", "sess-gate");
        let first = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(first.success, "第 1 轮应正常路由");

        for input in ["今天天气怎么样", "帮我写首诗赞美大海", "推荐几部电影"]
        {
            ctx.input = input.to_string();
            let res = orch.dispatch(&mut ctx, &test_state()).await;
            assert!(!res.success, "{input} 属于领域外独立需求，不应被粘性接管");
            assert!(
                res.output.contains("未匹配到相关领域专家"),
                "{input} 应返回领域边界提示，实际: {}",
                res.output
            );
            // 边界提示保留上下文引导（只提示、不代答）
            assert!(
                res.output.contains("Blender 动画专家"),
                "{input} 的边界提示应带上「上一轮专家」的引导: {}",
                res.output
            );
            assert!(
                res.expert_chain.is_empty(),
                "{input} 不应产生专家链（未真正调专家）"
            );
        }
    }

    /// 粘性记录**不依赖执行成败**：轮 1 路由命中了专家但执行失败，
    /// 轮 2 的承接追问仍应延续同一专家，而不是被判成领域外。
    ///
    /// 回归用例（实测暴露）：此前 `last_expert_id` 只在**成功分支**写入，
    /// 于是「轮 1 因缺 workspace_folder 失败 → 轮 2『再补充一下错误处理』」
    /// 会被判领域外，会话直接断裂——用户视角是「我刚跟 Rust 专家说过话」。
    #[tokio::test]
    async fn sticky_is_recorded_even_when_expert_execution_fails() {
        let orch = Orchestrator::new();
        orch.register_actor(Arc::new(FailingActor {
            id: "rust".into(),
            name: "Rust 编程专家".into(),
            tags: vec!["rust".into(), "code".into()],
        }));

        let mut ctx = AgentContext::new("用 rust 写一个函数", "sess-fail-sticky");
        let first = orch.dispatch(&mut ctx, &test_state()).await;
        assert!(!first.success, "该轮执行应失败（模拟缺前置条件）");
        assert_eq!(
            ctx.session
                .get_metadata("last_expert_id")
                .and_then(|v| v.as_str()),
            Some("rust"),
            "只要路由命中就应记录粘性专家，与执行成败无关"
        );

        ctx.input = "再补充一下错误处理".to_string();
        let second = orch.dispatch(&mut ctx, &test_state()).await;
        assert_eq!(
            second.expert_chain,
            vec!["Rust 编程专家".to_string()],
            "承接追问应延续 Rust，而不是被粘性门控拦成领域外"
        );
        assert!(
            !second.output.contains("未匹配到相关领域专家"),
            "不应返回领域边界提示: {}",
            second.output
        );
    }

    /// 粘性门控的纯函数判定：承接表达放行、独立需求拒绝。
    #[test]
    fn continuation_gate_classification() {
        for s in [
            "再加个骨骼",
            "继续",
            "那它和 Iterator 的关系呢",
            "补充一下错误处理",
            "这个怎么改",
            "然后呢",
        ] {
            assert!(super::looks_like_continuation(s), "{s:?} 应判定为承接表达");
        }
        for s in [
            "今天天气怎么样",
            "帮我写首诗赞美大海",
            "推荐几部电影",
            "量子纠缠是什么",
            "介绍一下图论",
            "",
            "   ",
        ] {
            assert!(
                !super::looks_like_continuation(s),
                "{s:?} 不应判定为承接表达"
            );
        }
    }

    /// 显式 tag 始终优先于粘性：会话粘在 Blender 时，显式问 Rust 仍应切到 Rust。
    #[tokio::test]
    async fn explicit_tag_beats_session_sticky() {
        let orch = orch_with_two_experts();
        let mut ctx = AgentContext::new("教我怎么用Blender做阵列修改器", "sess-switch");
        let _ = orch.dispatch(&mut ctx, &test_state()).await;

        ctx.input = "改用 rust 写一个 CLI".to_string();
        let res = orch.dispatch(&mut ctx, &test_state()).await;
        assert_eq!(res.expert_chain, vec!["Rust 编程专家".to_string()]);
    }
}
