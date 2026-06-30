//! # Runtime Layer - 运行层
//!
//! 职责：真正执行层，所有可运行能力、模型、工具、约束
//!
//! ## 包含模块
//!
//! - **LLM 抽象层**: 统一模型 Trait（OpenAI / Ollama / 任意兼容）
//! - **工具系统**: 极简 Tool Trait，name/desc/schema/run
//! - **约束护栏**: 代码级强制限制，最大工具调用轮次、超时等
//! - **Session**: 所有状态归 Session，Runtime 无状态

// 子模块
pub mod llm;
pub mod tools;

mod session;
pub use session::{Session, SessionConfig, SessionState};

mod constraints;
pub use constraints::Constraints;

// 从子模块导出
pub use llm::{
    LLMClient, LLMConfig, LLMProvider, LLMResponse, Message, MockLLM, Role, ToolInfo, LLM,
};
pub use tools::{Tool, ToolInfo as ToolInfoType, ToolResult};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// 运行时配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeConfig {
    /// 最大工具调用轮次
    pub max_turns: usize,
    /// 最大上下文长度 (token)
    pub max_context_tokens: usize,
    /// 超时时间 (秒)
    pub timeout_seconds: u64,
    /// 默认 temperature
    pub default_temperature: f32,
    /// 默认 max_tokens
    pub default_max_tokens: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_turns: 10,
            max_context_tokens: 8192,
            timeout_seconds: 60,
            default_temperature: 0.7,
            default_max_tokens: 2048,
        }
    }
}

/// 统一运行时
pub struct Runtime {
    config: RuntimeConfig,
    tools: Arc<RwLock<Vec<Arc<dyn tools::Tool>>>>,
    llm: Arc<RwLock<Option<Arc<dyn llm::LLM>>>>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("config", &self.config)
            .field("tool_count", &self.tools.read().unwrap().len())
            .field("has_llm", &self.llm.read().unwrap().is_some())
            .finish()
    }
}

