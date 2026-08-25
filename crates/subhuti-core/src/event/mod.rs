//! # 事件驱动架构 (Event-Driven Architecture)
//!
//! Agent 内部执行核心的事件总线，解耦组件间的直接调用。
//!
//! ## 架构
//!
//! ```text
//! ┌───────────────────────────────────────────────────────┐
//! │                     EventBus                           │
//! │                                                       │
//! │   publish() ──► [broadcast channel] ──► subscribers   │
//! │                                                       │
//! │   ┌─────────┐  ┌──────────┐  ┌─────────────────┐     │
//! │   │ Agent   │  │ Memory   │  │ TraceObserver   │     │
//! │   │ 执行器   │  │ 记忆系统  │  │ 追踪观察者       │     │
//! │   └────┬────┘  └────┬─────┘  └───────┬─────────┘     │
//! │        │            │                │                │
//! │        ▼            ▼                ▼                │
//! │     publish      subscribe        subscribe           │
//! └───────────────────────────────────────────────────────┘
//! ```
//!
//! ## 事件流
//!
//! ```text
//! UserMessage ──► AgentMatched ──► FlowStarted ──► LLMCalling
//!      │                                              │
//!      │              ┌───────────────────────────────┘
//!      │              ▼
//!      │         LLMResponded ──► ToolCalling ──► ToolResponded
//!      │                                              │
//!      │              ┌───────────────────────────────┘
//!      │              ▼
//!      ◄───────── AgentCompleted
//! ```

pub mod bus;
pub mod handler;
pub mod recorder;
pub mod types;

pub use bus::{EventBus, EventBusConfig};
pub use handler::{EventFilter, EventHandler, EventSubscription};
pub use recorder::{
    EventPlayer, EventRecorder, Recording, ReplayError, ReplayResult, ReplayStats, ReplayStrategy,
    TimelineEntry,
};
pub use types::{AgentEventData, Event, EventId, EventMetadata, EventTimestamp};
