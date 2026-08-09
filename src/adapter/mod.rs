//! # 适配层
//!
//! 六边形架构的驱动侧/被驱动侧适配器：
//! - `inbound`：协议适配（HTTP/CLI），调用应用层入站端口
//! - `outbound`：技术适配，实现领域层出站端口 + 框架适配配置
//!
//! ## 对称性
//!
//! | 子模块 | 对端口的关系 | 依赖方向 |
//! |--------|-------------|----------|
//! | inbound | 调用 app 入站端口（ChatPort 等） | inbound → app |
//! | outbound | 实现 domain 出站端口（OrchestrationEnginePort 等） | outbound → domain |

pub mod inbound;
pub mod outbound;
