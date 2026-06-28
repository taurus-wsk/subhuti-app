//! # ReactFlow - ReAct 循环流程
//!
//! 适用场景：需要自动工具调用的场景
//!
//! 流程：Plan → Act → Observe → Reflect 循环

use super::{Flow, FlowConfig, FlowContext, FlowState};
use crate::runtime::llm::Role;
use anyhow::Result;
use async_trait::async_trait;
use tracing::{info, warn};

/// ReAct 流程实现
#[derive(Debug)]
pub struct ReactFlow {
    config: FlowConfig,
}

impl ReactFlow {
    /// 创建新的 ReactFlow
    pub fn new() -> Self {
        Self::with_config(FlowConfig::default())
    }

    /// 使用配置创建
    pub fn with_config(config: FlowConfig) -> Self {
        Self { config }
    }

    /// 构建系统提示词
    fn build_system_prompt(&self, tools: &[crate::runtime::tools::ToolInfo]) -> String {
        let mut prompt = String::from(
            "You are a helpful AI assistant.\n\n\
            IMPORTANT: Only use tools when the user's request actually requires them.\n\
            For simple greetings, questions, or text-based responses, just respond directly without using any tool.\n\n\
            Available tools:\n\n"
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
            "\n\
            IMPORTANT RULES:\n\
            1. For greetings (like '你好', 'hello', 'hi'): Respond directly without tools\n\
            2. For simple questions: Respond directly without tools\n\
            3. Only use a tool if you need to:\n\
               - Perform calculations (use 'calculate')\n\
               - Search for information (use 'search')\n\
               - Get weather (use 'weather')\n\
            \n\
            When you need to use a tool, respond in JSON format:\n\
            {\"tool\": \"tool_name\", \"args\": {\"param1\": \"value1\"}}\n\n\
            Otherwise, respond with your final answer directly.",
        );

        prompt
    }

    /// 解析工具调用（预留方法）
    #[allow(dead_code)]
    fn parse_tool_call(&self, response: &str) -> Option<(String, serde_json::Value)> {
        // 简单解析 JSON 格式的工具调用
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

    /// 观察结果 - 清洗和标准化
    fn observe_result(&self, result: &crate::runtime::tools::ToolResult) -> String {
        if result.success {
            result.content.clone()
        } else {
            result
                .error
                .clone()
                .unwrap_or_else(|| "Unknown error".to_string())
        }
    }
}

impl Default for ReactFlow {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Flow for ReactFlow {
    fn name(&self) -> &str {
        "react"
    }

    fn description(&self) -> &str {
        "ReAct 循环流程，自动工具调用"
    }

    async fn execute(&self, ctx: &mut FlowContext<'_>) -> Result<String> {
        info!("Starting ReactFlow for session: {}", ctx.session.id);

        let mut consecutive_no_tool = 0;
        let mut last_response = String::new();

        // 获取可用的工具列表
        let tools = ctx.get_tools();

        // 构建系统提示词
        let system_prompt = self.build_system_prompt(&tools);
        if ctx.session.system_prompt().is_none() {
            ctx.session.set_system_prompt(system_prompt);
        }

        loop {
            ctx.increment_iteration();

            // 检查是否超过最大迭代次数
            if ctx.is_exceeded_max_iterations() {
                warn!("Max iterations reached: {}", self.config.max_iterations);
                break;
            }

            // 检查收敛
            if consecutive_no_tool >= self.config.convergence_threshold {
                info!(
                    "Converged after {} consecutive no-tool responses",
                    consecutive_no_tool
                );
                break;
            }

            ctx.set_state(FlowState::Planning);

            // 调用 LLM（使用 function calling API）
            let llm_response = ctx.call_llm_with_tools().await?;
            last_response = llm_response.content.clone();

            // 检查是否有工具调用
            if let Some(tool_call) = llm_response.tool_call {
                consecutive_no_tool = 0;
                ctx.set_state(FlowState::Acting);
                ctx.session.increment_tool_calls();

                // 检查工具调用是否有效（参数不为空）
                let args_str = serde_json::to_string(&tool_call.arguments).unwrap_or_default();
                if args_str == "{}" || args_str.is_empty() {
                    // 工具调用参数为空，跳过工具执行
                    // 添加工具消息（使用 tool_call_id）
                    tracing::warn!("Tool call has empty arguments, re-prompting without tools");
                    ctx.session.add_tool_message("{}", &tool_call.id);

                    // 再次调用 LLM，这次不传工具（通过 system prompt 指示）
                    let retry_response = ctx.call_llm().await?;
                    ctx.set_state(FlowState::Completed);
                    ctx.session.add_message(Role::Assistant, &retry_response);
                    return Ok(retry_response);
                }

                info!(
                    "Executing tool: {} with args: {:?}",
                    tool_call.name, tool_call.arguments
                );

                // 执行工具
                match ctx.execute_tool(&tool_call.name, tool_call.arguments).await {
                    Ok(result) => {
                        // 清洗结果
                        ctx.set_state(FlowState::Observing);
                        let observed = self.observe_result(&result);
                        ctx.session.add_message(Role::Tool, observed);
                    }
                    Err(e) => {
                        warn!("Tool execution failed: {}", e);
                        ctx.session.add_message(Role::Tool, format!("Error: {}", e));
                    }
                }
            } else {
                // 没有工具调用（普通文本响应）
                consecutive_no_tool += 1;
                ctx.session
                    .add_message(Role::Assistant, &llm_response.content);

                if consecutive_no_tool >= self.config.convergence_threshold {
                    ctx.set_state(FlowState::Completed);
                    info!("ReactFlow completed");
                    return Ok(llm_response.content);
                }
            }
        }

        // 最终回答
        ctx.set_state(FlowState::Completed);
        ctx.session.add_message(Role::Assistant, &last_response);
        Ok(last_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_call() {
        let flow = ReactFlow::new();
        let json = r#"{"tool": "search", "args": {"query": "test"}}"#;
        let result = flow.parse_tool_call(json);
        assert!(result.is_some());

        let (name, args) = result.unwrap();
        assert_eq!(name, "search");
        assert_eq!(args["query"], "test");
    }

    #[test]
    fn test_parse_tool_call_invalid() {
        let flow = ReactFlow::new();
        let result = flow.parse_tool_call("Just a normal response");
        assert!(result.is_none());
    }

    #[test]
    fn test_react_flow_creation() {
        let flow = ReactFlow::new();
        assert_eq!(flow.name(), "react");
    }
}
