//! # LLM Layer - LLM 抽象层
//!
//! 统一模型 Trait（OpenAI / Ollama / 任意兼容）
//! - 统一流式 / 非流式
//! - 统一参数（temperature、max_tokens）
//! - 重试机制（自动重试 + Fallback）

mod client;
mod retry;

pub use client::{DoubaoClient, LLMClient, MockLLM, MockLlmClient, OllamaClient, OpenAIClient};
pub use retry::{chat_stream_with_retry, chat_with_retry, with_retry, RetryConfig, RetryResult};

use crate::runtime::tools::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 系统
    System,
    /// 用户
    User,
    /// 助手
    Assistant,
    /// 工具
    Tool,
}

/// 消息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    /// 角色
    pub role: Role,
    /// 内容
    pub content: String,
    /// 工具调用 ID（当 role=Tool 时需要）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    /// 创建系统消息
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// 创建用户消息
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// 创建助手消息
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
        }
    }

    /// 创建工具消息
    pub fn tool(content: impl Into<String>, tool_call_id: &str) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.to_string()),
        }
    }
}

/// LLM 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LLMConfig {
    /// 模型名称
    pub model: String,
    /// API 地址
    pub api_url: String,
    /// API Key
    pub api_key: Option<String>,
    /// Temperature
    pub temperature: f32,
    /// 最大 token 数
    pub max_tokens: usize,
}

impl Default for LLMConfig {
    fn default() -> Self {
        Self {
            model: "gpt-4".to_string(),
            api_url: "https://api.openai.com/v1".to_string(),
            api_key: None,
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

/// LLM 提供者
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LLMProvider {
    /// OpenAI
    OpenAI,
    /// Ollama (本地)
    Ollama,
    /// Doubao (字节跳动)
    Doubao,
    /// 自定义
    Custom,
}

/// LLM Trait - 统一模型接口
#[async_trait]
pub trait LLM: Send + Sync {
    /// 获取提供者
    fn provider(&self) -> LLMProvider;

    /// 获取配置
    fn config(&self) -> &LLMConfig;

    /// 聊天完成 (非流式)
    async fn chat(&self, messages: Vec<Message>) -> Result<String>;

    /// 聊天完成 - 支持工具调用
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse>;

    /// 聊天完成 (流式)
    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()>;

    /// 检查健康状态
    async fn health_check(&self) -> Result<bool>;
}

/// LLM 响应
#[derive(Debug, Clone)]
pub struct LLMResponse {
    /// 文本响应
    pub content: String,
    /// 工具调用（如果有）
    pub tool_call: Option<ToolCall>,
    /// 使用的模型
    pub model: Option<String>,
    /// Prompt Token 数量
    pub prompt_tokens: Option<u32>,
    /// Completion Token 数量
    pub completion_tokens: Option<u32>,
    /// 总 Token 数量
    pub total_tokens: Option<u32>,
}

/// 工具信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// 工具类型（目前固定为 function）
    #[serde(rename = "type")]
    pub tool_type: String,
    /// 函数定义
    pub function: FunctionDefinition,
}

/// 函数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    /// 函数名称
    pub name: String,
    /// 函数描述
    pub description: String,
    /// 参数模式
    pub parameters: serde_json::Value,
}

impl ToolInfo {
    /// 从 Tool trait 创建
    pub fn from_tool<T: Tool>(tool: &T) -> Self {
        let info = tool.info();
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: info.name,
                description: info.description,
                parameters: info.parameters,
            },
        }
    }
}

/// 工具调用
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    /// 调用 ID
    pub id: String,
    /// 工具名称
    pub name: String,
    /// 参数 (JSON)
    pub arguments: serde_json::Value,
}

/// 工具调用结果
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallResult {
    /// 调用 ID
    pub id: String,
    /// 结果 (JSON)
    pub result: serde_json::Value,
    /// 是否错误
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content, "Hello");

        let sys = Message::system("You are helpful");
        assert_eq!(sys.role, Role::System);
    }
}
