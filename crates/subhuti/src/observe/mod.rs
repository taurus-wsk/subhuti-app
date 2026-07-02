//! # 可观测性系统
//!
//! 分层设计：
//! - **调试链路层**：使用标准 tracing Span 体系，自动形成调用树
//! - **业务埋点层**：只保留业务落库字段，用于持久化和审计

pub mod session;
pub mod trace;

pub use trace::{TokenUsage, Trace, TraceId, TraceObserver, TraceStatus, TraceStore};

pub use session::{SessionObserver, SessionRecord, SessionRecordParams, SessionRequest};
