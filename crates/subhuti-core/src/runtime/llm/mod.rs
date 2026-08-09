//! # LLM Layer - LLM 抽象层
//!
//! 统一模型 Trait（OpenAI / Ollama / 任意兼容）
//! - 统一流式 / 非流式
//! - 统一参数（temperature、max_tokens）
//! - 重试机制（自动重试 + Fallback）

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// 消息
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
        }
    }

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
    pub model: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub temperature: f32,
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
    OpenAI,
    Ollama,
    Doubao,
    /// 智谱 AI (GLM-4 / GLM-4.7-Flash 系列)，OpenAI 兼容协议
    Zhipu,
    Custom,
}

/// LLM Trait - 统一模型接口
#[async_trait]
pub trait LLM: Send + Sync {
    fn provider(&self) -> LLMProvider;
    fn config(&self) -> &LLMConfig;
    async fn chat(&self, messages: Vec<Message>) -> crate::Result<String>;
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> crate::Result<LLMResponse>;
    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> crate::Result<()>;
    async fn health_check(&self) -> crate::Result<bool>;
}

/// LLM 响应
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: String,
    pub tool_call: Option<ToolCall>,
    pub model: Option<String>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// 工具信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// 函数定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl ToolInfo {
    pub fn from_name_and_desc(
        name: &str,
        description: &str,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}

/// 工具调用
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// 工具调用结果
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCallResult {
    pub id: String,
    pub result: serde_json::Value,
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
