//! # LLM Client - LLM 客户端实现
//!
//! 提供 OpenAI、Ollama 和 Doubao 的实现

use super::{LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolCall, ToolInfo, LLM};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;

/// OpenAI API 请求
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
    max_tokens: usize,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

impl From<Message> for OpenAIMessage {
    fn from(msg: Message) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        Self {
            role: role.to_string(),
            content: msg.content,
        }
    }
}

/// OpenAI API 响应
#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessageResponse {
    content: Option<String>,
}

/// OpenAI Chat Completions API 请求（支持 function calling）
#[derive(Debug, Serialize)]
struct OpenAIChatRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
    max_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolInfo>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessageContent {
    #[allow(dead_code)]
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<OpenAIFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

/// OpenAI Chat Completions API 响应
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChatChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChatChoice {
    message: OpenAIMessageContent,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

/// OpenAI LLM 客户端
#[derive(Debug, Clone)]
pub struct OpenAIClient {
    config: LLMConfig,
    http_client: Client,
}

impl OpenAIClient {
    /// 创建新的 OpenAI 客户端
    pub fn new(config: LLMConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }
}

#[async_trait]
impl LLM for OpenAIClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::OpenAI
    }

    fn config(&self) -> &LLMConfig {
        &self.config
    }

    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let request = OpenAIRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(OpenAIMessage::from).collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: false,
        };

        let url = format!("{}/chat/completions", self.config.api_url);
        let mut req_builder = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json");

        if let Some(ref key) = self.config.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", key));
        }

        let response = req_builder
            .json(&request)
            .send()
            .await
            .context("Failed to send request to OpenAI")?;

        let openai_response: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        openai_response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .ok_or_else(|| anyhow::anyhow!("No content in response"))
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse> {
        // OpenAI 支持 function calling
        let request = OpenAIChatRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(OpenAIMessage::from).collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            tools: Some(tools),
        };

        let url = format!("{}/chat/completions", self.config.api_url);
        let mut req_builder = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json");

        if let Some(ref key) = self.config.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", key));
        }

        let response = req_builder
            .json(&request)
            .send()
            .await
            .context("Failed to send request to OpenAI")?;

        let openai_response: OpenAIChatResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI response")?;

        if let Some(choice) = openai_response.choices.first() {
            if let Some(function_call) = &choice.message.function_call {
                return Ok(LLMResponse {
                    content: function_call.arguments.clone(),
                    tool_call: Some(ToolCall {
                        id: format!("call_{}", uuid_simple()),
                        name: function_call.name.clone(),
                        arguments: serde_json::from_str(&function_call.arguments)
                            .unwrap_or(serde_json::Value::String(function_call.arguments.clone())),
                    }),
                    model: Some(self.config.model.clone()),
                    prompt_tokens: None,
                    completion_tokens: None,
                    total_tokens: None,
                });
            }
            return Ok(LLMResponse {
                content: choice.message.content.clone().unwrap_or_default(),
                tool_call: None,
                model: Some(self.config.model.clone()),
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
            });
        }

        anyhow::bail!("No content in response")
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        // 简化实现，实际应该处理 SSE 流
        let response = self.chat(messages).await?;
        callback(response);
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        // 简化实现
        Ok(true)
    }
}

/// Ollama API 请求
#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

impl From<Message> for OllamaMessage {
    fn from(msg: Message) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        Self {
            role: role.to_string(),
            content: msg.content,
        }
    }
}

/// Ollama API 响应
#[derive(Debug, Deserialize)]
struct OllamaResponse {
    message: OllamaMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OllamaMessageResponse {
    content: String,
}

/// Ollama Chat API 请求（支持 function calling）
#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolInfo>>,
}

/// Ollama Chat API 响应
#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaChatMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OllamaChatMessageResponse {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OllamaToolCall {
    function: OllamaFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OllamaFunctionCall {
    name: String,
    arguments: serde_json::Value,
}

/// Ollama LLM 客户端
#[derive(Debug, Clone)]
pub struct OllamaClient {
    config: LLMConfig,
    http_client: Client,
}

impl OllamaClient {
    /// 创建新的 Ollama 客户端
    pub fn new(config: LLMConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }
}

