use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::{Arc, Mutex};
use subhuti_core::runtime::llm::{
    LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolCall, ToolInfo, LLM,
};
use tracing::debug;

fn map_reqwest_error(e: reqwest::Error) -> subhuti_core::Error {
    // 归类网络层错误，方便前端/日志快速定位根因
    let mut kind = if e.is_timeout() {
        "网络超时"
    } else if e.is_connect() {
        "连接失败"
    } else if e.is_request() {
        "请求未发送成功"
    } else if e.is_body() {
        "响应体读取失败"
    } else if e.is_decode() {
        "响应解析失败"
    } else if e.is_builder() {
        "请求构建错误"
    } else if e.is_redirect() {
        "重定向失败"
    } else {
        "HTTP/传输错误"
    }
    .to_string();

    // 记录 HTTP 状态码：若错误携带 status（如 4xx 上下文超限/401/429），优先展示，
    // 便于区分「服务端拒绝(4xx)」与「网络层失败(无 status)」
    if let Some(status) = e.status() {
        kind = format!("HTTP {} ", status.as_u16());
    }

    // 递归收集底层错误链（hyper/tonic 的连接、TLS、超时等具体原因）
    let mut detail = String::new();
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        let msg = s.to_string();
        if !msg.is_empty() && !detail.contains(&msg) {
            detail.push_str(&format!(" <- {}", msg));
        }
        source = s.source();
    }

    let full = format!("[{}] {}", kind, e);
    if detail.is_empty() {
        subhuti_core::Error::Any(anyhow::anyhow!("LLM HTTP {}: {}", kind, full))
    } else {
        subhuti_core::Error::Any(anyhow::anyhow!("LLM HTTP {}: {}{}", kind, full, detail))
    }
}

/// 对成功发送但返回非 2xx 的响应，提取 status + 响应体片段构成清晰错误。
/// 用于 OpenAI 兼容接口（智谱走 parse_zhipu_json 已有同样逻辑）。
async fn ensure_http_success(
    response: reqwest::Response,
) -> subhuti_core::Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    // 只截取响应体前 2000 字符，避免超长 body 淹没错误信息
    let body_text = response.text().await.unwrap_or_default();
    let snippet: String = body_text.chars().take(2000).collect();
    let hint = if snippet.is_empty() {
        "（无响应体）".to_string()
    } else {
        snippet
    };
    Err(subhuti_core::Error::Any(anyhow::anyhow!(
        "LLM HTTP {} (status): {}",
        status.as_u16(),
        hint
    )))
}

/// 智谱 API 返回的标准错误体
#[derive(Debug, Deserialize)]
struct ZhipuApiErrorBody {
    error: ZhipuApiErrorInner,
}
#[derive(Debug, Deserialize)]
struct ZhipuApiErrorInner {
    code: String,
    message: String,
}

/// 对非流式响应做统一的 HTTP status + 错误体检查，解析成友好的错误
async fn parse_zhipu_json<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> subhuti_core::Result<T> {
    let status = response.status();
    if !status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        let hint = serde_json::from_str::<ZhipuApiErrorBody>(&body_text)
            .map(|e| format!("[code={}] {}", e.error.code, e.error.message))
            .unwrap_or_else(|_| body_text.clone());
        return Err(subhuti_core::Error::Any(anyhow::anyhow!(
            "Zhipu API HTTP {}: {}",
            status.as_u16(),
            hint
        )));
    }
    response.json::<T>().await.map_err(|e| map_reqwest_error(e))
}

pub struct MockLLM {
    config: LLMConfig,
    responses: Mutex<Vec<String>>,
    tool_call_responses: Mutex<Vec<ToolCall>>,
    captured_messages: Mutex<Vec<Vec<Message>>>,
    call_count: Mutex<usize>,
    default_echo: bool,
}

impl MockLLM {
    pub fn new() -> Self {
        Self {
            config: LLMConfig::default(),
            responses: Mutex::new(Vec::new()),
            tool_call_responses: Mutex::new(Vec::new()),
            captured_messages: Mutex::new(Vec::new()),
            call_count: Mutex::new(0),
            default_echo: false,
        }
    }

    pub fn with_response(response: &str) -> Self {
        let mock = Self::new();
        mock.add_response(response);
        mock
    }

