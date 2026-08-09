//! # 基础设施层（纯技术）
//!
//! 连接池、配置加载、日志等底层工具，给 adapter 提供基础能力。
//! 不含业务适配逻辑（适配器在 `adapter/outbound`）。

pub mod config;
