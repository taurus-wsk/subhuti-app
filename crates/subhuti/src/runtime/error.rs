//! # Error Handling - 统一错误处理模块
//!
//! 提供统一的错误类型、错误分类、错误码和错误处理策略。
//!
//! ## 错误分类
//!
//! - **BusinessError**: 业务逻辑错误（参数校验、权限不足等）
//! - **SystemError**: 系统错误（数据库连接、配置错误等）
//! - **NetworkError**: 网络错误（超时、连接失败等）
//! - **LLMError**: LLM 相关错误（模型不可用、API 调用失败等）
//! - **ToolError**: 工具执行错误（工具不存在、参数错误等）

use anyhow::Error;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::time::SystemTime;

// ─── 错误码定义 ──────────────────────────────────────────────

pub type ErrorCode = u16;

/// 业务错误码 (1000-1999)
pub mod business_codes {
    pub const PARAM_INVALID: super::ErrorCode = 1001;
    pub const PERMISSION_DENIED: super::ErrorCode = 1002;
    pub const RESOURCE_NOT_FOUND: super::ErrorCode = 1003;
    pub const DUPLICATE_RESOURCE: super::ErrorCode = 1004;
    pub const VALIDATION_FAILED: super::ErrorCode = 1005;
    pub const RATE_LIMIT_EXCEEDED: super::ErrorCode = 1006;
}

/// 系统错误码 (2000-2999)
pub mod system_codes {
    pub const CONFIG_ERROR: super::ErrorCode = 2001;
    pub const DATABASE_ERROR: super::ErrorCode = 2002;
    pub const MEMORY_ERROR: super::ErrorCode = 2003;
    pub const RUNTIME_ERROR: super::ErrorCode = 2004;
    pub const DEADLOCK: super::ErrorCode = 2005;
}

/// 网络错误码 (3000-3999)
pub mod network_codes {
    pub const CONNECTION_TIMEOUT: super::ErrorCode = 3001;
    pub const CONNECTION_REFUSED: super::ErrorCode = 3002;
    pub const DNS_RESOLVE_FAILED: super::ErrorCode = 3003;
    pub const TLS_ERROR: super::ErrorCode = 3004;
    pub const NETWORK_UNREACHABLE: super::ErrorCode = 3005;
}

/// LLM 错误码 (4000-4999)
pub mod llm_codes {
    pub const CLIENT_NOT_CONFIGURED: super::ErrorCode = 4001;
    pub const API_CALL_FAILED: super::ErrorCode = 4002;
    pub const MODEL_NOT_AVAILABLE: super::ErrorCode = 4003;
    pub const RATE_LIMIT: super::ErrorCode = 4004;
    pub const AUTHENTICATION_FAILED: super::ErrorCode = 4005;
    pub const MAX_RETRIES_EXCEEDED: super::ErrorCode = 4006;
    pub const INVALID_RESPONSE: super::ErrorCode = 4007;
}

/// 工具错误码 (5000-5999)
pub mod tool_codes {
    pub const TOOL_NOT_FOUND: super::ErrorCode = 5001;
    pub const PARAMETER_ERROR: super::ErrorCode = 5002;
    pub const EXECUTION_FAILED: super::ErrorCode = 5003;
    pub const TIMEOUT: super::ErrorCode = 5004;
}

// ─── 错误分类 ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    Business,
    System,
    Network,
    LLM,
    Tool,
}

impl Display for ErrorCategory {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCategory::Business => write!(f, "business"),
            ErrorCategory::System => write!(f, "system"),
            ErrorCategory::Network => write!(f, "network"),
            ErrorCategory::LLM => write!(f, "llm"),
            ErrorCategory::Tool => write!(f, "tool"),
        }
    }
}

// ─── 统一错误类型 ──────────────────────────────────────────

#[derive(Debug)]
pub enum SubhutiError {
    // 业务错误
    Business {
        code: ErrorCode,
        message: String,
        details: Option<String>,
    },

    // 系统错误
    System {
        code: ErrorCode,
        message: String,
        source: Option<Error>,
    },

    // 网络错误
    Network {
        code: ErrorCode,
        message: String,
        retryable: bool,
        source: Option<Error>,
    },

    // LLM 错误
    LLM {
        code: ErrorCode,
        message: String,
        retryable: bool,
        source: Option<Error>,
    },

    // 工具错误
    Tool {
        code: ErrorCode,
        message: String,
        tool_name: Option<String>,
        source: Option<Error>,
    },
}

impl SubhutiError {
    /// 获取错误码
    pub fn code(&self) -> ErrorCode {
        match self {
            SubhutiError::Business { code, .. } => *code,
            SubhutiError::System { code, .. } => *code,
            SubhutiError::Network { code, .. } => *code,
            SubhutiError::LLM { code, .. } => *code,
            SubhutiError::Tool { code, .. } => *code,
        }
    }

