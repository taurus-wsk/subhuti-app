use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionLevel {
    ReadOnly,
    ReadWrite,
    HighRisk,
}

#[derive(Debug, Clone)]
pub struct ToolPermission {
    pub tool_name: String,
    pub permission: PermissionLevel,
    pub requires_approval: bool,
    pub allowed_conditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailConfig {
    pub enable_sensitive_data_protection: bool,
    pub enable_prompt_injection_protection: bool,
    pub enable_permission_check: bool,
    pub fail_closed: bool,
    pub max_token_limit: usize,
    pub max_tool_calls_per_step: usize,
}

impl Default for GuardrailConfig {
    fn default() -> Self {
        Self {
            enable_sensitive_data_protection: true,
            enable_prompt_injection_protection: true,
            enable_permission_check: true,
            fail_closed: true,
            max_token_limit: 4096,
            max_tool_calls_per_step: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GuardrailResult {
    pub allowed: bool,
    pub reason: String,
    pub sanitized_input: Option<String>,
    pub sanitized_output: Option<String>,
    pub required_approval: bool,
    pub approval_id: Option<String>,
}

impl GuardrailResult {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason: reason.into(),
            sanitized_input: None,
            sanitized_output: None,
            required_approval: false,
            approval_id: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: reason.into(),
            sanitized_input: None,
            sanitized_output: None,
            required_approval: false,
            approval_id: None,
        }
    }

    pub fn sanitize(
        sanitized_input: impl Into<String>,
        sanitized_output: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            allowed: true,
            reason: reason.into(),
            sanitized_input: Some(sanitized_input.into()),
            sanitized_output: Some(sanitized_output.into()),
            required_approval: false,
            approval_id: None,
        }
    }

    pub fn require_approval(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: reason.into(),
            sanitized_input: None,
            sanitized_output: None,
            required_approval: true,
            approval_id: None,
        }
    }
}

#[async_trait]
pub trait IGuardrail: Send + Sync {
    async fn check_input(&self, input: &str) -> GuardrailResult;

    async fn check_output(&self, output: &str) -> GuardrailResult;

    async fn check_tool_call(
        &self,
        tool_name: &str,
        arguments: &str,
        permission: &ToolPermission,
    ) -> GuardrailResult;

    async fn check_permission(
        &self,
        actor_id: &str,
        target_resource: &str,
        action: &str,
    ) -> GuardrailResult;

    fn config(&self) -> &GuardrailConfig;
}

pub struct CompositeGuardrail {
    guardrails: Vec<Arc<dyn IGuardrail>>,
    config: GuardrailConfig,
}

impl CompositeGuardrail {
    pub fn new(guardrails: Vec<Arc<dyn IGuardrail>>, config: GuardrailConfig) -> Self {
        Self { guardrails, config }
    }
}

#[async_trait]
impl IGuardrail for CompositeGuardrail {
    async fn check_input(&self, input: &str) -> GuardrailResult {
        for guardrail in &self.guardrails {
            let result = guardrail.check_input(input).await;
            if !result.allowed {
                return result;
            }
            if let Some(sanitized) = result.sanitized_input {
                return GuardrailResult::sanitize(
                    sanitized,
                    result.sanitized_output.unwrap_or_default(),
                    "输入已脱敏",
                );
            }
        }
        GuardrailResult::allow("所有护栏检查通过")
    }

    async fn check_output(&self, output: &str) -> GuardrailResult {
        for guardrail in &self.guardrails {
            let result = guardrail.check_output(output).await;
            if !result.allowed {
                return result;
            }
            if let Some(sanitized) = result.sanitized_output {
                return GuardrailResult::sanitize(
                    result.sanitized_input.unwrap_or_default(),
                    sanitized,
                    "输出已脱敏",
                );
            }
        }
        GuardrailResult::allow("所有护栏检查通过")
    }

    async fn check_tool_call(
        &self,
        tool_name: &str,
        arguments: &str,
        permission: &ToolPermission,
    ) -> GuardrailResult {
        for guardrail in &self.guardrails {
            let result = guardrail
                .check_tool_call(tool_name, arguments, permission)
                .await;
            if !result.allowed {
                return result;
            }
        }
        GuardrailResult::allow("工具调用检查通过")
    }

    async fn check_permission(
        &self,
        actor_id: &str,
        target_resource: &str,
        action: &str,
    ) -> GuardrailResult {
        for guardrail in &self.guardrails {
            let result = guardrail
                .check_permission(actor_id, target_resource, action)
                .await;
            if !result.allowed {
                return result;
            }
        }
        GuardrailResult::allow("权限检查通过")
    }

    fn config(&self) -> &GuardrailConfig {
        &self.config
    }
}

pub struct NoopGuardrail {
    config: GuardrailConfig,
}

impl NoopGuardrail {
    pub fn new(config: GuardrailConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl IGuardrail for NoopGuardrail {
    async fn check_input(&self, _input: &str) -> GuardrailResult {
        GuardrailResult::allow("无操作护栏")
    }

    async fn check_output(&self, _output: &str) -> GuardrailResult {
        GuardrailResult::allow("无操作护栏")
    }

    async fn check_tool_call(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _permission: &ToolPermission,
    ) -> GuardrailResult {
        GuardrailResult::allow("无操作护栏")
    }

    async fn check_permission(
        &self,
        _actor_id: &str,
        _target_resource: &str,
        _action: &str,
    ) -> GuardrailResult {
        GuardrailResult::allow("无操作护栏")
    }

    fn config(&self) -> &GuardrailConfig {
        &self.config
    }
}
