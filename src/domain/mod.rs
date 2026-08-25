//! # 领域层（洋葱核心）
//!
//! 纯业务逻辑，**完全独立于外部框架**（不依赖 subhuti、HTTP、数据库等）。
//!
//! ## 目录结构
//!
//! - `experts/` - 领域专家（演员），实现 `DomainExpert` trait
//! - `traits/` - 领域接口定义（`DomainExpert`、`DomainLlm`、`DomainSkill`）
//!
//! ## 与框架核心的关系
//!
//! 领域层定义纯领域接口（`DomainExpert` 等），不依赖任何框架类型。
//! 出站适配层通过适配器（`DomainExpertAdapter`）将领域专家桥接到框架的 `ExpertAgent`。
//!
//! 框架核心（`subhuti-core`）提供执行管线和机制：
//! - `Orchestrator`（命运编织者）：决定走哪条路
//! - `Graph` / `RuleEngine`（执行引擎）：按路径执行
//!
//! 应用层（`CompositionRoot`）作为组合根，将领域专家注册到框架核心。
//!
//! ## 设计原则
//!
//! - **领域层完全独立**：不依赖 subhuti 或任何外部框架
//! - **依赖倒置**：领域层定义接口，出站适配层实现适配器
//! - **可测试性**：可 mock `DomainLlm` 独立测试领域逻辑
//! - **可提取性**：领域代码可整体提取为独立 crate，编译为 WASM 插件
//!
pub mod dto;
pub mod experts;
pub mod pending_ask;
pub mod ports;
pub mod tool_fallback;
pub mod traits;