#[async_trait]
impl LLM for OllamaClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Ollama
    }

    fn config(&self) -> &LLMConfig {
        &self.config
    }

    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        let request = OllamaRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(OllamaMessage::from).collect(),
            stream: false,
        };

        let url = format!("{}/api/chat", self.config.api_url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Ollama")?;

        let ollama_response: OllamaResponse = response
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        Ok(ollama_response.message.content)
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        // 简化实现
        let response = self.chat(messages).await?;
        callback(response);
        Ok(())
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse> {
        // Ollama 也支持 function calling，使用格式化的 tools
        let request = OllamaChatRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(OllamaMessage::from).collect(),
            tools: Some(tools),
        };

        let url = format!("{}/api/chat", self.config.api_url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Ollama")?;

        let ollama_response: OllamaChatResponse = response
            .json()
            .await
            .context("Failed to parse Ollama chat response")?;

        // 检查是否有 tool_call
        if let Some(tool_calls) = ollama_response.message.tool_calls {
            if let Some(tool_call) = tool_calls.first() {
                return Ok(LLMResponse {
                    content: serde_json::to_string(&tool_call.function.arguments)
                        .unwrap_or_default(),
                    tool_call: Some(ToolCall {
                        id: format!("call_{}", uuid_simple()),
                        name: tool_call.function.name.clone(),
                        arguments: tool_call.function.arguments.clone(),
                    }),
                    model: Some(self.config.model.clone()),
                    prompt_tokens: None,
                    completion_tokens: None,
                    total_tokens: None,
                });
            }
        }

        Ok(LLMResponse {
            content: ollama_response.message.content,
            tool_call: None,
            model: Some(self.config.model.clone()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/tags", self.config.api_url);
        match self.http_client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

/// Doubao API 请求（为 OpenAI 兼容格式预留）
#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct DoubaoRequest {
    model: String,
    input: Vec<DoubaoMessage>,
    temperature: f32,
}

#[allow(dead_code)]
#[derive(Debug, Serialize)]
struct DoubaoMessage {
    role: String,
    content: String,
}

impl From<Message> for DoubaoMessage {
    fn from(msg: Message) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        Self {
            role: role.to_string(),
            content: msg.content,
        }
    }
}

/// Doubao API 响应
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DoubaoResponse {
    output: Vec<DoubaoOutput>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DoubaoOutput {
    content: Option<Vec<DoubaoContent>>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct DoubaoContent {
    text: String,
}

/// Doubao Chat Completions API 请求（支持 function calling）
#[derive(Debug, Serialize)]
struct DoubaoChatRequest {
    model: String,
    messages: Vec<DoubaoChatMessage>,
    temperature: f32,
    tools: Option<Vec<ToolInfo>>,
}

#[derive(Debug, Serialize)]
struct DoubaoChatMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>, // 当 role=tool 时需要
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<DoubaoFunctionCall>,
}

impl From<Message> for DoubaoChatMessage {
    fn from(msg: Message) -> Self {
        let role = match msg.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        Self {
            role: role.to_string(),
            content: Some(msg.content),
            name: None,
            tool_call_id: msg.tool_call_id,
            function_call: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DoubaoFunctionCall {
    name: String,
    arguments: String,
}

/// Doubao Chat Completions API 响应
#[derive(Debug, Deserialize)]
struct DoubaoChatResponse {
    id: Option<String>,
    choices: Vec<DoubaoChatChoice>,
    usage: Option<DoubaoUsage>,
}

#[derive(Debug, Deserialize)]
struct DoubaoChatChoice {
    message: DoubaoChatMessageContent,
    #[allow(dead_code)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DoubaoChatMessageContent {
    #[allow(dead_code)]
    role: String,
    content: String,
    /// 工具调用数组（豆包 API 使用这个字段名）
    #[serde(rename = "tool_calls", skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<DoubaoToolCallItem>>,
    /// 单个函数调用（旧格式，保留兼容性）
    #[serde(skip_serializing_if = "Option::is_none")]
    function_call: Option<DoubaoFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct DoubaoToolCallItem {
    id: Option<String>,
    #[allow(dead_code)]
    call_type: Option<String>,
    function: DoubaoFunctionCall,
}

#[derive(Debug, Deserialize)]
struct DoubaoUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Doubao LLM 客户端
#[derive(Debug, Clone)]
pub struct DoubaoClient {
    config: LLMConfig,
    http_client: Client,
    api_key: String,
}

impl DoubaoClient {
    /// 创建新的 Doubao 客户端
    pub fn new(mut config: LLMConfig) -> Result<Self> {
        load_subhuti_env();

        let api_key = if let Some(key) = config.api_key.take() {
            key
        } else {
            env::var("DOUBAO_API_KEY").context("DOUBAO_API_KEY not set")?
        };

        // 创建带超时的 HTTP client
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            config,
            http_client,
            api_key,
        })
    }

    /// 同步版本 - 给 Executor 用
    pub fn chat_sync(&self, messages: Vec<Message>) -> Result<String> {
        tokio::runtime::Runtime::new()?.block_on(async { self.chat(messages).await })
    }
}

#[async_trait]
impl LLM for DoubaoClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Doubao
    }

    fn config(&self) -> &LLMConfig {
        &self.config
    }

    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        // 使用 Chat Completions API
        let request = DoubaoChatRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(DoubaoChatMessage::from).collect(),
            temperature: self.config.temperature,
            tools: None, // 不传入 tools
        };

        let url = if self.config.api_url.is_empty() {
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string()
        } else {
            self.config
                .api_url
                .clone()
                .replace("/responses", "/chat/completions")
        };

        tracing::debug!("Doubao chat request URL: {}", url);
        tracing::debug!(
            "Doubao chat request: {:?}",
            serde_json::to_string(&request).unwrap_or_default()
        );

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Doubao")?;

        let status = response.status();
        let body_text = response
            .text()
            .await
            .context("Failed to read response body")?;
        tracing::debug!(
            "Doubao chat response status: {}, body: {}",
            status,
            body_text
        );

        if !status.is_success() {
            anyhow::bail!("Doubao API error: {} - {}", status, body_text);
        }

        let doubao_response: DoubaoChatResponse =
            serde_json::from_str(&body_text).context("Failed to parse Doubao chat response")?;

        tracing::debug!("Doubao chat response: {:?}", doubao_response);

        // 解析响应
        if let Some(choice) = doubao_response.choices.first() {
            tracing::debug!("Choice message: content={:?}", choice.message.content);

            // 普通文本响应
            let content = choice.message.content.clone();
            if content.is_empty() {
                tracing::warn!(
                    "Doubao returned empty content, finish_reason={:?}",
                    choice.finish_reason
                );
            }
            return Ok(content);
        }

        anyhow::bail!("No content in chat response")
    }

    /// 使用 Chat Completions API（支持 function calling）
    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse> {
        // 使用 chat/completions API
        let url = if self.config.api_url.is_empty() {
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions".to_string()
        } else {
            self.config
                .api_url
                .clone()
                .replace("/responses", "/chat/completions")
        };

        let request = DoubaoChatRequest {
            model: self.config.model.clone(),
            messages: messages.into_iter().map(DoubaoChatMessage::from).collect(),
            temperature: self.config.temperature,
            tools: Some(tools),
        };

        tracing::debug!("Doubao chat request URL: {}", url);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Doubao chat API")?;

        let doubao_response: DoubaoChatResponse = response
            .json()
            .await
            .context("Failed to parse Doubao chat response")?;

        tracing::debug!("Doubao chat response: {:?}", doubao_response);

        // 解析响应
        if let Some(choice) = doubao_response.choices.first() {
            tracing::debug!(
                "Choice message: content={:?}, tool_calls={:?}, function_call={:?}",
                choice.message.content,
                choice.message.tool_calls,
                choice.message.function_call
            );

            // 检查是否有 tool_calls（豆包 API 使用 tool_calls 数组）
            if let Some(tool_calls) = &choice.message.tool_calls {
                if !tool_calls.is_empty() {
                    if let Some(tool_call) = tool_calls.first() {
                        return Ok(LLMResponse {
                            content: tool_call.function.arguments.clone(),
                            tool_call: Some(ToolCall {
                                id: tool_call
                                    .id
                                    .clone()
                                    .unwrap_or_else(|| format!("call_{}", uuid_simple())),
                                name: tool_call.function.name.clone(),
                                arguments: serde_json::from_str(&tool_call.function.arguments)
                                    .unwrap_or(serde_json::Value::String(
                                        tool_call.function.arguments.clone(),
                                    )),
                            }),
                            model: Some(self.config.model.clone()),
                            prompt_tokens: doubao_response
                                .usage
                                .as_ref()
                                .and_then(|u| u.prompt_tokens),
                            completion_tokens: doubao_response
                                .usage
                                .as_ref()
                                .and_then(|u| u.completion_tokens),
                            total_tokens: doubao_response
                                .usage
                                .as_ref()
                                .and_then(|u| u.total_tokens),
                        });
                    }
                }
            }

            // 兼容旧的 function_call 格式
            if let Some(function_call) = &choice.message.function_call {
                return Ok(LLMResponse {
                    content: function_call.arguments.clone(),
                    tool_call: Some(ToolCall {
                        id: format!("call_{}", uuid_simple()),
                        name: function_call.name.clone(),
                        arguments: serde_json::from_str(&function_call.arguments)
                            .unwrap_or(serde_json::Value::String(function_call.arguments.clone())),
                    }),
                    model: Some(
                        doubao_response
                            .id
                            .unwrap_or_else(|| self.config.model.clone()),
                    ),
                    prompt_tokens: doubao_response.usage.as_ref().and_then(|u| u.prompt_tokens),
                    completion_tokens: doubao_response
                        .usage
                        .as_ref()
                        .and_then(|u| u.completion_tokens),
                    total_tokens: doubao_response.usage.as_ref().and_then(|u| u.total_tokens),
                });
            }

            // 普通文本响应（finish_reason 可能是 "stop" 或其他）
            let content = choice.message.content.clone();
            if content.is_empty() {
                tracing::warn!(
                    "Doubao returned empty content, finish_reason={:?}",
                    choice.finish_reason
                );
                // 如果 finish_reason 是 tool_calls 但没有内容，可能是模型需要更多信息
                if choice.finish_reason.as_deref() == Some("tool_calls") {
                    anyhow::bail!("Model requested tool but provided no tool_calls in response")
                }
            }
            return Ok(LLMResponse {
                content,
                tool_call: None,
                model: Some(self.config.model.clone()),
                prompt_tokens: doubao_response.usage.as_ref().and_then(|u| u.prompt_tokens),
                completion_tokens: doubao_response
                    .usage
                    .as_ref()
                    .and_then(|u| u.completion_tokens),
                total_tokens: doubao_response.usage.as_ref().and_then(|u| u.total_tokens),
            });
        }

        anyhow::bail!("No content in chat response")
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        let response = self.chat(messages).await?;
        callback(response);
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        // 简化实现
        Ok(true)
    }
}

fn load_subhuti_env() {
    // 从当前工作目录加载 .env（项目根目录）
    match dotenvy::dotenv() {
        Ok(_) => tracing::info!("Loaded environment variables"),
        Err(e) => tracing::debug!("No .env file or load failed: {}", e),
    }
}

// ============================================================
// MockLLM - 测试用 LLM 模拟
// ============================================================

use std::sync::Mutex;

/// LLM Mock 客户端（用于测试）
///
/// 支持：
/// - 预设响应队列（按顺序返回）
/// - 捕获请求消息（验证 Prompt 构建）
/// - 工具调用响应
///
/// ```rust,ignore
/// let mock = MockLLM::with_response("这是模拟响应");
/// runtime.set_mock_llm(mock);
/// let result = runtime.call_llm(messages).await?;
/// assert_eq!(result, "这是模拟响应");
/// ```
pub struct MockLLM {
    config: LLMConfig,
    /// 预设响应队列
    responses: Mutex<Vec<String>>,
    /// 预设工具调用响应
    tool_call_responses: Mutex<Vec<ToolCall>>,
    /// 捕获的消息历史（每次调用 chat 时记录）
    captured_messages: Mutex<Vec<Vec<Message>>>,
    /// 调用次数
    call_count: Mutex<usize>,
}

impl std::fmt::Debug for MockLLM {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockLLM")
            .field("config", &self.config)
            .field("responses_len", &self.responses.lock().unwrap().len())
            .field(
                "captured_count",
                &self.captured_messages.lock().unwrap().len(),
            )
            .finish()
    }
}

impl MockLLM {
    /// 创建空 MockLLM
    pub fn new() -> Self {
        Self {
            config: LLMConfig::default(),
            responses: Mutex::new(Vec::new()),
            tool_call_responses: Mutex::new(Vec::new()),
            captured_messages: Mutex::new(Vec::new()),
            call_count: Mutex::new(0),
        }
    }

    /// 创建带单个固定响应的 MockLLM
    pub fn with_response(response: &str) -> Self {
        let mock = Self::new();
        mock.add_response(response);
        mock
    }

    /// 添加一个预设响应（按顺序消费）
    pub fn add_response(&self, response: &str) {
        self.responses.lock().unwrap().push(response.to_string());
    }

    /// 批量添加预设响应
    pub fn add_responses(&self, responses: Vec<&str>) {
        let mut q = self.responses.lock().unwrap();
        for r in responses {
            q.push(r.to_string());
        }
    }

    /// 添加预设工具调用响应
    pub fn add_tool_call_response(&self, tool_call: ToolCall) {
        self.tool_call_responses.lock().unwrap().push(tool_call);
    }

    /// 获取捕获的消息历史（所有调用记录）
    pub fn get_captured_messages(&self) -> Vec<Vec<Message>> {
        self.captured_messages.lock().unwrap().clone()
    }

    /// 获取最后一次调用的消息
    pub fn get_last_messages(&self) -> Option<Vec<Message>> {
        self.captured_messages.lock().unwrap().last().cloned()
    }

    /// 获取总调用次数
    pub fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    /// 重置状态
    pub fn reset(&self) {
        self.responses.lock().unwrap().clear();
        self.tool_call_responses.lock().unwrap().clear();
        self.captured_messages.lock().unwrap().clear();
        *self.call_count.lock().unwrap() = 0;
    }
}

impl Default for MockLLM {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LLM for MockLLM {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Custom
    }

    fn config(&self) -> &LLMConfig {
        &self.config
    }

    async fn chat(&self, messages: Vec<Message>) -> Result<String> {
        *self.call_count.lock().unwrap() += 1;
        self.captured_messages
            .lock()
            .unwrap()
            .push(messages.clone());

        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            // 默认回显最后一条用户消息
            let last = messages
                .iter()
                .rev()
                .find(|m| m.role == Role::User)
                .map(|m| m.content.clone())
                .unwrap_or_else(|| "[MockLLM] no preset response".to_string());
            Ok(last)
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        _tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse> {
        *self.call_count.lock().unwrap() += 1;
        self.captured_messages.lock().unwrap().push(messages);

        // 先检查是否有工具调用预设
        let mut tool_calls = self.tool_call_responses.lock().unwrap();
        if !tool_calls.is_empty() {
            let tool_call = tool_calls.remove(0);
            return Ok(LLMResponse {
                content: format!(
                    "[MockLLM] tool_call: {}({})",
                    tool_call.name, tool_call.arguments
                ),
                tool_call: Some(tool_call),
                model: Some("mock-llm".to_string()),
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            });
        }

        // 否则返回文本响应
        let mut responses = self.responses.lock().unwrap();
        let content = if responses.is_empty() {
            "[MockLLM] default response".to_string()
        } else {
            responses.remove(0)
        };

        Ok(LLMResponse {
            content,
            tool_call: None,
            model: Some("mock-llm".to_string()),
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_tokens: Some(15),
        })
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        let response = self.chat(messages).await?;
        // 按词分段模拟流式输出
        for word in response.split_whitespace() {
            callback(format!("{} ", word));
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// 简单的 UUID 生成
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        "{:x}{:x}{:x}",
        (timestamp >> 64) as u32,
        (timestamp >> 32) as u16 as u32,
        timestamp as u32
    )
}

/// LLM 客户端工厂
pub enum LLMClient {
    OpenAI(OpenAIClient),
    Ollama(OllamaClient),
    Doubao(DoubaoClient),
}

impl LLMClient {
    /// 从配置创建客户端
    pub fn from_config(config: LLMConfig) -> Result<Self> {
        if config.api_url.contains("ollama") {
            Ok(LLMClient::Ollama(OllamaClient::new(config)))
        } else if config.api_url.contains("volces") || config.model.contains("doubao") {
            DoubaoClient::new(config).map(LLMClient::Doubao)
        } else {
            Ok(LLMClient::OpenAI(OpenAIClient::new(config)))
        }
    }

    /// 获取底层 LLM trait 对象
    pub fn as_llm(&self) -> &dyn LLM {
        match self {
            LLMClient::OpenAI(c) => c,
            LLMClient::Ollama(c) => c,
            LLMClient::Doubao(c) => c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_message_conversion() {
        let msg = Message::user("Hello");
        let openai_msg: OpenAIMessage = msg.into();
        assert_eq!(openai_msg.role, "user");
        assert_eq!(openai_msg.content, "Hello");
    }

    #[test]
    fn test_ollama_message_conversion() {
        let msg = Message::system("You are helpful");
        let ollama_msg: OllamaMessage = msg.into();
        assert_eq!(ollama_msg.role, "system");
        assert_eq!(ollama_msg.content, "You are helpful");
    }
}