impl Runtime {
    /// 创建新的 Runtime 实例
    pub fn new() -> Self {
        Self::with_config(RuntimeConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: RuntimeConfig) -> Self {
        Self {
            config,
            tools: Arc::new(RwLock::new(Vec::new())),
            llm: Arc::new(RwLock::new(None)),
        }
    }

    /// 使用配置和 LLM 创建（自动初始化 LLM 客户端）
    pub fn with_config_and_llm(
        config: RuntimeConfig,
        llm_config: &llm::LLMConfig,
        provider: llm::LLMProvider,
    ) -> Self {
        let runtime = Self::with_config(config);

        // 根据 provider 自动创建 LLM 客户端
        let client_result = match provider {
            llm::LLMProvider::OpenAI => Ok(llm::LLMClient::OpenAI(llm::OpenAIClient::new(
                llm_config.clone(),
            ))),
            llm::LLMProvider::Ollama => Ok(llm::LLMClient::Ollama(llm::OllamaClient::new(
                llm_config.clone(),
            ))),
            llm::LLMProvider::Doubao => {
                llm::DoubaoClient::new(llm_config.clone()).map(llm::LLMClient::Doubao)
            }
            llm::LLMProvider::Custom => {
                // Custom provider 尝试自动推断
                llm::LLMClient::from_config(llm_config.clone())
            }
        };

        if let Ok(client) = client_result {
            runtime.set_llm(client);
            tracing::info!(
                "LLM client initialized: provider={}",
                match provider {
                    llm::LLMProvider::OpenAI => "OpenAI",
                    llm::LLMProvider::Ollama => "Ollama",
                    llm::LLMProvider::Doubao => "Doubao",
                    llm::LLMProvider::Custom => "Custom",
                }
            );
        } else {
            tracing::warn!("Failed to initialize LLM client: {:?}", client_result.err());
        }

        runtime
    }

    /// 注册 LLM 客户端
    pub fn set_llm(&self, llm: llm::LLMClient) {
        // 将 LLMClient 转换为 Arc<dyn LLM>
        let arc_llm: Arc<dyn llm::LLM> = match llm {
            llm::LLMClient::OpenAI(c) => Arc::new(c),
            llm::LLMClient::Ollama(c) => Arc::new(c),
            llm::LLMClient::Doubao(c) => Arc::new(c),
            llm::LLMClient::Mock(c) => Arc::new(c),
        };
        *self.llm.write().unwrap() = Some(arc_llm);
    }

    /// 注入 Mock LLM（测试专用）
    ///
    /// ```rust,ignore
    /// let mock = MockLLM::with_response("这是模拟响应");
    /// runtime.set_mock_llm(mock);
    /// assert!(runtime.has_llm());
    /// ```
    pub fn set_mock_llm(&self, mock: llm::MockLLM) {
        *self.llm.write().unwrap() = Some(Arc::new(mock));
    }

    /// 检查是否有 LLM
    pub fn has_llm(&self) -> bool {
        self.llm.read().unwrap().is_some()
    }

    /// 调用 LLM
    pub async fn call_llm(&self, messages: Vec<llm::Message>) -> Result<String> {
        // 获取 Arc<dyn LLM>，然后释放锁
        let llm_arc = {
            let llm_guard = self.llm.read().unwrap();
            llm_guard.clone()
        };

        if let Some(llm) = llm_arc {
            llm.chat(messages).await
        } else {
            Err(anyhow::anyhow!("No LLM client configured"))
        }
    }

    /// 调用 LLM（返回完整响应，包含 Token 统计）
    pub async fn call_llm_with_stats(
        &self,
        messages: Vec<llm::Message>,
    ) -> Result<llm::LLMResponse> {
        // 获取 Arc<dyn LLM>，然后释放锁
        let llm_arc = {
            let llm_guard = self.llm.read().unwrap();
            llm_guard.clone()
        };

        if let Some(llm) = llm_arc {
            llm.chat_with_tools(messages, vec![]).await
        } else {
            Err(anyhow::anyhow!("No LLM client configured"))
        }
    }

    /// 调用 LLM（流式输出）
    ///
    /// callback: 每收到一块数据时调用，返回 true 继续，返回 false 停止
    pub async fn call_llm_streaming(
        &self,
        messages: Vec<llm::Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        // 获取 Arc<dyn LLM>，然后释放锁
        let llm_arc = {
            let llm_guard = self.llm.read().unwrap();
            llm_guard.clone()
        };

        if let Some(llm) = llm_arc {
            llm.chat_streaming(messages, callback).await
        } else {
            Err(anyhow::anyhow!("No LLM client configured"))
        }
    }

    /// 调用 LLM（支持工具调用）
    pub async fn call_llm_with_tools(
        &self,
        messages: Vec<llm::Message>,
    ) -> Result<llm::LLMResponse> {
        // 获取 Arc<dyn LLM>，然后释放锁
        let llm_arc = {
            let llm_guard = self.llm.read().unwrap();
            llm_guard.clone()
        };

        if let Some(llm) = llm_arc {
            // 获取工具信息并转换为 LLM 需要的格式
            let tools = self
                .get_tools()
                .into_iter()
                .map(|t| llm::ToolInfo {
                    tool_type: "function".to_string(),
                    function: llm::FunctionDefinition {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    },
                })
                .collect();

            llm.chat_with_tools(messages, tools).await
        } else {
            Err(anyhow::anyhow!("No LLM client configured"))
        }
    }

    /// 注册工具
    pub fn register_tool<T: tools::Tool + 'static>(&self, tool: T) {
        self.tools.write().unwrap().push(Arc::new(tool));
    }

    /// 获取所有工具
    pub fn get_tools(&self) -> Vec<tools::ToolInfo> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|t| t.info())
            .collect()
    }

    /// 执行工具
    pub async fn execute_tool(
        &self,
        name: &str,
        params: serde_json::Value,
    ) -> Result<tools::ToolResult> {
        // 获取工具的 Arc，然后释放锁
        let tool_arc = {
            let tools_guard = self.tools.read().unwrap();
            tools_guard.iter().find(|t| t.info().name == name).cloned()
        };

        if let Some(tool) = tool_arc {
            tool.run(params).await
        } else {
            Err(anyhow::anyhow!("Tool not found: {}", name))
        }
    }

    /// 获取配置
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_runtime_creation() {
        let runtime = Runtime::new();
        assert_eq!(runtime.config().max_turns, 10);
        assert!(!runtime.has_llm());
    }
}
