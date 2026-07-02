//! # 可观测性 Trace 系统
//!
//! ## 分层设计
//!
//! ### 调试链路层（使用标准 tracing Span 体系）
//! - 自动形成调用树：orchestrate → dispatch → expert_run → llm_call
//! - 所有日志自动携带 trace_id、user_id、session_id 等字段
//! - 开发期：彩色终端输出，带 span 层级
//! - 生产期：JSON 格式输出，配合 jq/grep 过滤
//! - 零成本享受整个 tracing 生态工具（tracing-flame, tokio-console 等）
//!
//! ### 业务埋点层（仅保留业务落库字段）
//! - Trace 结构体只记录需要持久化的业务数据
//! - TraceObserver 负责存储和查询业务 Trace
//! - 调试信息全部走 tracing，不在这里记录
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! // HTTP 入口：创建业务 trace 后，立刻开一个顶层 span
//! let mut trace = state.trace_observer.create_trace(&user_id, &session_id, &req.message);
//! let trace_id = trace.id.as_str().to_string();
//!
//! let span = tracing::info_span!(
//!     "orchestrate",
//!     %trace_id,
//!     %user_id,
//!     %session_id,
//! );
//! let _enter = span.enter();
//!
//! // 后续所有 tracing::info!/debug!/error! 都会自动带上上面的字段
//! tracing::info!("开始处理请求");
//!
//! // 子模块开嵌套 span
//! let plugin_span = tracing::debug_span!(
//!     "expert_run",
//!     expert_id = %expert.id(),
//!     step = step_idx,
//! );
//! let _plugin_enter = plugin_span.enter();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId(pub String);

impl Default for TraceId {
    fn default() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl TraceId {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceStatus {
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub id: TraceId,
    pub user_id: String,
    pub session_id: String,
    pub input: String,
    pub output: Option<String>,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub total_duration_ms: Option<u64>,
    pub chain_name: Option<String>,
    pub expert_chain: Vec<String>,
    pub status: TraceStatus,
}

impl Trace {
    pub fn new(user_id: &str, session_id: &str, input: &str) -> Self {
        Self {
            id: TraceId::new(),
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            input: input.to_string(),
            output: None,
            started_at: chrono::Utc::now(),
            ended_at: None,
            total_duration_ms: None,
            chain_name: None,
            expert_chain: Vec::new(),
            status: TraceStatus::Running,
        }
    }

    pub fn complete_success(&mut self, output: String, duration_ms: u64) {
        self.output = Some(output);
        self.total_duration_ms = Some(duration_ms);
        self.ended_at = Some(chrono::Utc::now());
        self.status = TraceStatus::Success;
    }

    pub fn complete_failed(&mut self, error: String, duration_ms: u64) {
        self.output = Some(error);
        self.total_duration_ms = Some(duration_ms);
        self.ended_at = Some(chrono::Utc::now());
        self.status = TraceStatus::Failed;
    }
}

#[derive(Debug, Clone, Default)]
pub struct TraceStore {
    traces: HashMap<String, Trace>,
}

impl TraceStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(&mut self, trace: Trace) {
        self.traces.insert(trace.id.0.clone(), trace);
    }

    pub fn get(&self, trace_id: &str) -> Option<&Trace> {
        self.traces.get(trace_id)
    }

    pub fn list(&self) -> Vec<&Trace> {
        self.traces.values().collect()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TraceObserver {
    store: TraceStore,
}

impl TraceObserver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_trace(&self, user_id: &str, session_id: &str, input: &str) -> Trace {
        Trace::new(user_id, session_id, input)
    }

    pub fn store_trace(&self, _trace: Trace) {}

    pub fn get_trace(&self, trace_id: &str) -> Option<&Trace> {
        self.store.get(trace_id)
    }

    pub fn list_summaries(&self) -> Vec<(String, String, String)> {
        Vec::new()
    }

    pub fn get_span_tree(&self, _trace_id: &str) -> Option<serde_json::Value> {
        None
    }
}
