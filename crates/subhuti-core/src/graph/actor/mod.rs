//! # Actor 层 - 执行模型
//!
//! 图 + 事件 + Actor 三层混合架构的执行层。
//!
//! ## 三层职责
//!
//! | 层 | 职责 | 模块 |
//! |----|------|------|
//! | **Graph** | 拓扑结构、路由、循环检测 | `graph::engine` |
//! | **Actor** | 并发执行、状态隔离、故障恢复 | `graph::actor` |
//! | **Event** | 解耦通信、可观测性 | `event` |
//!
//! ## Actor 模型核心
//!
//! - **NodeActor**：每个图节点封装为独立 Actor，有私有状态和邮箱
//! - **Supervisor**：管理 Actor 生命周期，提供故障恢复策略
//! - **Mailbox**：`mpsc` 通道实现异步消息传递和背压
//!
//! ## 消息协议
//!
//! ```text
//! Engine ──Execute──► NodeActor ──NodeResult──► Engine
//!         ──HealthCheck──►        ──ActorHealth──►
//!         ──GetStats──►           ──ActorStats──►
//!         ──Terminate──►          (停止)
//! ```

pub mod event_driven_actor;
pub mod message;
pub mod node_actor;
pub mod scheduler;
pub mod supervisor;

pub use event_driven_actor::EventDrivenActor;
pub use message::{ActorAddr, ActorHandle, ActorHealth, ActorLifecycle, ActorStats, NodeMessage};
pub use node_actor::NodeActor;
pub use scheduler::EventDrivenScheduler;
pub use supervisor::{SupervisionStrategy, Supervisor};
