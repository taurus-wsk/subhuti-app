//! # Tool Interface
//!
//! 工具系统接口定义。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCommand {
    pub name: String,
    pub arguments: serde_json::Value,
}

impl ToolCommand {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            arguments: serde_json::Value::Null,
        }
    }

    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.arguments = args;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCommandInfo {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub data: serde_json::Value,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(content: &str) -> Self {
        Self {
            content: content.to_string(),
            data: serde_json::Value::Null,
            is_error: false,
        }
    }

    pub fn err(content: &str) -> Self {
        Self {
            content: content.to_string(),
            data: serde_json::Value::Null,
            is_error: true,
        }
    }
}

#[async_trait]
pub trait ToolIntegration: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;
    async fn check_available(&self) -> bool;
    async fn list_commands(&self) -> Vec<ToolCommandInfo>;
    async fn execute(&self, command: ToolCommand) -> crate::Result<ToolResult>;
}

#[async_trait]
pub trait ToolRegistry: Send + Sync {
    async fn register(&self, tool: Arc<dyn ToolIntegration>) -> crate::Result<()>;
    async fn unregister(&self, name: &str) -> crate::Result<()>;
    async fn get_tool(&self, name: &str) -> Option<Arc<dyn ToolIntegration>>;
    async fn list_tools(&self) -> Vec<Arc<dyn ToolIntegration>>;
    async fn is_available(&self, name: &str) -> bool;
    async fn execute(&self, tool_name: &str, command: ToolCommand) -> crate::Result<ToolResult>;
}
