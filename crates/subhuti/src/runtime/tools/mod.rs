//! # Tools Layer - 工具系统
//!
//! 极简 Tool Trait：name / desc / schema / run
//! 所有记忆搜索、外部能力全部是工具

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 工具信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
    /// 参数 schema
    pub parameters: Value,
}

/// 工具执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// 是否成功
    pub success: bool,
    /// 结果内容
    pub content: String,
    /// 错误信息
    pub error: Option<String>,
}

impl ToolResult {
    /// 创建成功结果
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
            error: None,
        }
    }

    /// 创建错误结果
    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            content: String::new(),
            error: Some(error.into()),
        }
    }
}

/// 工具 Trait
#[async_trait]
pub trait Tool: Send + Sync {
    /// 获取工具信息
    fn info(&self) -> ToolInfo;

    /// 执行工具
    async fn run(&self, params: Value) -> Result<ToolResult>;
}

/// 内置工具命名空间
pub mod builtin {
    use super::*;
    use crate::memory::Memory;

    /// 搜索短期记忆工具
    pub struct SearchShortTermTool {
        memory: Memory,
    }

    impl SearchShortTermTool {
        pub fn new(memory: Memory) -> Self {
            Self { memory }
        }
    }

    #[async_trait]
    impl Tool for SearchShortTermTool {
        fn info(&self) -> ToolInfo {
            ToolInfo {
                name: "search_short_term".to_string(),
                description: "搜索短期工作记忆".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "搜索查询"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "返回结果数量限制",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
            }
        }

        async fn run(&self, params: Value) -> Result<ToolResult> {
            let query = params["query"].as_str().unwrap_or("");
            let limit = params["limit"].as_u64().unwrap_or(5) as usize;

            let results = self.memory.search_short_term(query, limit);
            let content = serde_json::to_string(&results).unwrap_or_default();

            Ok(ToolResult::ok(content))
        }
    }

    /// 搜索长期记忆工具
    pub struct SearchArchiveTool {
        memory: Memory,
    }

    impl SearchArchiveTool {
        pub fn new(memory: Memory) -> Self {
            Self { memory }
        }
    }

    #[async_trait]
    impl Tool for SearchArchiveTool {
        fn info(&self) -> ToolInfo {
            ToolInfo {
                name: "search_archive".to_string(),
                description: "搜索长期归档记忆".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "搜索查询"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "返回结果数量限制",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
            }
        }

        async fn run(&self, params: Value) -> Result<ToolResult> {
            let query = params["query"].as_str().unwrap_or("");
            let limit = params["limit"].as_u64().unwrap_or(5) as usize;

            let results = self.memory.search_archive(query, limit);
            let content = serde_json::to_string(&results).unwrap_or_default();

            Ok(ToolResult::ok(content))
        }
    }

    /// 搜索知识库工具
    pub struct SearchKnowledgeTool {
        memory: Memory,
    }

    impl SearchKnowledgeTool {
        pub fn new(memory: Memory) -> Self {
            Self { memory }
        }
    }

    #[async_trait]
    impl Tool for SearchKnowledgeTool {
        fn info(&self) -> ToolInfo {
            ToolInfo {
                name: "search_knowledge".to_string(),
                description: "搜索知识库语义记忆".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "搜索查询"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "返回结果数量限制",
                            "default": 5
                        }
                    },
                    "required": ["query"]
                }),
            }
        }

        async fn run(&self, params: Value) -> Result<ToolResult> {
            let query = params["query"].as_str().unwrap_or("");
            let limit = params["limit"].as_u64().unwrap_or(5) as usize;

            let results = self.memory.search_knowledge(query, limit);
            let content = serde_json::to_string(&results).unwrap_or_default();

            Ok(ToolResult::ok(content))
        }
    }
}

/// 注册所有内置记忆工具
pub fn register_memory_tools(memory: crate::memory::Memory, runtime: &crate::runtime::Runtime) {
    runtime.register_tool(builtin::SearchShortTermTool::new(memory.clone()));
    runtime.register_tool(builtin::SearchArchiveTool::new(memory.clone()));
    runtime.register_tool(builtin::SearchKnowledgeTool::new(memory.clone()));
}
