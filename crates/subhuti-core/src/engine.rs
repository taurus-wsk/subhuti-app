//! # Subhuti 引擎调度
//!
//! 引擎调度层：持有 `Arc<dyn Trait>` 依赖，提供 `dispatch()`、`register_actor()` 等编排方法。
//! 不依赖具体 infra 实现，所有依赖由调用方通过 trait 对象注入。

use std::sync::Arc;

use crate::event::EventBus;
use crate::graph::{Graph, GraphBuilder, NodeResult};
use crate::memory::Memory;
use crate::orchestrator::{
    Actor, AgentContext, DispatchRule, ExecutionRule, ExpertAgent, ExpertState,
    FrameworkExpertInfo, OrchestrationResult, Orchestrator, TaskAnalysisRule,
};
use crate::runtime::LLM;
use crate::sutra_library::SutraLibraryPort;
use crate::vertical::{AssetLibrary, ProjectMemory, ToolRegistry, WorkflowStore};

/// Subhuti 引擎调度器
///
/// 所有字段使用 `Arc<dyn Trait>` 避免具体 infra 依赖。
/// 构造时由调用方（组合根）注入具体实现。
pub struct Subhuti {
    memory: Arc<dyn Memory>,
    event_bus: Arc<EventBus>,
    orchestrator: tokio::sync::Mutex<Orchestrator>,
    asset_library: Arc<dyn AssetLibrary>,
    project_memory: Arc<dyn ProjectMemory>,
    tool_registry: Arc<dyn ToolRegistry>,
    workflow_store: Arc<dyn WorkflowStore>,
    llm: Option<Arc<dyn LLM>>,
    sutra_library: std::sync::RwLock<Option<Arc<dyn SutraLibraryPort>>>,
    /// Session 存储：根据 session_id 持久化会话历史
    sessions: std::sync::Arc<
        tokio::sync::RwLock<std::collections::HashMap<String, crate::runtime::session::Session>>,
    >,
}

impl Subhuti {
    /// 创建引擎实例
    ///
    /// 调用方需提供所有 `Arc<dyn Trait>` 依赖。
    /// LLM 可后续通过 `set_llm()` 注入。
    pub fn new(
        memory: Arc<dyn Memory>,
        event_bus: Arc<EventBus>,
        asset_library: Arc<dyn AssetLibrary>,
        project_memory: Arc<dyn ProjectMemory>,
        tool_registry: Arc<dyn ToolRegistry>,
        workflow_store: Arc<dyn WorkflowStore>,
    ) -> Self {
        let mut orchestrator = Orchestrator::new();
        orchestrator = orchestrator.with_event_bus(event_bus.clone());
        Self {
            memory,
            event_bus,
            orchestrator: tokio::sync::Mutex::new(orchestrator),
            asset_library,
            project_memory,
            tool_registry,
            workflow_store,
            llm: None,
            sutra_library: std::sync::RwLock::new(None),
            sessions: std::sync::Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// 获取或创建 Session
    pub async fn get_or_create_session(
        &self,
        session_id: &str,
    ) -> crate::runtime::session::Session {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get(session_id) {
            session.clone()
        } else {
            let session = crate::runtime::session::Session::new(session_id);
            sessions.insert(session_id.to_string(), session.clone());
            session
        }
    }

    /// 保存 Session（将修改后的 Session 存回存储）
    pub async fn save_session(&self, session: crate::runtime::session::Session) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id().to_string(), session);
    }

    /// 注入 LLM 实例
    pub fn set_llm(&mut self, llm: Arc<dyn LLM>) {
        self.llm = Some(llm);
    }

    /// 注入藏经阁记忆引擎
    pub fn set_sutra_library(&self, lib: Arc<dyn SutraLibraryPort>) {
        if let Ok(mut guard) = self.sutra_library.write() {
            *guard = Some(lib);
        }
    }

    /// 获取当前 LLM 实例
    pub fn current_llm(&self) -> Option<Arc<dyn LLM>> {
        self.llm.clone()
    }

    pub fn event_bus(&self) -> &Arc<EventBus> {
        &self.event_bus
    }

    pub fn actor_registry(&self) -> &tokio::sync::Mutex<Orchestrator> {
        &self.orchestrator
    }

    /// 注册 ExpertAgent（通过 Orchestrator）
    pub async fn register_orchestrator_expert(&self, agent: Arc<dyn ExpertAgent>) {
        self.orchestrator.lock().await.register_agent(agent);
    }

    /// 注册 Actor 到全局演员池
    pub async fn register_actor(&self, actor: Arc<dyn Actor>) {
        self.orchestrator.lock().await.register_actor(actor);
    }

    /// 注册图编排
    pub async fn register_graph(&self, mut graph: Graph) {
        graph.set_event_bus(self.event_bus.clone());
        self.orchestrator.lock().await.register_graph(graph);
    }

    /// 使用默认上下文执行编排
    pub async fn dispatch(&self, input: &str) -> OrchestrationResult {
        let mut ctx = AgentContext::new(input, "default");
        let state = self.build_expert_state();
        self.orchestrator
            .lock()
            .await
            .dispatch(&mut ctx, &state)
            .await
    }