    pub fn with_echo() -> Self {
        let mut mock = Self::new();
        mock.default_echo = true;
        mock
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// 复用现有配置，只切换模型名（测试用）
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        let mut model_name = model.into();
        if model_name.trim().is_empty() {
            model_name = "mock-model".to_string();
        }
        let mut cfg = self.config.clone();
        cfg.model = model_name;
        Self {
            config: cfg,
            // 拷贝现有响应队列、工具调用响应、历史消息和调用次数（保证 swap_model 后还能继续用之前 mock）
            responses: Mutex::new(self.responses.lock().unwrap().clone()),
            tool_call_responses: Mutex::new(self.tool_call_responses.lock().unwrap().clone()),
            captured_messages: Mutex::new(self.captured_messages.lock().unwrap().clone()),
            call_count: Mutex::new(*self.call_count.lock().unwrap()),
            default_echo: self.default_echo,
        }
    }

    pub fn add_response(&self, response: &str) {
        self.responses.lock().unwrap().push(response.to_string());
    }

    pub fn add_responses(&self, responses: Vec<&str>) {
        let mut guard = self.responses.lock().unwrap();
        for r in responses {
            guard.push(r.to_string());
        }
    }

    pub fn add_tool_call_response(&self, tool_call: ToolCall) {
        self.tool_call_responses.lock().unwrap().push(tool_call);
    }

    pub fn get_call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    pub fn get_captured_messages(&self) -> Vec<Vec<Message>> {
        self.captured_messages.lock().unwrap().clone()
    }

    pub fn get_last_messages(&self) -> Option<Vec<Message>> {
        let messages = self.captured_messages.lock().unwrap();
        messages.last().cloned()
    }

    fn capture_messages(&self, messages: &[Message]) {
        self.captured_messages
            .lock()
            .unwrap()
            .push(messages.to_vec());
        *self.call_count.lock().unwrap() += 1;
    }

    fn get_next_response(&self, messages: &[Message]) -> String {
        let mut responses = self.responses.lock().unwrap();
        if !responses.is_empty() {
            responses.remove(0)
        } else if self.default_echo {
            messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default()
        } else {
            "[MockLLM] 模拟响应".to_string()
        }
    }

    fn get_next_tool_call(&self) -> Option<ToolCall> {
        let mut tool_calls = self.tool_call_responses.lock().unwrap();
        if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls.remove(0))
        }
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

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        debug!("[MockLLM] 收到消息: {:?}", messages);
        self.capture_messages(&messages);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        Ok(self.get_next_response(&messages))
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        _tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        debug!("[MockLLM] 收到消息（带工具）: {:?}", messages);
        self.capture_messages(&messages);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        if let Some(tool_call) = self.get_next_tool_call() {
            Ok(LLMResponse {
                content: "[MockLLM] 模拟工具调用响应".to_string(),
                tool_call: Some(tool_call),
                model: Some("mock".to_string()),
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            })
        } else {
            Ok(LLMResponse {
                content: "[MockLLM] 模拟工具调用响应".to_string(),
                tool_call: None,
                model: Some("mock".to_string()),
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
            })
        }
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        debug!("[MockLLM] 流式响应: {:?}", messages);
        self.capture_messages(&messages);

        let response = self.get_next_response(&messages);
        let chunks: Vec<&str> = response.split_whitespace().collect();

        for chunk in chunks {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            callback(chunk.to_string());
        }
        Ok(())
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        Ok(true)
    }
}

pub type MockLlmClient = MockLLM;

pub struct OpenAIConfig {
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: "".to_string(),
            api_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4".to_string(),
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

pub struct OpenAIClient {
    config: OpenAIConfig,
    llm_config: LLMConfig,
    http_client: Client,
}

impl OpenAIClient {
    pub fn new(config: OpenAIConfig) -> Self {
        let llm_config = LLMConfig {
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            api_key: Some(config.api_key.clone()),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        };
        Self {
            config,
            llm_config,
            http_client: Client::new(),
        }
    }

    pub fn arc(config: OpenAIConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// 复用现有配置，只切换模型名
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        let mut cfg = OpenAIConfig {
            api_key: self.config.api_key.clone(),
            api_url: self.config.api_url.clone(),
            model: model.into(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };
        if cfg.model.trim().is_empty() {
            cfg.model = "gpt-4o-mini".to_string();
        }
        Self::new(cfg)
    }
}

#[async_trait]
impl LLM for OpenAIClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::OpenAI
    }

