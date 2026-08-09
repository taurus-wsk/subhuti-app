//! # 观察者端口与 DTO
//!
//! 横切关注点端口：供入站适配层中间件使用，实现由出站适配层提供。
//! trace/session 属于应用运维概念，不放入领域层。
//!
//! ## 端口
//! - `TraceObserverPort`：追踪观察者（记录执行链路）
//! - `SessionObserverPort`：会话观察者（记录会话信息）
//!
//! ## DTO
//! - `TraceHandle`：追踪句柄（含状态机与日志副作用）
//! - `TraceStatus`：追踪状态
//! - `SessionRecordParams`：会话记录参数

/// 追踪状态（DTO）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStatus {
    InProgress,
    Success,
    Failed,
}

/// 追踪句柄（DTO）
///
/// 封装追踪操作，不泄漏框架类型。
pub struct TraceHandle {
    pub trace_id: String,
    user_id: String,
    session_id: String,
    message: String,
    start_time: std::time::Instant,
    output: Option<String>,
    error: Option<String>,
    duration_ms: Option<u64>,
    status: TraceStatus,
    chain_name: Option<String>,
    expert_chain: Option<Vec<String>>,
}

impl TraceHandle {
    /// 创建新的追踪句柄
    pub fn new(trace_id: String, user_id: String, session_id: String, message: String) -> Self {
        Self {
            trace_id,
            user_id,
            session_id,
            message,
            start_time: std::time::Instant::now(),
            output: None,
            error: None,
            duration_ms: None,
            status: TraceStatus::InProgress,
            chain_name: None,
            expert_chain: None,
        }
    }

    /// 标记追踪成功完成
    pub fn complete_success(&mut self, output: String, duration_ms: u64) {
        self.output = Some(output);
        self.duration_ms = Some(duration_ms);
        self.status = TraceStatus::Success;
        tracing::info!(
            "Trace completed: trace_id={}, user={}, session={}, duration={}ms, chain={:?}, experts={:?}",
            self.trace_id,
            self.user_id,
            self.session_id,
            duration_ms,
            self.chain_name,
            self.expert_chain,
        );
    }

    /// 标记追踪失败
    pub fn complete_failed(&mut self, error: String, duration_ms: u64) {
        self.error = Some(error.clone());
        self.duration_ms = Some(duration_ms);
        self.status = TraceStatus::Failed;
        tracing::error!(
            "Trace failed: trace_id={}, user={}, session={}, error={}, duration={}ms",
            self.trace_id,
            self.user_id,
            self.session_id,
            error,
            duration_ms,
        );
    }

    /// 获取开始时间
    pub fn start_time(&self) -> std::time::Instant {
        self.start_time
    }

    /// 获取用户 ID
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// 获取会话 ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 获取消息（用户输入）
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 获取输出
    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
    }

    /// 获取错误信息
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 获取耗时（毫秒）
    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }

    /// 获取追踪状态
    pub fn status(&self) -> TraceStatus {
        self.status
    }

    /// 设置策略链名称
    pub fn set_chain_name(&mut self, name: Option<String>) {
        self.chain_name = name;
    }

    /// 设置专家执行链
    pub fn set_expert_chain(&mut self, chain: Option<Vec<String>>) {
        self.expert_chain = chain;
    }

    /// 获取策略链名称
    pub fn chain_name(&self) -> Option<&str> {
        self.chain_name.as_deref()
    }

    /// 获取专家执行链
    pub fn expert_chain_list(&self) -> Vec<String> {
        self.expert_chain.clone().unwrap_or_default()
    }
}

/// 细粒度 Span 数据（DTO）
///
/// 由框架 EventBus 事件转换而来，按 trace_id 归集。
/// 存储在 SubhutiTraceObserverAdapter.spans HashMap 中，供 get_span_tree 组装。
#[derive(Debug, Clone)]
pub struct SpanData {
    /// span 类型：对应框架 AgentEventData 的变体名（snake_case）
    ///   "user_message" / "chain_selected" / "agent_started" / "agent_completed" /
    ///   "agent_failed" / "graph_started" / "graph_completed" /
    ///   "flow_started" / "flow_step_executed" / "flow_completed"
    pub span_type: String,
    /// 人类可读名称（专家名、节点名、flow_type 等）
    pub name: String,
    /// 输入（部分事件有，如 agent_started.input）
    pub input: Option<String>,
    /// 输出（部分事件有，如 agent_completed.output）
    pub output: Option<String>,
    /// 耗时（毫秒，部分事件有）
    pub duration_ms: Option<u64>,
    /// Token 使用量（部分 LLM 事件有，若未来接入）
    pub tokens: Option<u64>,
    /// 事件发生时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 成功/失败（agent_completed/agent_failed 等事件）
    pub success: Option<bool>,
    /// 额外元数据（策略名、step_index 等）
    pub extra: std::collections::HashMap<String, String>,
}

/// 会话记录参数（DTO）
///
/// 封装会话记录数据，不泄漏框架类型。
#[derive(Debug, Clone)]
pub struct SessionRecordParams {
    pub session_id: String,
    pub user_id: Option<String>,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub trace_id: String,
    pub input: String,
    pub output: Option<String>,
    pub duration_ms: Option<u64>,
    pub matched_skill: Option<String>,
    pub token_usage: Option<String>,
    pub status: String,
}

// ─── 观察者 Port（Driven Port，供入站适配层中间件使用）────────────────

/// 追踪观察者端口（出站端口）
///
/// 入站适配层通过此接口记录执行链路，具体实现由出站适配层提供。
/// TraceEventBridge（订阅框架 EventBus）通过 `record_span` 写入细粒度 span。
pub trait TraceObserverPort: Send + Sync + 'static {
    /// 创建追踪记录
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle;

    /// 存储追踪记录
    fn store_trace(&self, trace: TraceHandle);

    /// 记录一条细粒度 span（由 EventBridge 从框架事件转换后调用）
    ///
    /// 写入 `spans: HashMap<trace_id, Vec<SpanData>>`，查询时组装嵌套树。
    fn record_span(&self, trace_id: &str, span: SpanData);

    /// 获取追踪摘要列表
    fn list_summaries(&self) -> Vec<serde_json::Value>;

    /// 根据 ID 获取追踪记录
    fn get_trace(&self, id: &str) -> Option<serde_json::Value>;

    /// 获取追踪的 Span 树
    fn get_span_tree(&self, id: &str) -> Option<serde_json::Value>;
}

/// 会话观察者端口（出站端口）
///
/// 入站适配层通过此接口记录会话信息，具体实现由出站适配层提供。
pub trait SessionObserverPort: Send + Sync + 'static {
    /// 记录会话请求
    fn record_request(&self, params: SessionRecordParams);

    /// 获取会话列表
    fn list_sessions(&self) -> Vec<serde_json::Value>;

    /// 根据 ID 获取会话
    fn get_session(&self, id: &str) -> Option<serde_json::Value>;
}
