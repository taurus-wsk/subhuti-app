//! # PlanActFlow - 先规划再执行流程
//!
//! 适用场景：复杂任务，需要先规划步骤再执行
//!
//! 流程：
//! 1. Plan: LLM 生成执行计划
//! 2. Act: 按计划执行工具
//! 3. Observe: 收集结果
//! 4. Final: 总结回答

use super::{Flow, FlowConfig, FlowContext, FlowState};
use crate::runtime::llm::Role;
use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

/// 先规划再执行流程
#[derive(Debug)]
pub struct PlanActFlow {
    config: FlowConfig,
}

impl PlanActFlow {
    /// 创建新的 PlanActFlow
    pub fn new() -> Self {
        Self::with_config(FlowConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: FlowConfig) -> Self {
        Self { config }
    }

    /// 构建规划提示词
    fn build_plan_prompt(&self, tools: &[crate::runtime::tools::ToolInfo]) -> String {
        let mut prompt = String::from(
            "You are an AI assistant. First, analyze the user's request and create a plan.\n\n\
            Available tools:\n\n",
        );

        for tool in tools {
            prompt.push_str(&format!(
                "- {}: {}\n  Parameters: {}\n\n",
                tool.name,
                tool.description,
                serde_json::to_string_pretty(&tool.parameters).unwrap_or_default()
            ));
        }

        prompt.push_str(
            "\nRespond in the following format:\n\
            PLAN:\n\
            1. [step description]\n\
            2. [step description]\n\
            ...\n\n\
            Then execute each step using tools if needed:\n\
            {\"tool\": \"tool_name\", \"args\": {\"param\": \"value\"}}\n\n\
            Finally, provide a summary answer.",
        );

        prompt
    }

    /// 解析工具调用（预留方法）
    #[allow(dead_code)]
    fn parse_tool_call(&self, response: &str) -> Option<(String, serde_json::Value)> {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(response) {
            if let (Some(tool), Some(args)) = (
                parsed.get("tool").and_then(|v| v.as_str()),
                parsed.get("args"),
            ) {
                return Some((tool.to_string(), args.clone()));
            }
        }
        None
    }
}

impl Default for PlanActFlow {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Flow for PlanActFlow {
    fn name(&self) -> &str {
        "plan_act"
    }

    fn description(&self) -> &str {
        "先规划再执行流程，适用于复杂任务"
    }

    async fn execute(&self, ctx: &mut FlowContext<'_>) -> Result<String> {
        info!("Starting PlanActFlow for session: {}", ctx.session.id);

        let tools = ctx.get_tools();
        let system_prompt = self.build_plan_prompt(&tools);
        if ctx.session.system_prompt().is_none() {
            ctx.session.set_system_prompt(system_prompt);
        }

        let mut last_response = String::new();

        loop {
            ctx.increment_iteration();

            if ctx.is_exceeded_max_iterations() {
                warn!("Max iterations reached: {}", self.config.max_iterations);
                break;
            }

            ctx.set_state(FlowState::Planning);

            // 获取 LLM 响应（使用 function calling API）
            let llm_response = ctx.call_llm_with_tools().await?;
            last_response = llm_response.content.clone();

            // 检查是否有工具调用
            if let Some(tool_call) = llm_response.tool_call {
                ctx.set_state(FlowState::Acting);
                ctx.session.increment_tool_calls();

                info!(
                    "Executing tool: {} with args: {:?}",
                    tool_call.name, tool_call.arguments
                );

                match ctx.execute_tool(&tool_call.name, tool_call.arguments).await {
                    Ok(result) => {
                        ctx.set_state(FlowState::Observing);
                        let observed = if result.success {
                            result.content.clone()
                        } else {
                            result
                                .error
                                .clone()
                                .unwrap_or_else(|| "Unknown error".to_string())
                        };
                        ctx.session.add_message(Role::Tool, observed);
                    }
                    Err(e) => {
                        warn!("Tool execution failed: {}", e);
                        ctx.session.add_message(Role::Tool, format!("Error: {}", e));
                    }
                }
            } else {
                // 没有工具调用，认为已完成
                ctx.set_state(FlowState::Completed);
                ctx.session
                    .add_message(Role::Assistant, &llm_response.content);
                info!("PlanActFlow completed");
                return Ok(llm_response.content);
            }
        }

        ctx.set_state(FlowState::Completed);
        ctx.session.add_message(Role::Assistant, &last_response);
        Ok(last_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_act_flow_creation() {
        let flow = PlanActFlow::new();
        assert_eq!(flow.name(), "plan_act");
    }
}
