//! # HTTP 适配器层
//!
//! 六边形架构的入站适配层适配器：将 HTTP 协议请求转换为业务用例调用，
//! 将业务响应转换为 HTTP 响应。
//!
//! 设计原则：
//! - 适配器持有业务端口（UseCase），是边界层的固有设计
//! - 所有 axum 提取器、Request、Response 只存在适配器内部
//! - 协议转换、错误处理统一封装在适配器层；trace/session 由应用层 `TraceAppService` 装饰器自动记录
//! - 业务层完全不认识 axum
//!
//! ## 方案 C：inventory 自动注册
//!
//! 每个路由通过 `inventory::submit!` 自注册路径、方法、trace 配置，
//! server.rs 无需手动维护路由列表。新增路由只需在此文件添加 handler 函数 +
//! `inventory::submit!` 块即可。

use std::sync::Arc;

use async_stream::stream;
use axum::{
    extract::{Json, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json as AxumJson,
};
use tokio::sync::mpsc;

use crate::adapter::inbound::http::route_adapter::RouteEntry;
use crate::adapter::inbound::http::routes::AppState;
use crate::application::observer::{record_fn_log, LogLevel};
use crate::application::{
    ChatPort, ExpertQueryPort, SessionObserverPort, StreamEvent, TraceObserverPort,
};
use crate::domain::dto::OrchestrateRequest as PortRequest;

// ─── 通用响应类型 ────────────────────────────────────────────────

/// 统一成功响应
///
/// 所有 API 成功响应均使用此结构，确保格式一致：
/// `{ "success": true, "data": <T> }`
#[derive(Debug, serde::Serialize)]
pub struct ApiSuccess<T: serde::Serialize> {
    pub success: bool,
    pub data: T,
}

/// 统一错误响应
///
/// 所有 API 错误响应均使用此结构，确保格式一致：
/// `{ "success": false, "error": "...", "code": 500 }`
#[derive(Debug, serde::Serialize)]
pub struct ApiError {
    pub success: bool,
    pub error: String,
    pub code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ─── IntoResponse 实现 ───────────────────────────────────────────

impl<T: serde::Serialize> IntoResponse for ApiSuccess<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, AxumJson(self)).into_response()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            AxumJson(self),
        )
            .into_response()
    }
}

// ─── 便捷构造方法 ──────────────────────────────────────────────────

impl ApiSuccess<serde_json::Value> {
    pub fn ok(data: serde_json::Value) -> Self {
        Self {
            success: true,
            data,
        }
    }
}

impl ApiError {
    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: msg.into(),
            code: 500,
            details: None,
        }
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: msg.into(),
            code: 404,
            details: None,
        }
    }

    #[allow(dead_code)]
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: msg.into(),
            code: 400,
            details: None,
        }
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.details = Some(detail);
        self
    }
}

// ─── HTTP 请求 DTO ──────────────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct OrchestrateRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub chain: Option<String>,
    /// 指定要使用的图名称（为空时自动匹配）
    pub graph: Option<String>,
    /// 指定要使用的专家 ID（优先级高于 graph，直接路由到该专家）
    pub expert_id: Option<String>,
    /// 项目工作目录路径（前端聊天设置传入，透传给专家）
    pub workspace_folder: Option<String>,
    /// 自定义系统提示词（前端聊天设置传入，覆盖专家默认 system prompt）
    pub system_prompt: Option<String>,
}

// ─── 工具函数 ──────────────────────────────────────────────────

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ─── 流式事件转 SSE（适配器职责：协议格式 + 分块策略）────────────

