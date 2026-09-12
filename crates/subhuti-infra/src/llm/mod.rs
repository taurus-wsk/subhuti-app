//! # LLM 基础设施层
//!
//! 具体的 LLM API 客户端实现，对接第三方服务。

pub mod cached_llm;
pub mod client;
pub mod limits;
pub mod retry;

pub use cached_llm::{CacheStats, CachedLLM};
pub use client::*;
pub use limits::{trim_messages, ContextLimitLLM, LimitConfig, TrimOutcome};
pub use retry::{compute_backoff, RetryConfig, RetryLLM};
