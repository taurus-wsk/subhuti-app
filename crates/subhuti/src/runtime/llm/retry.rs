//! # LLM Retry Mechanism - LLM 重试机制
//!
//! 提供 LLM 调用的可靠性保障：
//! - **自动重试**：失败后自动重试（最多 3 次）
//! - **指数退避**：重试间隔逐渐增加（1s → 2s → 4s）
//! - **Fallback 降级**：最终失败后返回友好提示或调用本地模型
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use subhuti::runtime::llm::{LLM, RetryConfig, chat_with_retry};
//!
//! let client = DoubaoClient::new(config);
//! let retry_config = RetryConfig::default();
//!
//! // 带重试的调用
//! let response = chat_with_retry(&client, messages, &retry_config).await?;
//! ```

use crate::runtime::llm::{Message, LLM};
use anyhow::Result;
use std::time::Duration;
use tokio::time::sleep;

/// 重试配置
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// 最大重试次数
    pub max_retries: u32,
    /// 初始延迟（毫秒）
    pub initial_delay_ms: u64,
    /// 是否启用 Fallback
    pub enable_fallback: bool,
    /// Fallback 响应文本
    pub fallback_message: String,
    /// 是否使用指数退避
    pub exponential_backoff: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            enable_fallback: true,
            fallback_message: "抱歉，AI 服务暂时不可用，请稍后再试。".to_string(),
            exponential_backoff: true,
        }
    }
}

impl RetryConfig {
    /// 创建保守配置（少重试）
    pub fn conservative() -> Self {
        Self {
            max_retries: 1,
            initial_delay_ms: 500,
            enable_fallback: true,
            fallback_message: "服务暂时不可用。".to_string(),
            exponential_backoff: false,
        }
    }

    /// 创建激进配置（多重试）
    pub fn aggressive() -> Self {
        Self {
            max_retries: 5,
            initial_delay_ms: 500,
            enable_fallback: true,
            fallback_message: "抱歉，多次尝试后仍无法连接 AI 服务。".to_string(),
            exponential_backoff: true,
        }
    }

    /// 禁用 Fallback
    pub fn no_fallback() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            enable_fallback: false,
            fallback_message: String::new(),
            exponential_backoff: true,
        }
    }
}

/// 重试结果
#[derive(Debug)]
pub struct RetryResult {
    /// 最终响应
    pub response: Option<String>,
    /// 是否成功
    pub success: bool,
    /// 重试次数
    pub retry_count: u32,
    /// 最后的错误
    pub last_error: Option<String>,
    /// 是否使用了 Fallback
    pub used_fallback: bool,
}

impl RetryResult {
    /// 成功结果
    pub fn success(response: String, retry_count: u32) -> Self {
        Self {
            response: Some(response),
            success: true,
            retry_count,
            last_error: None,
            used_fallback: false,
        }
    }

    /// Fallback 结果
    pub fn fallback(message: String, retry_count: u32, last_error: String) -> Self {
        Self {
            response: Some(message),
            success: false,
            retry_count,
            last_error: Some(last_error),
            used_fallback: true,
        }
    }

    /// 完全失败
    pub fn failed(retry_count: u32, error: String) -> Self {
        Self {
            response: None,
            success: false,
            retry_count,
            last_error: Some(error),
            used_fallback: false,
        }
    }
}