/// 把协议中立的 StreamEvent 流转为 SSE Event 流
///
/// 职责：分块大小、sleep 节奏、SSE 事件 JSON 格式 —— 全部在适配器层。
/// 应用层只发语义事件（Start/Thought/Plan/Step/Chunk/Done/Error），不关心传输细节。
///
/// `session_id` 会回显进每个事件负载（跨模块串联标准）：前端不再靠请求体手动关联
/// 会话与事件，而是直接从流内读取服务端采用并一路下传的 `session_id`，避免脱节。
fn stream_to_sse(
    receiver: mpsc::Receiver<StreamEvent>,
    session_id: String,
) -> impl futures::Stream<Item = Result<Event, axum::BoxError>> {
    stream! {
        let mut rx = receiver;
        // 给每个 SSE 事件统一标注 session_id（协议中立的关联手段）
        let decorate = |json: String| -> String {
            match serde_json::from_str::<serde_json::Value>(&json) {
                Ok(mut v) => {
                    if let Some(o) = v.as_object_mut() {
                        o.insert("session_id".into(), session_id.clone().into());
                    }
                    v.to_string()
                }
                Err(_) => json, // 非 JSON 载荷原样透传
            }
        };
        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::Start => {
                    yield Ok(Event::default().data(decorate(r#"{"type":"start"}"#.into())));
                }
                StreamEvent::Thought { message } => {
                    let json = serde_json::json!({
                        "type": "thought",
                        "message": message,
                    }).to_string();
                    yield Ok(Event::default().data(decorate(json)));
                }
                StreamEvent::Plan { message } => {
                    let json = serde_json::json!({
                        "type": "plan",
                        "message": message,
                    }).to_string();
                    yield Ok(Event::default().data(decorate(json)));
                }
                StreamEvent::Step { message, expert, todo_state } => {
                    let mut payload = serde_json::json!({
                        "type": "step",
                        "message": message,
                        "expert": expert,
                    });
                    if let Some(ts) = todo_state {
                        payload.as_object_mut()
                            .map(|o| o.insert("todo_state".into(), ts.into()));
                    }
                    yield Ok(Event::default().data(decorate(payload.to_string())));
                }
                StreamEvent::Ask { ask_id, question, options } => {
                    let json = serde_json::json!({
                        "type": "ask",
                        "ask_id": ask_id,
                        "question": question,
                        "options": options,
                    }).to_string();
                    yield Ok(Event::default().data(decorate(json)));
                }
                StreamEvent::Chunk { content } => {
                    // 真流式：content 已是模型产出的**真实增量**（域层逐 delta 下发），
                    // 这里原样透传，不再做「64 字符切块 + 50ms 人为延时」的假打字机。
                    // 非流式兜底路径下 content 为整块 output，会一次性到达（属预期）。
                    let json = serde_json::json!({
                        "type": "data",
                        "content": content,
                        "done": false,
                    }).to_string();
                    yield Ok(Event::default().data(decorate(json)));
                }
                StreamEvent::Done { output, meta } => {
                    let mut payload = serde_json::Map::new();
                    payload.insert("type".into(), "done".into());
                    payload.insert("content".into(), output.into());
                    if let serde_json::Value::Object(m) = meta {
                        for (k, v) in m {
                            payload.insert(k, v);
                        }
                    }
                    yield Ok(Event::default().data(decorate(
                        serde_json::Value::Object(payload).to_string(),
                    )));
                }
                StreamEvent::Error { error } => {
                    let json = serde_json::json!({"type": "error", "error": error}).to_string();
                    yield Ok(Event::default().data(decorate(json)));
                }
            }
        }
    }
}

// ─── 编排统一入口（按 Accept 内容协商响应模式）───────────────────

/// 是否要求 SSE 流式响应（`Accept` 内容协商）
fn wants_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

/// 一次性 JSON 编排：等待整条编排链路结束后返回结构化结果
async fn orchestrate_json(state: AppState, req: OrchestrateRequest) -> Response {
    let user_id = req
        .user_id
        .clone()
        .unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.clone().unwrap_or_else(uuid_v4);

    record_fn_log(
        None,
        "",
        LogLevel::Info,
        format!(
            "收到编排请求 (user={}, session={}, message_len={}, chain={:?})",
            user_id,
            session_id,
            req.message.len(),
            req.chain
        ),
        None,
    );

    let response = state
        .chat_port
        .orchestrate(PortRequest {
            message: req.message.clone(),
            user_id: Some(user_id.clone()),
            session_id: Some(session_id.clone()),
            chain: req.chain.clone(),
            graph: req.graph.clone(),
            expert_id: req.expert_id.clone(),
            trace_id: None,
            workspace_folder: req.workspace_folder.clone(),
            system_prompt: req.system_prompt.clone(),
        })
        .await;

    if response.success {
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            format!(
                "编排完成 (chain={:?}, experts={}, trace={})",
                response.chain,
                response.expert_chain.len(),
                response.trace_id
            ),
            None,
        );
        ApiSuccess::ok(serde_json::json!({
            "output": response.output,
            "trace_id": response.trace_id,
            "session_id": response.session_id,
            "chain": response.chain,
            "expert_chain": response.expert_chain,
            "expert_outputs": response.expert_outputs,
            "duration_ms": response.duration_ms,
        }))
        .into_response()
    } else {
        let error_msg = response.error.clone().unwrap_or_default();
        record_fn_log(
            None,
            "",
            LogLevel::Error,
            format!("编排错误 (error={})", error_msg),
            None,
        );
        ApiError::internal(error_msg)
            .with_detail(serde_json::json!({
                "session_id": session_id,
                "trace_id": response.trace_id,
            }))
            .into_response()
    }
}

