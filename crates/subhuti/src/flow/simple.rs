//! # SimpleFlow - 简单对话流程
//!
//! 适用场景：简单对话，无工具调用
//!
//! 流程：直接调用 LLM，返回结果

use super::{Flow, FlowConfig, FlowContext, FlowState};
use crate::runtime::llm::Role;
use anyhow::Result;
use async_trait::async_trait;

/// 简单对话流程
#[derive(Debug)]
pub struct SimpleFlow {
    #[allow(dead_code)]
    config: FlowConfig,
}

impl SimpleFlow {
    /// 创建新的 SimpleFlow
    pub fn new() -> Self {
        Self::with_config(FlowConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: FlowConfig) -> Self {
        Self { config }
    }
}

impl Default for SimpleFlow {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Flow for SimpleFlow {
    fn name(&self) -> &str {
        "simple"
    }

    fn description(&self) -> &str {
        "简单对话流程，直接调用 LLM，无工具调用"
    }

    async fn execute(&self, ctx: &mut FlowContext<'_>) -> Result<String> {
        tracing::info!("Starting SimpleFlow for session: {}", ctx.session.id);

        ctx.set_state(FlowState::Planning);

        // 直接调用 LLM
        let response = ctx.call_llm().await?;

        ctx.set_state(FlowState::Completed);
        ctx.session.add_message(Role::Assistant, &response);

        tracing::info!("SimpleFlow completed");
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_flow_creation() {
        let flow = SimpleFlow::new();
        assert_eq!(flow.name(), "simple");
    }
}
