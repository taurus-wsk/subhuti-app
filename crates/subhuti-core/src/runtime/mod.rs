//! # Runtime
//!
//! 运行时接口定义，无具体实现。

pub mod llm;
pub mod session;
pub mod tools;

pub use llm::{
    LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolCall, ToolCallResult, ToolInfo, LLM,
};
pub use session::*;
pub use tools::{Tool, ToolResponse, ToolResult};
