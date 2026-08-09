//! # Common Types
//!
//! 通用类型定义，无业务逻辑。

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtxId(pub String);

impl CtxId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_str(s: &str) -> Self {
        Self(s.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CtxId {
    fn default() -> Self {
        Self::new()
    }
}
