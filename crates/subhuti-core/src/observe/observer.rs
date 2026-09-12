//! # 框架级追踪观察者
//!
//! 提供函数调用链路追踪的统一基础设施：
//! - `TraceObserverPort` — 追踪观察者端口（其他项目实现此 trait 提供存储）
//! - `FnTracer` — 函数追踪器辅助工具（自动记录 input/output/memory/duration）
//! - `record_fn_log` — 统一日志入口（同时输出到 tracing + 报告存储）
//! - `TraceHandle` — 追踪句柄（含状态机）
//! - `SpanData` / `FnCallData` / `LogEntry` — 数据 DTO
//!
//! ## 使用方式
//!
//! ```ignore
//! // 1. 实现 TraceObserverPort
//! struct MyObserver;
//! impl TraceObserverPort for MyObserver { ... }
//!
//! // 2. 在函数入口/出口使用 FnTracer
//! let tracer = FnTracer::new("my_fn", None, Some(input_json));
//! let result = my_fn().await;
//! tracer.finish(&observer, &trace_id, output_json, duration_ms, Some(true));
//!
//! // 3. 在函数关键点记录日志
//! record_fn_log(Some(&observer), &trace_id, LogLevel::Info, "开始处理", Some("my_fn"));
//! ```

use std::collections::HashMap;

/// 追踪状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStatus {
    InProgress,
    Success,
    Failed,
}

