//! Subhuti AI Agent 完整示例
//!
//! 展示如何整合 LLM + Tool + Memory 构建完整的 Agent

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use subhuti::runtime::tools::{Tool, ToolInfo, ToolResult};

/// 自定义工具 - 获取当前时间
#[allow(dead_code)]
struct TimeTool;

#[async_trait]
impl Tool for TimeTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "get_time".to_string(),
            description: "获取当前时间".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn run(&self, _params: Value) -> Result<ToolResult> {
        let now = chrono::Utc::now();
        Ok(ToolResult::ok(format!(
            "当前时间: {}",
            now.format("%Y-%m-%d %H:%M:%S UTC")
        )))
    }
}

/// 自定义工具 - 计算器
#[allow(dead_code)]
struct CalculatorTool;

#[async_trait]
impl Tool for CalculatorTool {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            name: "calculate".to_string(),
            description: "执行数学计算".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "数学表达式，如 '2 + 3 * 4'"
                    }
                },
                "required": ["expression"]
            }),
        }
    }

    async fn run(&self, params: Value) -> Result<ToolResult> {
        let expression = params["expression"].as_str().unwrap_or("0");
        // 简化实现 - 实际应该使用 math parser
        Ok(ToolResult::ok(format!("计算表达式: {}", expression)))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== Subhuti AI Agent 完整演示 ===\n");

    Ok(())
}
