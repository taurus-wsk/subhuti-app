//! # Subhuti 引擎调度
//!
//! 引擎调度层：持有 `Arc<dyn Trait>` 依赖，提供 `dispatch()`、`register_actor()` 等编排方法。
//! 不依赖具体 infra 实现，所有依赖由调用方通过 trait 对象注入。

use std::sync::Arc;

use crate::event::EventBus;
use crate::orchestrator::{
    Actor, AgentContext, ExpertAgent, ExpertState, FrameworkExpertInfo, OrchestrationResult,
    Orchestrator, TaskAnalysisRule,
};
use crate::runtime::LLM;
use crate::sutra_library::SutraLibraryPort;

/// Subhuti 引擎调度器
///
/// 所有字段使用 `Arc<dyn Trait>` 避免具体 infra 依赖。
/// 构造时由调用方（组合根）注入具体实现。
pub struct Subhuti {
    event_bus: Arc<EventBus>,
    /// 编排器（**无锁**）
    ///
    /// 历史问题：这里曾是 `tokio::sync::Mutex<Orchestrator>`，且 `dispatch()` 全程持锁，
    /// 导致所有 HTTP 请求被完全串行化——第二个请求必须等第一个（可能几十秒）跑完。
    ///
    /// 现在 `Orchestrator` 内部状态（专家表 / 演员池 / 图注册表 / 规则引擎）全部改为
    /// 细粒度 `RwLock`，读多写少，注册与配置走写锁、编排执行走读锁，
    /// 因此这里不再需要任何外层锁，多个请求可真正并发执行。
    orchestrator: Orchestrator,
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
    /// 只需注入 `EventBus`；LLM / 藏经阁由 `set_llm()` / `set_sutra_library()` 后续注入。
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        let mut orchestrator = Orchestrator::new();
        orchestrator = orchestrator.with_event_bus(event_bus.clone());
        Self {
            event_bus,
            orchestrator,
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

    /// 获取编排器引用（无锁共享，可并发读）
    pub fn actor_registry(&self) -> &Orchestrator {
        &self.orchestrator
    }

    /// 注册 ExpertAgent（通过 Orchestrator）
    pub async fn register_orchestrator_expert(&self, agent: Arc<dyn ExpertAgent>) {
        self.orchestrator.register_agent(agent);
    }

    /// 注册 Actor 到全局演员池
    pub async fn register_actor(&self, actor: Arc<dyn Actor>) {
        self.orchestrator.register_actor(actor);
    }

    /// 使用默认上下文执行编排
    pub async fn dispatch(&self, input: &str) -> OrchestrationResult {
        let mut ctx = AgentContext::new(input, "default");
        let state = self.build_expert_state();
        self.orchestrator.dispatch(&mut ctx, &state).await
    }

    /// 使用自定义上下文执行编排
    pub async fn dispatch_with_context(&self, mut ctx: AgentContext) -> OrchestrationResult {
        let state = self.build_expert_state();

        // 无锁：Orchestrator 内部为细粒度 RwLock，多个请求可并发 dispatch
        let result = self.orchestrator.dispatch(&mut ctx, &state).await;

        // 保存 Session（会话历史持久化）
        self.save_session(ctx.session.clone()).await;

        result
    }

    /// 构建共享 ExpertState（供图节点等使用）
    pub fn build_expert_state(&self) -> ExpertState {
        let mut builder = ExpertState::builder().event_bus(self.event_bus.clone());
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
        self.orchestrator.set_analysis_rule(rule);
    }

    // ─── 任务分析 & 专家匹配 ─────────────────────────────────────

    /// 任务分析（与 dispatch 同源的标签打分派生，零 LLM）
    pub async fn analyze_task(&self, message: &str) -> serde_json::Value {
        let profile = self.orchestrator.analyze_task(message);
        tracing::info!(
            "[Subhuti·analyze_task] domain_tags={:?}, task_type={}",
            profile.domain_tags,
            profile.task_type
        );
        serde_json::to_value(profile)
            .unwrap_or_else(|_| serde_json::json!({ "error": "TaskProfile 序列化失败" }))
    }

    /// 专家匹配（与 dispatch 主链路同源的标签打分，预览 = 实际路由）
    pub async fn match_expert(&self, input: &str) -> Vec<FrameworkExpertInfo> {
        let matched_ids: Vec<String> = self
            .orchestrator
            .match_experts(input)
            .iter()
            .map(|a| a.id().to_string())
            .collect();
        let snapshots = self.orchestrator.list_expert_snapshots();
        let matched: Vec<FrameworkExpertInfo> = matched_ids
            .iter()
            .filter_map(|id| snapshots.iter().find(|s| &s.id == id).cloned())
            .collect();
        tracing::info!(
            "[Subhuti·match_expert] input={:?}, matched={:?}",
            input,
            matched.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
        );
        matched
    }

    /// 获取专家快照列表
    pub async fn list_orchestrator_experts(&self) -> Vec<FrameworkExpertInfo> {
        self.orchestrator.list_expert_snapshots()
    }

    /// 通过技能 ID 查找所属专家
    pub async fn find_agent_by_skill(
        &self,
        skill_id: &str,
    ) -> Option<(String, Arc<dyn ExpertAgent>)> {
        let orchestrator = &self.orchestrator;
        for agent in orchestrator.list_experts() {
            if agent.skills().iter().any(|s| s.id == skill_id) {
                return Some((agent.id().to_string(), agent));
            }
        }
        None
    }
}

// ─── 编译期并发契约断言 ─────────────────────────────────────────
//
// `Subhuti` 被 HTTP 服务以 `Arc` 共享给所有请求任务，因此必须 `Send + Sync`。
//
// 背景：`orchestrator` 字段原本是 `tokio::sync::Mutex<Orchestrator>`，
// 且 `dispatch()` 全程持锁 —— 两个 HTTP 请求会严格排队（第二个要等第一个跑完，
// Agent 场景下常常是几十秒）。现在改为无锁 + 内部细粒度 RwLock。
//
// 此断言用于防回归：一旦将来有人引入非 Sync 字段、或重新把编排器套进
// 需要 `&mut` 的容器，这里会直接编译失败，而不是悄悄退化成串行。
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Subhuti>();
};