/// 流式编排：把协议中立的 StreamEvent 流转为 SSE 事件流
fn orchestrate_sse(state: AppState, req: OrchestrateRequest) -> Response {
    let user_id = req
        .user_id
        .clone()
        .unwrap_or_else(|| "anonymous".to_string());
    let session_id = req.session_id.clone().unwrap_or_else(uuid_v4);

    record_fn_log(
        None,
        "",
        LogLevel::Info,
        format!(
            "编排流式请求 (user={}, session={}, message={}, chain={:?})",
            user_id, session_id, req.message, req.chain
        ),
        None,
    );

    let receiver = state.chat_port.orchestrate_stream(PortRequest {
        message: req.message.clone(),
        user_id: Some(user_id.clone()),
        session_id: Some(session_id.clone()),
        chain: req.chain.clone(),
        graph: req.graph.clone(),
        expert_id: req.expert_id.clone(),
        trace_id: None,
        workspace_folder: req.workspace_folder.clone(),
        system_prompt: req.system_prompt.clone(),
    });

    Sse::new(stream_to_sse(receiver, session_id)).into_response()
}

/// POST /subhuti/api/v1/orchestrate（编排统一入口）
///
/// 同一个业务动作（编排一条消息），由客户端用 `Accept` 头选择响应模式：
/// - `Accept: text/event-stream` → SSE 流式（进度事件 + 逐 delta 的回答）
/// - 其他 / 未带 `Accept`        → 一次性 JSON（等整条链路结束后返回）
///
/// 之所以保留两种模式而非只留流式：MCP 的 `tools/call` 与脚本/CI 需要的都是
/// **一次完整的结构化结果**，SSE 不适用；且 MCP 本身不经 HTTP，直调 `chat_port`。
async fn orchestrate_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<OrchestrateRequest>,
) -> Response {
    if wants_event_stream(&headers) {
        orchestrate_sse(state, req)
    } else {
        orchestrate_json(state, req).await
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/orchestrate",
        method: "POST",
        trace_enabled: true,  // 核心业务路由标记（debug 分类用）
        register: |r| r.route("/subhuti/api/v1/orchestrate", post(orchestrate_handler)),
    }
}

// ─── Orchestrate Analyze 路由 ───────────────────────────────────

/// POST /subhuti/api/v1/orchestrate/analyze
async fn orchestrate_analyze_handler(
    State(state): State<AppState>,
    Json(req): Json<OrchestrateRequest>,
) -> impl IntoResponse {
    let profile = state.expert_query_port.analyze_task(&req.message).await;

    ApiSuccess::ok(serde_json::json!({
        "profile": profile,
        "suggested_strategy": "SimpleDispatch",
    }))
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/orchestrate/analyze",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/orchestrate/analyze", post(orchestrate_analyze_handler)),
    }
}

// ─── Orchestrate Match 路由 ─────────────────────────────────────