    /// 使用自定义上下文执行编排
    pub async fn dispatch_with_context(&self, mut ctx: AgentContext) -> OrchestrationResult {
        let state = self.build_expert_state();

        tracing::info!("[dispatch_with_context] 开始获取 orchestrator 锁");
        let mut orchestrator = self.orchestrator.lock().await;
        tracing::info!("[dispatch_with_context] 已获取 orchestrator 锁，开始执行 dispatch");

        // 克隆必要的数据，以便在释放锁后继续执行
        let input = ctx.input.clone();
        let session = ctx.session.clone();
        let metadata = ctx.metadata.clone();

        // 在锁内执行 dispatch（因为需要访问 orchestrator 内部状态）
        let result = orchestrator.dispatch(&mut ctx, &state).await;

        tracing::info!("[dispatch_with_context] dispatch 执行完成，准备释放锁");
        drop(orchestrator); // 显式释放锁

        // 保存 Session（会话历史持久化）- 锁外执行
        self.save_session(ctx.session.clone()).await;

        tracing::info!("[dispatch_with_context] 完成");
        result
    }

    /// 构建共享 ExpertState（供图节点等使用）
    pub fn build_expert_state(&self) -> ExpertState {
        let mut builder = ExpertState::builder(self.memory.clone())
            .event_bus(self.event_bus.clone())
            .asset_library(self.asset_library.clone())
            .project_memory(self.project_memory.clone())
            .tool_registry(self.tool_registry.clone())
            .workflow_store(self.workflow_store.clone());
        if let Some(ref llm) = self.llm {
            builder = builder.llm(llm.clone());
        }
        if let Some(ref lib) = *self.sutra_library.read().unwrap() {
            builder = builder.sutra_library(lib.clone());
        }
        builder.build()
    }

    // ─── 规则设置 ─────────────────────────────────────────────────

    pub async fn set_analysis_rule(&self, rule: Arc<dyn TaskAnalysisRule>) {
        self.orchestrator.lock().await.set_analysis_rule(rule);
    }

    pub async fn set_dispatch_rule(&self, rule: Arc<dyn DispatchRule>) {
        self.orchestrator.lock().await.set_dispatch_rule(rule);
    }

    pub async fn set_execution_rule(&self, rule: Arc<dyn ExecutionRule>) {
        self.orchestrator.lock().await.set_execution_rule(rule);
    }

    // ─── 默认图 ──────────────────────────────────────────────────

    /// 同步插件专家并注册默认图
    pub async fn sync_experts_to_orchestrator(&self) {
        self.register_default_graph().await;
    }

    /// 注册默认图（单节点，无标签，兜底用）
    async fn register_default_graph(&self) {
        let bus = self.event_bus.clone();
        let mut graph = GraphBuilder::new()
            .name("default")
            .node("default_node", |state| async move {
                let input = state.get("input").unwrap_or_default();
                NodeResult::ok(input)
            })
            .entry("default_node")
            .build()
            .expect("默认图构建失败");
        graph.set_event_bus(bus);
        self.orchestrator.lock().await.register_graph(graph);
        self.orchestrator.lock().await.set_default_graph("default");
        tracing::debug!("已注册默认图");
    }

    // ─── 任务分析 & 专家匹配 ─────────────────────────────────────

    pub fn active_expert_info(&self) -> Option<FrameworkExpertInfo> {
        None
    }

    /// 任务分析
    pub async fn analyze_task(&self, message: &str) -> serde_json::Value {
        let orchestrator = self.orchestrator.lock().await;
        match orchestrator.rule_engine().analyze_task(message) {
            Ok(profile) => {
                tracing::info!(
                    "[Subhuti·analyze_task] domain_tags={:?}, task_type={}",
                    profile.domain_tags,
                    profile.task_type
                );
                serde_json::to_value(profile)
                    .unwrap_or_else(|_| serde_json::json!({ "error": "TaskProfile 序列化失败" }))
            }
            Err(e) => {
                tracing::warn!("[Subhuti·analyze_task] 分析失败: {}", e);
                serde_json::json!({ "error": e.to_string(), "input": message })
            }
        }
    }

    /// 专家匹配
    pub async fn match_expert(&self, input: &str) -> Vec<FrameworkExpertInfo> {
        let orchestrator = self.orchestrator.lock().await;
        let profile = match orchestrator.rule_engine().analyze_task(input) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[Subhuti·match_expert] analyze_task 失败: {}", e);
                return Vec::new();
            }
        };
        let agents = orchestrator.list_experts();
        let plan = match orchestrator
            .rule_engine()
            .decide_strategy(&profile, &agents)
        {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[Subhuti·match_expert] decide_strategy 失败: {}", e);
                return Vec::new();
            }
        };
        let snapshots = orchestrator.list_expert_snapshots();
        let matched: Vec<FrameworkExpertInfo> = plan
            .steps
            .iter()
            .filter_map(|step| snapshots.iter().find(|s| s.id == step.agent_id).cloned())
            .collect();
        tracing::info!(
            "[Subhuti·match_expert] input={:?}, domain_tags={:?}, strategy={:?}, matched={:?}",
            input,
            profile.domain_tags,
            plan.strategy,
            matched.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
        );
        matched
    }

    /// 获取专家快照列表
    pub async fn list_orchestrator_experts(&self) -> Vec<FrameworkExpertInfo> {
        self.orchestrator.lock().await.list_expert_snapshots()
    }

    /// 获取真实 Agent 列表
    pub async fn get_orchestrator_agents(&self) -> Vec<Arc<dyn ExpertAgent>> {
        self.orchestrator.lock().await.list_experts()
    }

    /// 通过技能 ID 查找所属专家
    pub async fn find_agent_by_skill(
        &self,
        skill_id: &str,
    ) -> Option<(String, Arc<dyn ExpertAgent>)> {
        let orchestrator = self.orchestrator.lock().await;
        for agent in orchestrator.list_experts() {
            if agent.skills().iter().any(|s| s.id == skill_id) {
                return Some((agent.id().to_string(), agent));
            }
        }
        None
    }
}