    /// 获取错误分类
    pub fn category(&self) -> ErrorCategory {
        match self {
            SubhutiError::Business { .. } => ErrorCategory::Business,
            SubhutiError::System { .. } => ErrorCategory::System,
            SubhutiError::Network { .. } => ErrorCategory::Network,
            SubhutiError::LLM { .. } => ErrorCategory::LLM,
            SubhutiError::Tool { .. } => ErrorCategory::Tool,
        }
    }

    /// 是否可重试
    pub fn is_retryable(&self) -> bool {
        match self {
            SubhutiError::Network { retryable, .. } => *retryable,
            SubhutiError::LLM { retryable, .. } => *retryable,
            _ => false,
        }
    }

    /// 获取错误消息
    pub fn message(&self) -> &str {
        match self {
            SubhutiError::Business { message, .. } => message,
            SubhutiError::System { message, .. } => message,
            SubhutiError::Network { message, .. } => message,
            SubhutiError::LLM { message, .. } => message,
            SubhutiError::Tool { message, .. } => message,
        }
    }

    /// 获取源错误
    pub fn source(&self) -> Option<&Error> {
        match self {
            SubhutiError::System { source, .. } => source.as_ref(),
            SubhutiError::Network { source, .. } => source.as_ref(),
            SubhutiError::LLM { source, .. } => source.as_ref(),
            SubhutiError::Tool { source, .. } => source.as_ref(),
            _ => None,
        }
    }

    // ─── 便捷构造函数 ──────────────────────────────────────

    pub fn business(code: ErrorCode, message: impl Into<String>) -> Self {
        SubhutiError::Business {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn business_with_details(
        code: ErrorCode,
        message: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        SubhutiError::Business {
            code,
            message: message.into(),
            details: Some(details.into()),
        }
    }

    pub fn system(code: ErrorCode, message: impl Into<String>) -> Self {
        SubhutiError::System {
            code,
            message: message.into(),
            source: None,
        }
    }

    pub fn system_with_source(code: ErrorCode, message: impl Into<String>, source: Error) -> Self {
        SubhutiError::System {
            code,
            message: message.into(),
            source: Some(source),
        }
    }

    pub fn network(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        SubhutiError::Network {
            code,
            message: message.into(),
            retryable,
            source: None,
        }
    }

    pub fn network_with_source(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        source: Error,
    ) -> Self {
        SubhutiError::Network {
            code,
            message: message.into(),
            retryable,
            source: Some(source),
        }
    }

    pub fn llm(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        SubhutiError::LLM {
            code,
            message: message.into(),
            retryable,
            source: None,
        }
    }

    pub fn llm_with_source(
        code: ErrorCode,
        message: impl Into<String>,
        retryable: bool,
        source: Error,
    ) -> Self {
        SubhutiError::LLM {
            code,
            message: message.into(),
            retryable,
            source: Some(source),
        }
    }

    pub fn tool(
        code: ErrorCode,
        message: impl Into<String>,
        tool_name: impl Into<Option<String>>,
    ) -> Self {
        SubhutiError::Tool {
            code,
            message: message.into(),
            tool_name: tool_name.into(),
            source: None,
        }
    }

    pub fn tool_with_source(
        code: ErrorCode,
        message: impl Into<String>,
        tool_name: impl Into<Option<String>>,
        source: Error,
    ) -> Self {
        SubhutiError::Tool {
            code,
            message: message.into(),
            tool_name: tool_name.into(),
            source: Some(source),
        }
    }
}

impl Display for SubhutiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SubhutiError::Business {
                code,
                message,
                details,
            } => {
                if let Some(details) = details {
                    write!(f, "[BIZ-{:04}] {}: {}", code, message, details)
                } else {
                    write!(f, "[BIZ-{:04}] {}", code, message)
                }
            }
            SubhutiError::System {
                code,
                message,
                source,
            } => {
                if let Some(source) = source {
                    write!(f, "[SYS-{:04}] {}: {}", code, message, source)
                } else {
                    write!(f, "[SYS-{:04}] {}", code, message)
                }
            }
            SubhutiError::Network {
                code,
                message,
                retryable,
                source,
            } => {
                let retry_str = if *retryable { "(retryable)" } else { "" };
                if let Some(source) = source {
                    write!(f, "[NET-{:04}] {} {}: {}", code, retry_str, message, source)
                } else {
                    write!(f, "[NET-{:04}] {} {}", code, retry_str, message)
                }
            }
            SubhutiError::LLM {
                code,
                message,
                retryable,
                source,
            } => {
                let retry_str = if *retryable { "(retryable)" } else { "" };
                if let Some(source) = source {
                    write!(f, "[LLM-{:04}] {} {}: {}", code, retry_str, message, source)
                } else {
                    write!(f, "[LLM-{:04}] {} {}", code, retry_str, message)
                }
            }
            SubhutiError::Tool {
                code,
                message,
                tool_name,
                source,
            } => {
                let tool_str = tool_name
                    .as_ref()
                    .map_or_else(|| "".to_string(), |n| format!("(tool={})", n));
                if let Some(source) = source {
                    write!(f, "[TOOL-{:04}] {} {}: {}", code, tool_str, message, source)
                } else {
                    write!(f, "[TOOL-{:04}] {}", code, message)
                }
            }
        }
    }
}

