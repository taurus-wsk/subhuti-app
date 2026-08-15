//! # Subhuti App
//!
//! DDD 六边形架构应用。
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │  适配层 (adapter/)                                   │
//! │  inbound: HTTP/CLI（调用 app 入站端口）              │
//! │  outbound: 出站适配器 + 框架配置（实现 domain 端口） │
//! ├─────────────────────────────────────────────────────┤
//! │  应用层 (application/)                               │
//! │  CompositionRoot: 组装 Subhuti + 领域专家 + 垂直场景 │
//! ├─────────────────────────────────────────────────────┤
//! │  领域层 (domain/)                                    │
//! │  领域专家: BlenderExpert | ...（可转 WASM）          │
//! ├─────────────────────────────────────────────────────┤
//! │  基础设施层 (infra/)                                 │
//! │  配置加载 | 连接池 | 日志                            │
//! ├─────────────────────────────────────────────────────┤
//! │  框架层 (crates/subhuti)                             │
//! │  Orchestrator | ExpertAgent | Memory | Vertical     │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## 依赖方向
//!
//! - adapter/inbound → application → domain → 框架层
//! - adapter/outbound → domain → 框架层
//! - infra → 给 adapter 提供基础能力
//! - 领域层不依赖适配层和基础设施层（DDD 原则）
//! - 所有层依赖框架层（subhuti crate）

pub mod adapter;
pub mod application;
pub mod domain;
pub mod infra;
pub mod report;
