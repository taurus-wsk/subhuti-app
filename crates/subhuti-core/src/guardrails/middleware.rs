use async_trait::async_trait;
use std::sync::Arc;

use super::interface::{GuardrailResult, IGuardrail, ToolPermission};

pub struct GuardrailMiddleware {
    guardrail: Arc<dyn IGuardrail>,
}

impl GuardrailMiddleware {
    pub fn new(guardrail: Arc<dyn IGuardrail>) -> Self {
        Self { guardrail }
    }

    pub async fn check_input(&self, input: &str) -> GuardrailResult {
        self.guardrail.check_input(input).await
    }

    pub async fn check_output(&self, output: &str) -> GuardrailResult {
        self.guardrail.check_output(output).await
    }

    pub async fn check_tool_call(
        &self,
        tool_name: &str,
        arguments: &str,
        permission: &ToolPermission,
    ) -> GuardrailResult {
        self.guardrail
            .check_tool_call(tool_name, arguments, permission)
            .await
    }

    pub async fn check_permission(
        &self,
        actor_id: &str,
        target_resource: &str,
        action: &str,
    ) -> GuardrailResult {
        self.guardrail
            .check_permission(actor_id, target_resource, action)
            .await
    }
}

#[async_trait]
pub trait GuardrailMiddlewareExt: Send + Sync {
    async fn with_guardrail<T, F>(
        &self,
        guardrail: &GuardrailMiddleware,
        input: &str,
        f: F,
    ) -> Result<T, GuardrailError>
    where
        F: FnOnce(&str) -> T + Send;

    async fn with_tool_guardrail<T, F>(
        &self,
        guardrail: &GuardrailMiddleware,
        tool_name: &str,
        arguments: &str,
        permission: &ToolPermission,
        f: F,
    ) -> Result<T, GuardrailError>
    where
        F: FnOnce() -> T + Send;
}

#[derive(Debug)]
pub enum GuardrailError {
    Denied(String),
    RequiresApproval(String),
    Sanitized(String),
}

impl std::fmt::Display for GuardrailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardrailError::Denied(reason) => write!(f, "护栏拒绝: {}", reason),
            GuardrailError::RequiresApproval(reason) => write!(f, "需要人工审批: {}", reason),
            GuardrailError::Sanitized(reason) => write!(f, "内容已脱敏: {}", reason),
        }
    }
}

impl std::error::Error for GuardrailError {}

pub struct GuardedExecutor<T> {
    inner: T,
    middleware: GuardrailMiddleware,
}

impl<T> GuardedExecutor<T> {
    pub fn new(inner: T, middleware: GuardrailMiddleware) -> Self {
        Self { inner, middleware }
    }

    pub async fn execute<F, R>(&self, input: &str, f: F) -> Result<R, GuardrailError>
    where
        F: FnOnce(&str, &T) -> R + Send,
    {
        let result = self.middleware.check_input(input).await;

        if !result.allowed {
            if result.required_approval {
                return Err(GuardrailError::RequiresApproval(result.reason));
            }
            return Err(GuardrailError::Denied(result.reason));
        }

        let processed_input = result.sanitized_input.as_deref().unwrap_or(input);
        Ok(f(processed_input, &self.inner))
    }
}

#[cfg(test)]
mod tests {
    use super::super::interface::{GuardrailConfig, NoopGuardrail};
    use super::*;

    #[tokio::test]
    async fn test_guardrail_middleware() {
        let config = GuardrailConfig::default();
        let guardrail = Arc::new(NoopGuardrail::new(config));
        let middleware = GuardrailMiddleware::new(guardrail);

        let result = middleware.check_input("test input").await;
        assert!(result.allowed);

        let result = middleware.check_output("test output").await;
        assert!(result.allowed);

        let permission = ToolPermission {
            tool_name: "test_tool".to_string(),
            permission: crate::guardrails::PermissionLevel::ReadOnly,
            requires_approval: false,
            allowed_conditions: Vec::new(),
        };

        let result = middleware
            .check_tool_call("test_tool", "{}", &permission)
            .await;
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn test_guarded_executor() {
        let config = GuardrailConfig::default();
        let guardrail = Arc::new(NoopGuardrail::new(config));
        let middleware = GuardrailMiddleware::new(guardrail);
        let executor = GuardedExecutor::new("inner_value", middleware);

        let result = executor
            .execute("test", |input, inner| {
                format!("processed: {} with: {}", input, inner)
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "processed: test with: inner_value");
    }
}