/// 带重试的 chat 调用
pub async fn chat_with_retry(
    client: &dyn LLM,
    messages: Vec<Message>,
    config: &RetryConfig,
) -> Result<RetryResult> {
    let mut retry_count = 0u32;
    let mut last_error = String::new();

    while retry_count <= config.max_retries {
        // 尝试调用
        let result = client.chat(messages.clone()).await;

        match result {
            Ok(response) => {
                if retry_count > 0 {
                    tracing::info!("LLM call succeeded after {} retries", retry_count);
                }
                return Ok(RetryResult::success(response, retry_count));
            }
            Err(e) => {
                last_error = e.to_string();
                retry_count += 1;

                if retry_count <= config.max_retries {
                    // 计算延迟
                    let delay_ms = if config.exponential_backoff {
                        config.initial_delay_ms * (2_u64.pow(retry_count - 1))
                    } else {
                        config.initial_delay_ms
                    };

                    tracing::warn!(
                        "LLM call failed (attempt {}), retrying in {}ms: {}",
                        retry_count,
                        delay_ms,
                        last_error
                    );

                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    // 所有重试都失败
    tracing::error!(
        "LLM call failed after {} retries: {}",
        retry_count,
        last_error
    );

    if config.enable_fallback {
        Ok(RetryResult::fallback(
            config.fallback_message.clone(),
            retry_count,
            last_error,
        ))
    } else {
        Err(anyhow::anyhow!(
            "LLM call failed after {} retries: {}",
            retry_count,
            last_error
        ))
    }
}

/// 带重试的 chat 调用（简化版，直接返回响应）
pub async fn with_retry(
    client: &dyn LLM,
    messages: Vec<Message>,
    config: &RetryConfig,
) -> Result<String> {
    let result = chat_with_retry(client, messages, config).await?;

    match result.response {
        Some(response) => Ok(response),
        None => Err(anyhow::anyhow!(
            "LLM call failed: {}",
            result.last_error.unwrap_or_default()
        )),
    }
}

/// 带重试的流式 chat 调用
pub async fn chat_stream_with_retry<F>(
    client: &dyn LLM,
    messages: Vec<Message>,
    config: &RetryConfig,
    callback: F,
) -> Result<RetryResult>
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    let mut retry_count = 0u32;
    let mut last_error = String::new();

    while retry_count <= config.max_retries {
        let cb = callback.clone();
        let result = client.chat_streaming(messages.clone(), Box::new(cb)).await;

        match result {
            Ok(()) => {
                if retry_count > 0 {
                    tracing::info!("LLM stream succeeded after {} retries", retry_count);
                }
                // 流式调用成功返回空字符串（内容已通过callback输出）
                return Ok(RetryResult::success(String::new(), retry_count));
            }
            Err(e) => {
                last_error = e.to_string();
                retry_count += 1;

                if retry_count <= config.max_retries {
                    let delay_ms = if config.exponential_backoff {
                        config.initial_delay_ms * (2_u64.pow(retry_count - 1))
                    } else {
                        config.initial_delay_ms
                    };

                    tracing::warn!(
                        "LLM stream failed (attempt {}), retrying in {}ms: {}",
                        retry_count,
                        delay_ms,
                        last_error
                    );

                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    tracing::error!(
        "LLM stream failed after {} retries: {}",
        retry_count,
        last_error
    );

    if config.enable_fallback {
        // 流式输出 fallback 消息
        callback(config.fallback_message.clone());
        Ok(RetryResult::fallback(
            config.fallback_message.clone(),
            retry_count,
            last_error,
        ))
    } else {
        Err(anyhow::anyhow!(
            "LLM stream failed after {} retries: {}",
            retry_count,
            last_error
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_retries, 3);
        assert!(config.enable_fallback);
    }

    #[test]
    fn test_retry_config_presets() {
        let conservative = RetryConfig::conservative();
        assert_eq!(conservative.max_retries, 1);

        let aggressive = RetryConfig::aggressive();
        assert_eq!(aggressive.max_retries, 5);

        let no_fallback = RetryConfig::no_fallback();
        assert!(!no_fallback.enable_fallback);
    }

    #[test]
    fn test_retry_result() {
        let success = RetryResult::success("response".to_string(), 0);
        assert!(success.success);
        assert!(!success.used_fallback);

        let fallback = RetryResult::fallback("fallback".to_string(), 3, "error".to_string());
        assert!(!fallback.success);
        assert!(fallback.used_fallback);

        let failed = RetryResult::failed(3, "error".to_string());
        assert!(!failed.success);
        assert!(!failed.used_fallback);
    }
}
