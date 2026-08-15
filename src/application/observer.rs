//! # 观察者端口与 DTO
//!
//! trace 相关类型直接从 `subhuti_core::observe` 导入（单一事实来源）。
//! session 相关类型（应用特有）保留在此文件。
//!
//! ## 重导出
//! - `TraceStatus` / `TraceHandle` / `SpanData` — 追踪 DTO
//! - `LogLevel` / `LogEntry` / `FnCallData` — 日志与函数调用 DTO
//! - `FnTracer` — 函数追踪器辅助工具
//! - `TraceObserverPort` — 追踪观察者端口
//! - `record_fn_log` — 统一日志入口
//!
//! ## 应用特有
//! - `SessionObserverPort` — 会话观察者端口
//! - `SessionRecordParams` — 会话记录参数

// ─── 重导出框架 trace 类型（来自 subhuti_core，唯一事实来源） ─────────────────
pub use subhuti_core::observe::{
    record_fn_log, FnCallData, FnTracer, LogEntry, LogLevel, SpanData, TraceHandle,
    TraceObserverPort, TraceStatus,
};

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
