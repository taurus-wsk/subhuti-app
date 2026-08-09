//! # Runtime
//!
//! 运行时接口定义，无具体实现。

pub mod context;
pub mod llm;
pub mod session;
pub mod tools;

pub use context::{
    CompressionStrategy, ContextCompressor, ContextConfig, ContextEntry, ContextManager,
    ContextPriority, ContextSnapshot, OverflowProtection, OverflowStrategy,
};
pub use llm::{
    LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolCall, ToolCallResult, ToolInfo, LLM,
};
pub use session::*;
pub use tools::{Tool, ToolResponse, ToolResult};
