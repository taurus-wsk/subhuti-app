//! # Vertical
//!
//! 垂直领域接口定义：工具、资产、项目记忆、工作流。

pub mod asset;
pub mod project;
pub mod tool;
pub mod workflow;

pub use asset::*;
pub use project::*;
pub use tool::{ToolCommand, ToolCommandInfo, ToolIntegration, ToolRegistry};
pub use workflow::*;
