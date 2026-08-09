//! # 应用层入站端口（Driving Port）
//!
//! 入站适配层通过这些窄端口调用应用层：
//! - `ChatPort`：聊天/编排调度（orchestrate + orchestrate_stream）
//! - `ExpertQueryPort`：专家查询（list_experts + active_expert + match_expert + analyze_task）
//! - `SkillPort`：技能操作（skill_list + execute_skill + execute_skill_stream）
//!
//! ## 设计原则
//!
//! - 领域 DTO 和出站端口在 `domain::dto` 和 `domain::ports` 中定义
//! - 应用层只定义入站端口（用例）；初始化逻辑由组合根直接使用具体类型
//! - 观察者端口在 `application::observer` 模块
//! - 所有端口都是 trait 对象安全（dyn compatible）
//! - 流式端口返回协议中立的 `StreamEvent`，适配器负责协议格式

use tokio::sync::mpsc;

use crate::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};

// ─── 流式事件（协议中立）─────────────────────────────────────────

/// 流式事件（协议中立）
///
/// 应用层产出业务语义事件，适配器负责转 HTTP/SSE/WS 协议格式 + 流式呈现策略（分块、节奏）。
/// 应用层只关心"发生了什么"，不关心"怎么传输"。
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 流开始
    Start,
    /// 数据块（应用层发完整内容，适配器决定分块策略）
    Chunk { content: String },
    /// 流结束（output 为完整输出，meta 为结果元数据，适配器透传）
    Done {
        output: String,
        meta: serde_json::Value,
    },
    /// 错误
    Error { error: String },
}

// ─── 入站 Port（Driving Port，入站适配层调用应用层）──────────────────

/// 聊天/编排调度端口（入站窄端口 1）
///
/// 负责执行调度和流式调度。
/// chat_handler / orchestrate_handler / chat_stream_handler 依赖此端口。
pub trait ChatPort: Sync + Send + 'static {
    /// 执行调度（核心）
    fn orchestrate(
        &self,
        request: OrchestrateRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>>;

    /// 流式执行调度（协议中立流事件）
    ///
    /// 返回 StreamEvent 流，适配器负责转 SSE/WS 协议格式 + 分块策略。
    fn orchestrate_stream(&self, request: OrchestrateRequest) -> mpsc::Receiver<StreamEvent>;
}

/// 专家查询端口（入站窄端口 2）
///
/// 负责专家列表、激活专家、专家匹配、任务分析。
/// orchestrate_experts_handler / orchestrate_analyze_handler / orchestrate_match_handler /
/// experts_list_handler / experts_active_handler / experts_match_handler 依赖此端口。
pub trait ExpertQueryPort: Sync + Send + 'static {
    /// 获取专家列表（真实从框架 Orchestrator 注册中心快照）
    fn list_experts(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>>;

    /// 获取当前激活的专家（返回 None = 未激活）
    fn active_expert(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExpertInfo>> + Send>>;

    /// 匹配专家（返回空 Vec = 未匹配到，语义合法）
    fn match_expert(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>>;

    /// 分析任务（框架层可扩展，空对象表示"未分析"）
    fn analyze_task(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send>>;
}

/// 技能操作端口（入站窄端口 3）
///
/// 负责技能列表、执行技能、流式执行技能。
/// skill_list_handler / skill_execute_handler / skill_stream_handler 依赖此端口。
pub trait SkillPort: Sync + Send + 'static {
    /// 获取所有技能（从专家聚合）
    fn skill_list(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<SkillInfo>> + Send>>;

    /// 执行技能（通过专家）
    ///
    /// - `trace_id`: 追踪 ID（由 TraceAppService 装饰器生成并传入；入站适配器可传空字符串）
    /// - `session_id`: 会话 ID（同上）
    fn execute_skill(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillResponse> + Send>>;

    /// 流式执行技能（协议中立流事件）
    ///
    /// - `trace_id`: 追踪 ID（由 TraceAppService 装饰器生成并传入）
    /// - `session_id`: 会话 ID（同上）
    fn execute_skill_stream(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
    ) -> mpsc::Receiver<StreamEvent>;
}