impl std::error::Error for SubhutiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SubhutiError::System { source, .. } => source.as_ref().map(|e| e.as_ref()),
            SubhutiError::Network { source, .. } => source.as_ref().map(|e| e.as_ref()),
            SubhutiError::LLM { source, .. } => source.as_ref().map(|e| e.as_ref()),
            SubhutiError::Tool { source, .. } => source.as_ref().map(|e| e.as_ref()),
            _ => None,
        }
    }
}

impl SubhutiError {
    pub fn to_error(self) -> Error {
        Error::msg(self.to_string())
    }
}

// ─── 错误日志记录器 ────────────────────────────────────────

/// 错误日志记录器
#[derive(Debug)]
pub struct ErrorLogger;

impl ErrorLogger {
    /// 记录错误日志
    pub fn log_error(error: &SubhutiError, trace_id: Option<&str>) {
        match error.category() {
            ErrorCategory::Business => {
                tracing::warn!(
                    error = %error,
                    code = error.code(),
                    category = %error.category(),
                    trace_id = ?trace_id,
                    "业务错误"
                );
            }
            ErrorCategory::System => {
                tracing::error!(
                    error = %error,
                    code = error.code(),
                    category = %error.category(),
                    trace_id = ?trace_id,
                    "系统错误"
                );
            }
            ErrorCategory::Network => {
                tracing::error!(
                    error = %error,
                    code = error.code(),
                    category = %error.category(),
                    retryable = error.is_retryable(),
                    trace_id = ?trace_id,
                    "网络错误"
                );
            }
            ErrorCategory::LLM => {
                tracing::error!(
                    error = %error,
                    code = error.code(),
                    category = %error.category(),
                    retryable = error.is_retryable(),
                    trace_id = ?trace_id,
                    "LLM 错误"
                );
            }
            ErrorCategory::Tool => {
                tracing::error!(
                    error = %error,
                    code = error.code(),
                    category = %error.category(),
                    trace_id = ?trace_id,
                    "工具错误"
                );
            }
        }
    }

    /// 记录错误并返回
    pub fn log_and_return(error: SubhutiError, trace_id: Option<&str>) -> Error {
        Self::log_error(&error, trace_id);
        error.into()
    }
}

// ─── 错误处理策略 ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryStrategy {
    pub max_retries: usize,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_factor: f32,
}

impl Default for RetryStrategy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay_ms: 1000,
            max_delay_ms: 10000,
            backoff_factor: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: usize,
    pub success_threshold: usize,
    pub reset_timeout_ms: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            reset_timeout_ms: 30000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub enabled: bool,
    pub fallback_message: String,
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fallback_message: "服务暂时不可用，请稍后重试".to_string(),
        }
    }
}

/// 错误处理配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ErrorHandlingConfig {
    pub retry: RetryStrategy,
    pub circuit_breaker: CircuitBreakerConfig,
    pub fallback: FallbackConfig,
}

// ─── 错误响应 DTO ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub success: bool,
    pub error_code: ErrorCode,
    pub error_message: String,
    pub error_category: String,
    pub retryable: bool,
    pub trace_id: Option<String>,
    pub timestamp: u64,
}

impl ErrorResponse {
    pub fn from_error(error: &SubhutiError, trace_id: Option<&str>) -> Self {
        Self {
            success: false,
            error_code: error.code(),
            error_message: error.message().to_string(),
            error_category: error.category().to_string(),
            retryable: error.is_retryable(),
            trace_id: trace_id.map(|s| s.to_string()),
            timestamp: SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = SubhutiError::business(business_codes::PARAM_INVALID, "参数无效");
        assert_eq!(error.code(), business_codes::PARAM_INVALID);
        assert_eq!(error.category(), ErrorCategory::Business);
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_error_display() {
        let error = SubhutiError::llm(llm_codes::API_CALL_FAILED, "API 调用失败", true);
        assert!(error.to_string().contains("[LLM-4002]"));
        assert!(error.to_string().contains("(retryable)"));
    }

    #[test]
    fn test_error_from_anyhow() {
        let error: Error = SubhutiError::system(system_codes::CONFIG_ERROR, "配置错误").into();
        assert!(error.to_string().contains("[SYS-2001]"));
    }

    #[test]
    fn test_error_response() {
        let error = SubhutiError::network(network_codes::CONNECTION_TIMEOUT, "连接超时", true);
        let response = ErrorResponse::from_error(&error, Some("trace-123"));

        assert_eq!(response.error_code, network_codes::CONNECTION_TIMEOUT);
        assert_eq!(response.error_category, "network");
        assert!(response.retryable);
        assert_eq!(response.trace_id, Some("trace-123".to_string()));
    }
}