    fn config(&self) -> &LLMConfig {
        &self.llm_config
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = OpenAICompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| OpenAIMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: false,
            tools: None,
        };

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let response = ensure_http_success(response).await?;

        let result: OpenAICompletionResponse = response.json().await.map_err(map_reqwest_error)?;
        Ok(result.choices[0]
            .message
            .content
            .clone()
            .unwrap_or_default())
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = OpenAICompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| OpenAIMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: false,
            tools: Some(tools),
        };

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let response = ensure_http_success(response).await?;

        let result: OpenAICompletionResponse = response.json().await.map_err(map_reqwest_error)?;
        let message = &result.choices[0].message;

        Ok(LLMResponse {
            content: message.content.clone().unwrap_or_default(),
            tool_call: message.tool_calls.as_ref().and_then(|calls| {
                calls.first().map(|c| ToolCall {
                    id: c.id.clone(),
                    name: c.function.name.clone(),
                    arguments: c.function.arguments.clone(),
                })
            }),
            model: Some(result.model),
            prompt_tokens: result.usage.as_ref().map(|u| u.prompt_tokens),
            completion_tokens: result.usage.as_ref().map(|u| u.completion_tokens),
            total_tokens: result.usage.as_ref().map(|u| u.total_tokens),
        })
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = OpenAICompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| OpenAIMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: true,
            tools: None,
        };

        let mut response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if line.starts_with("data: ") {
                    let data = line.strip_prefix("data: ").unwrap_or(line);
                    if data == "[DONE]" {
                        return Ok(());
                    }
                    if let Ok(event) = serde_json::from_str::<OpenAIStreamEvent>(data) {
                        if let Some(content) = event.choices[0].delta.content.clone() {
                            callback(content);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        let url = format!("{}/models", self.config.api_url);
        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        Ok(response.status().is_success())
    }
}

#[derive(Debug, Serialize)]
struct OpenAICompletionRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    temperature: f32,
    max_tokens: usize,
    stream: bool,
    tools: Option<Vec<ToolInfo>>,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAICompletionResponse {
    id: String,
    model: String,
    choices: Vec<OpenAICompletionChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAICompletionChoice {
    message: OpenAICompletionMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAICompletionMessage {
    role: String,
    /// 可能为 null/缺失（模型只返回 tool_calls 时不带 content）
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    id: String,
    function: OpenAIToolFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamEvent {
    choices: Vec<OpenAIStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
}

pub struct OllamaConfig {
    pub api_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:11434/api".to_string(),
            model: "llama3".to_string(),
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

pub struct OllamaClient {
    config: OllamaConfig,
    llm_config: LLMConfig,
    http_client: Client,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        let llm_config = LLMConfig {
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            api_key: None,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        };
        Self {
            config,
            llm_config,
            http_client: Client::new(),
        }
    }

    pub fn arc(config: OllamaConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// 复用现有配置，只切换模型名
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        let mut cfg = OllamaConfig {
            api_url: self.config.api_url.clone(),
            model: model.into(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };
        if cfg.model.trim().is_empty() {
            cfg.model = "llama3".to_string();
        }
        Self::new(cfg)
    }
}

#[async_trait]
impl LLM for OllamaClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Ollama
    }

    fn config(&self) -> &LLMConfig {
        &self.llm_config
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        let url = format!("{}/chat", self.config.api_url);
        let request_body = OllamaChatRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| OllamaMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            stream: false,
        };

        let response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let result: OllamaChatResponse = response.json().await.map_err(map_reqwest_error)?;
        Ok(result.message.content)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        _tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        self.chat(messages).await.map(|content| LLMResponse {
            content,
            tool_call: None,
            model: Some(self.config.model.clone()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        })
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        let url = format!("{}/chat", self.config.api_url);
        let request_body = OllamaChatRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| OllamaMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            stream: true,
        };

        let mut response = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if let Ok(event) = serde_json::from_str::<OllamaStreamEvent>(line) {
                    callback(event.message.content);
                    if event.done {
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        let url = format!("{}/tags", self.config.api_url);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        Ok(response.status().is_success())
    }
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OllamaChatResponse {
    model: String,
    message: OllamaMessage,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamEvent {
    message: OllamaMessage,
    done: bool,
}

pub struct DoubaoConfig {
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for DoubaoConfig {
    fn default() -> Self {
        Self {
            api_key: "".to_string(),
            api_url: "https://lf-api.bytedance.net/api/open/v1/chat".to_string(),
            model: "doubao-pro".to_string(),
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

pub struct DoubaoClient {
    config: DoubaoConfig,
    llm_config: LLMConfig,
    http_client: Client,
}

impl DoubaoClient {
    pub fn new(config: DoubaoConfig) -> Self {
        let llm_config = LLMConfig {
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            api_key: Some(config.api_key.clone()),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        };
        Self {
            config,
            llm_config,
            http_client: Client::new(),
        }
    }

    pub fn arc(config: DoubaoConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// 复用现有配置，只切换模型名
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        let mut cfg = DoubaoConfig {
            api_key: self.config.api_key.clone(),
            api_url: self.config.api_url.clone(),
            model: model.into(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };
        if cfg.model.trim().is_empty() {
            cfg.model = "doubao-pro".to_string();
        }
        Self::new(cfg)
    }
}

#[async_trait]
impl LLM for DoubaoClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Doubao
    }

    fn config(&self) -> &LLMConfig {
        &self.llm_config
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        let url = &self.config.api_url;
        let request_body = DoubaoChatRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| DoubaoMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        let response = self
            .http_client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let result: DoubaoChatResponse = response.json().await.map_err(map_reqwest_error)?;
        Ok(result.result.content)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        _tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        self.chat(messages).await.map(|content| LLMResponse {
            content,
            tool_call: None,
            model: Some(self.config.model.clone()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        })
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        let url = &self.config.api_url;
        let request_body = DoubaoChatRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| DoubaoMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        let mut response = self
            .http_client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if let Ok(event) = serde_json::from_str::<DoubaoStreamEvent>(line) {
                    callback(event.content);
                    if event.is_finish {
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        let url = format!("{}/health", self.config.api_url);
        let response = self
            .http_client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .send()
            .await
            .map_err(map_reqwest_error)?;
        Ok(response.status().is_success())
    }
}

#[derive(Debug, Serialize)]
struct DoubaoChatRequest {
    model: String,
    messages: Vec<DoubaoMessage>,
    temperature: f32,
    max_tokens: usize,
}

#[derive(Debug, Serialize)]
struct DoubaoMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct DoubaoChatResponse {
    result: DoubaoResult,
}

#[derive(Debug, Deserialize)]
struct DoubaoResult {
    content: String,
}

#[derive(Debug, Deserialize)]
struct DoubaoStreamEvent {
    content: String,
    is_finish: bool,
}

// ============================================================
// Zhipu (智谱 AI / GLM 系列) —— OpenAI 兼容协议
// 官方文档：https://open.bigmodel.cn/dev/api/normal-model/glm-4
// ============================================================

pub struct ZhipuConfig {
    pub api_key: String,
    pub api_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl Default for ZhipuConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
            // 默认标准版：QPS 宽松、秒回、不限流
            // 需要深度思考时用 with_model("glm-4.7-flash") 切换
            model: "glm-4-flash".to_string(),
            temperature: 0.7,
            max_tokens: 2048,
        }
    }
}

pub struct ZhipuClient {
    config: ZhipuConfig,
    llm_config: LLMConfig,
    http_client: Client,
}

impl ZhipuClient {
    pub fn new(config: ZhipuConfig) -> Self {
        let llm_config = LLMConfig {
            model: config.model.clone(),
            api_url: config.api_url.clone(),
            api_key: Some(config.api_key.clone()),
            temperature: config.temperature,
            max_tokens: config.max_tokens,
        };
        Self {
            config,
            llm_config,
            http_client: Client::new(),
        }
    }

    pub fn arc(config: ZhipuConfig) -> Arc<Self> {
        Arc::new(Self::new(config))
    }

    /// 复用现有配置（api_key / api_url / temperature / max_tokens），只切换模型名。
    /// 典型用法：临时切到 glm-4.7-flash 做深度思考，用完切回默认 glm-4-flash
    pub fn with_model(&self, model: impl Into<String>) -> Self {
        let mut cfg = ZhipuConfig {
            api_key: self.config.api_key.clone(),
            api_url: self.config.api_url.clone(),
            model: model.into(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };
        // 如果用户传了空字符串，回退到默认模型
        if cfg.model.trim().is_empty() {
            cfg.model = "glm-4-flash".to_string();
        }
        Self::new(cfg)
    }
}

#[async_trait]
impl LLM for ZhipuClient {
    fn provider(&self) -> LLMProvider {
        LLMProvider::Zhipu
    }

    fn config(&self) -> &LLMConfig {
        &self.llm_config
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = ZhipuCompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| ZhipuMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: false,
            tools: None,
        };

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let result: ZhipuCompletionResponse = parse_zhipu_json(response).await?;
        Ok(result.choices[0]
            .message
            .content
            .clone()
            .unwrap_or_default())
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = ZhipuCompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| ZhipuMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: false,
            tools: Some(tools),
        };

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let result: ZhipuCompletionResponse = parse_zhipu_json(response).await?;
        let message = &result.choices[0].message;

        Ok(LLMResponse {
            content: message.content.clone().unwrap_or_default(),
            tool_call: message.tool_calls.as_ref().and_then(|calls| {
                calls.first().map(|c| ToolCall {
                    id: c.id.clone(),
                    name: c.function.name.clone(),
                    arguments: c.function.arguments.clone(),
                })
            }),
            model: Some(result.model),
            prompt_tokens: result.usage.as_ref().map(|u| u.prompt_tokens),
            completion_tokens: result.usage.as_ref().map(|u| u.completion_tokens),
            total_tokens: result.usage.as_ref().map(|u| u.total_tokens),
        })
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        let url = format!("{}/chat/completions", self.config.api_url);
        let request_body = ZhipuCompletionRequest {
            model: self.config.model.clone(),
            messages: messages
                .into_iter()
                .map(|m| ZhipuMessage {
                    role: match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    }
                    .to_string(),
                    content: m.content,
                })
                .collect(),
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
            stream: true,
            tools: None,
        };

        let mut response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;

        if !response.status().is_success() {
            let status_code = response.status().as_u16();
            let body_text = response.text().await.unwrap_or_default();
            let hint = serde_json::from_str::<ZhipuApiErrorBody>(&body_text)
                .map(|e| format!("[code={}] {}", e.error.code, e.error.message))
                .unwrap_or_else(|_| body_text.clone());
            return Err(subhuti_core::Error::Any(anyhow::anyhow!(
                "Zhipu API HTTP {} (streaming): {}",
                status_code,
                hint
            )));
        }

        while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
            let text = String::from_utf8_lossy(&chunk);
            for line in text.lines() {
                if line.starts_with("data: ") {
                    let data = line.strip_prefix("data: ").unwrap_or(line);
                    if data == "[DONE]" {
                        return Ok(());
                    }
                    if let Ok(event) = serde_json::from_str::<ZhipuStreamEvent>(data) {
                        if let Some(content) = event.choices[0].delta.content.clone() {
                            callback(content);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        // 注意：智谱对 `/models` 接口权限控制较严（会返回 401），改用一次最小 chat 请求做健康检查
        let url = format!("{}/chat/completions", self.config.api_url);
        let ping_body = serde_json::json!({
            "model": self.config.model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
            "temperature": 0.0,
        });
        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&ping_body)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        Ok(response.status().is_success())
    }
}

#[derive(Debug, Serialize)]
struct ZhipuCompletionRequest {
    model: String,
    messages: Vec<ZhipuMessage>,
    temperature: f32,
    max_tokens: usize,
    stream: bool,
    tools: Option<Vec<ToolInfo>>,
}

#[derive(Debug, Serialize)]
struct ZhipuMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ZhipuCompletionResponse {
    id: String,
    model: String,
    choices: Vec<ZhipuCompletionChoice>,
    usage: Option<ZhipuUsage>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ZhipuCompletionChoice {
    message: ZhipuCompletionMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ZhipuCompletionMessage {
    role: String,
    /// 可能为 null/缺失（模型只返回 tool_calls 时不带 content）
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ZhipuToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ZhipuToolCall {
    id: String,
    function: ZhipuToolFunction,
}

#[derive(Debug, Deserialize)]
struct ZhipuToolFunction {
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ZhipuUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ZhipuStreamEvent {
    choices: Vec<ZhipuStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct ZhipuStreamChoice {
    delta: ZhipuStreamDelta,
}

#[derive(Debug, Deserialize)]
struct ZhipuStreamDelta {
    content: Option<String>,
}