/// 追踪句柄
///
/// 封装追踪操作，含状态机与日志副作用。
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
        record_fn_log(
            None,
            "",
            LogLevel::Info,
            format!(
                "Trace completed: trace_id={}, user={}, session={}, duration={}ms, chain={:?}, experts={:?}",
                self.trace_id, self.user_id, self.session_id, duration_ms, self.chain_name, self.expert_chain
            ),
            None,
        );
    }

    /// 标记追踪失败
    pub fn complete_failed(&mut self, error: String, duration_ms: u64) {
        self.error = Some(error.clone());
        self.duration_ms = Some(duration_ms);
        self.status = TraceStatus::Failed;
        record_fn_log(
            None,
            "",
            LogLevel::Error,
            format!(
                "Trace failed: trace_id={}, user={}, session={}, error={}, duration={}ms",
                self.trace_id, self.user_id, self.session_id, error, duration_ms
            ),
            None,
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
#[derive(Debug, Clone)]
pub struct SpanData {
    /// span 类型：对应框架 AgentEventData 的变体名（snake_case）
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
    pub extra: HashMap<String, String>,
}

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// 函数内日志条目（DTO）
///
/// 记录函数执行过程中的关键调试输出，与函数调用绑定。
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// 日志级别
    pub level: LogLevel,
    /// 日志消息
    pub message: String,
    /// 时间戳
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 所属函数名（可选，用于关联到函数调用节点）
    pub fn_name: Option<String>,
}

/// 函数调用追踪数据（DTO）
///
/// 记录函数级别的调用链路，包含执行时间、传入传出数据、数据大小、内存变化。
/// 由 FnTracer 辅助工具在函数入口/出口自动记录，供 get_fn_call_tree 组装树。
#[derive(Debug, Clone)]
pub struct FnCallData {
    /// 函数名（如 ChatPort::orchestrate）
    pub fn_name: String,
    /// 输入数据（JSON 序列化）
    pub input: Option<String>,
    /// 输出数据（JSON 序列化）
    pub output: Option<String>,
    /// 输入数据大小（字节，JSON 序列化后长度）
    pub input_bytes: Option<usize>,
    /// 输出数据大小（字节，JSON 序列化后长度）
    pub output_bytes: Option<usize>,
    /// 执行耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 调用开始时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 父函数名（None = root）
    pub parent_fn_name: Option<String>,
    /// 成功/失败
    pub success: Option<bool>,
    /// 函数入口时进程内存（RSS，字节）
    pub memory_entry: Option<u64>,
    /// 函数出口时进程内存（RSS，字节）
    pub memory_exit: Option<u64>,
    /// 函数执行过程中的日志条目
    pub logs: Vec<LogEntry>,
    /// 额外元数据（如模块路径、trait 名等）
    pub extra: HashMap<String, String>,
}

/// 函数追踪器辅助工具
///
/// 在函数入口创建，记录开始时间+输入数据+当前内存；
/// 在函数出口调用 finish()，记录输出数据+耗时+数据大小+内存变化。
///
/// # 用法
///
/// ```ignore
/// let tracer = FnTracer::new(
///     "OrchestrationService::orchestrate",
///     None, // root
///     Some(serde_json::to_string(&request).unwrap()),
/// );
/// let response = inner.orchestrate(request).await;
/// tracer.finish(
///     &*trace_observer, &trace_id,
///     Some(serde_json::to_string(&response).unwrap()),
///     start.elapsed().as_millis() as u64,
///     Some(true),
/// );
/// ```
pub struct FnTracer {
    fn_name: String,
    parent_fn_name: Option<String>,
    input: Option<String>,
    input_bytes: Option<usize>,
    timestamp: chrono::DateTime<chrono::Utc>,
    memory_entry: Option<u64>,
}

impl FnTracer {
    /// 创建函数追踪器（记录入口）
    ///
    /// - `fn_name`: 函数名，如 `"ChatPort::orchestrate"`
    /// - `parent_fn_name`: 父函数名，None 表示根函数
    /// - `input`: 输入数据的 JSON 字符串（可选）
    pub fn new(
        fn_name: impl Into<String>,
        parent_fn_name: Option<String>,
        input: Option<String>,
    ) -> Self {
        Self {
            fn_name: fn_name.into(),
            parent_fn_name,
            input_bytes: input.as_ref().map(|s| s.len()),
            input,
            timestamp: chrono::Utc::now(),
            memory_entry: current_memory_rss(),
        }
    }

    /// 完成函数追踪（记录出口）
    ///
    /// 调用此方法会将 FnCallData 记录到 TraceObserverPort。
    ///
    /// - `observer`: TraceObserverPort 引用
    /// - `trace_id`: 追踪 ID
    /// - `output`: 输出数据的 JSON 字符串（可选）
    /// - `duration_ms`: 执行耗时（毫秒）
    /// - `success`: 成功/失败
    pub fn finish(
        self,
        observer: &dyn TraceObserverPort,
        trace_id: &str,
        output: Option<String>,
        duration_ms: u64,
        success: Option<bool>,
    ) {
        let output_bytes = output.as_ref().map(|s| s.len());
        let memory_exit = current_memory_rss();
        let fn_call = FnCallData {
            fn_name: self.fn_name,
            input: self.input,
            output,
            input_bytes: self.input_bytes,
            output_bytes,
            duration_ms: Some(duration_ms),
            timestamp: self.timestamp,
            parent_fn_name: self.parent_fn_name,
            success,
            memory_entry: self.memory_entry,
            memory_exit,
            logs: vec![],
            extra: HashMap::new(),
        };
        observer.record_fn_call(trace_id, fn_call);
    }
}

/// 获取当前进程的 RSS（驻留内存，字节）
///
/// 使用 `memory_stats` crate 跨平台采集。
/// 失败时返回 None（如平台不支持）。
fn current_memory_rss() -> Option<u64> {
    memory_stats::memory_stats().map(|s| s.physical_mem as u64)
}

/// 记录一条函数执行日志（便捷函数）
///
/// 统一日志入口：同时输出到终端 stdout + 日志文件（通过 tracing）和跟踪报告。
///
/// - `observer`: 可为 None（仅输出到终端/日志文件，不写入报告）
/// - `trace_id`: 追踪 ID
/// - `level`: 日志级别
/// - `message`: 日志消息
/// - `fn_name`: 所属函数名（可选，用于报告关联）
pub fn record_fn_log(
    observer: Option<&dyn TraceObserverPort>,
    trace_id: &str,
    level: LogLevel,
    message: impl Into<String>,
    fn_name: Option<&str>,
) {
    let msg = message.into();
    let fn_tag = fn_name.unwrap_or("system");
    // 始终输出到 tracing（终端 stdout + 日志文件）
    // 关键：把 trace_id 作为字段附带，使 JSON 日志与 log_stream --trace-id 能按链路过滤
    match level {
        LogLevel::Trace => {
            tracing::trace!(target: "subhuti", trace_id = %trace_id, "[{}] {}", fn_tag, msg)
        }
        LogLevel::Debug => {
            tracing::debug!(target: "subhuti", trace_id = %trace_id, "[{}] {}", fn_tag, msg)
        }
        LogLevel::Info => {
            tracing::info!(target: "subhuti", trace_id = %trace_id, "[{}] {}", fn_tag, msg)
        }
        LogLevel::Warn => {
            tracing::warn!(target: "subhuti", trace_id = %trace_id, "[{}] {}", fn_tag, msg)
        }
        LogLevel::Error => {
            tracing::error!(target: "subhuti", trace_id = %trace_id, "[{}] {}", fn_tag, msg)
        }
    }
    // 如果 observer 可用，同时写入跟踪报告
    if let Some(obs) = observer {
        let log = LogEntry {
            level,
            message: msg,
            timestamp: chrono::Utc::now(),
            fn_name: fn_name.map(|s| s.to_string()),
        };
        obs.record_fn_log(trace_id, log);
    }
}

// ─── 追踪观察者 Port ─────────────────

/// 追踪观察者端口
///
/// 应用层通过此接口记录执行链路，具体实现由出站适配层提供。
pub trait TraceObserverPort: Send + Sync + 'static {
    /// 创建追踪记录
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle;

    /// 存储追踪记录
    fn store_trace(&self, trace: TraceHandle);

    /// 记录一条细粒度 span（由 EventBridge 从框架事件转换后调用）
    fn record_span(&self, trace_id: &str, span: SpanData);

    /// 记录一条函数调用追踪数据（由 FnTracer 调用）
    fn record_fn_call(&self, trace_id: &str, fn_call: FnCallData);

    /// 记录一条函数执行日志（由业务函数在关键点调用）
    fn record_fn_log(&self, trace_id: &str, log: LogEntry);

    /// 获取函数日志列表
    fn get_fn_logs(&self, trace_id: &str) -> Vec<LogEntry>;

    /// 获取函数调用树（JSON）
    fn get_fn_call_tree(&self, trace_id: &str) -> Option<serde_json::Value>;

    /// 获取追踪摘要列表
    fn list_summaries(&self) -> Vec<serde_json::Value>;

    /// 根据 ID 获取追踪记录
    fn get_trace(&self, id: &str) -> Option<serde_json::Value>;

    /// 获取追踪的 Span 树
    fn get_span_tree(&self, id: &str) -> Option<serde_json::Value>;
}
