//! # Observe
//!
//! 观测和追踪接口定义。

use serde::{Deserialize, Serialize};

pub mod session;
pub mod trace;

pub use session::*;
pub use trace::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Success,
    Failed,
    InProgress,
    Cancelled,
}