/// POST /subhuti/api/v1/orchestrate/match
async fn orchestrate_match_handler(
    State(state): State<AppState>,
    Json(req): Json<OrchestrateRequest>,
) -> impl IntoResponse {
    let experts = state.expert_query_port.match_expert(&req.message).await;

    ApiSuccess::ok(serde_json::json!({
        "matches": experts,
        "total": experts.len(),
    }))
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/orchestrate/match",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/orchestrate/match", post(orchestrate_match_handler)),
    }
}

// ─── Orchestrate Experts 路由 ───────────────────────────────────

/// GET /subhuti/api/v1/orchestrate/experts
async fn orchestrate_experts_handler(State(state): State<AppState>) -> impl IntoResponse {
    let experts = state.expert_query_port.list_experts().await;

    ApiSuccess::ok(serde_json::json!({
        "experts": experts,
        "total": experts.len(),
    }))
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/orchestrate/experts",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/orchestrate/experts", get(orchestrate_experts_handler)),
    }
}

// ─── Ask Resolve 路由 ───────────────────────────────────────────

/// POST /subhuti/api/v1/ask-resolve（主动提问的答复投递）
///
/// 前端在单选卡片上点击选项后调用，把 `ask_id` + `answer` 投递给正在
/// 等待该提问答复的专家执行协程，唤醒后带着答复继续规划/执行。
#[derive(Debug, serde::Deserialize)]
pub struct AskResolveRequest {
    /// 要答复的提问 ID（来自 Ask SSE 事件）
    pub ask_id: String,
    /// 用户选择的选项文本
    pub answer: String,
}

async fn ask_resolve_handler(Json(req): Json<AskResolveRequest>) -> impl IntoResponse {
    let resolved = crate::domain::pending_ask::resolve(&req.ask_id, req.answer.clone());
    if resolved {
        ApiSuccess::ok(serde_json::json!({
            "status": "resolved",
            "ask_id": req.ask_id,
        }))
        .into_response()
    } else {
        ApiError::not_found(format!("未找到挂起的提问: {}", req.ask_id)).into_response()
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/ask-resolve",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/ask-resolve", post(ask_resolve_handler)),
    }
}

// ─── 适配器工厂 ──────────────────────────────────────────────────

/// HTTP 依赖注入工厂
///
/// 适配器本身无状态（关联函数），仅 AppState 需要注入端口。
/// trace/session 记录由应用层 TraceAppService 装饰器自动处理，
/// observer 仅用于 traces/sessions 查询路由读取历史记录。
pub struct HttpAdapterFactory {
    chat_port: Arc<dyn ChatPort>,
    expert_query_port: Arc<dyn ExpertQueryPort>,
    trace_observer: Arc<dyn TraceObserverPort>,
    session_observer: Arc<dyn SessionObserverPort>,
    pg_storage: Option<Arc<subhuti_infra::sutra_library::storage::PgStorage>>,
}

impl HttpAdapterFactory {
    pub fn new(
        chat_port: Arc<dyn ChatPort>,
        expert_query_port: Arc<dyn ExpertQueryPort>,
        trace_observer: Arc<dyn TraceObserverPort>,
        session_observer: Arc<dyn SessionObserverPort>,
        pg_storage: Option<Arc<subhuti_infra::sutra_library::storage::PgStorage>>,
    ) -> Self {
        Self {
            chat_port,
            expert_query_port,
            trace_observer,
            session_observer,
            pg_storage,
        }
    }

    /// 创建 AppState（依赖注入容器，供 handler 通过 State 取端口）
    pub fn create_app_state(&self) -> AppState {
        AppState {
            chat_port: self.chat_port.clone(),
            expert_query_port: self.expert_query_port.clone(),
            trace_observer: self.trace_observer.clone(),
            session_observer: self.session_observer.clone(),
            pg_storage: self.pg_storage.clone(),
        }
    }

    /// 获取 trace observer（供 traces 查询路由读取历史记录）
    pub fn trace_observer(&self) -> Arc<dyn TraceObserverPort> {
        self.trace_observer.clone()
    }

    /// 获取 session observer（供 sessions 查询路由读取历史记录）
    pub fn session_observer(&self) -> Arc<dyn SessionObserverPort> {
        self.session_observer.clone()
    }
}
