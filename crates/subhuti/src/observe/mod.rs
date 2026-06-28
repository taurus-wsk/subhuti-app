//! # 可观测性系统
//!
//! 提供完整的 Agent 运行观测能力：
//! - **Trace 追踪**：记录完整的请求处理链路
//! - **Span 树**：可视化展示思考过程
//! - **统计指标**：性能分析、Token 消耗
//!
//! ## 核心概念
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                    TraceObserver                       │
//! │  ┌─────────────┐   ┌─────────────┐   ┌────────────┐ │
//! │  │ TraceStore  │   │ TraceBuilder│   │  SpanTree  │ │
//! │  │  存储追踪   │   │  创建追踪   │   │  可视化    │ │
//! │  └─────────────┘   └─────────────┘   └────────────┘ │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! let observer = TraceObserver::new();
//!
//! // 创建 Trace
//! let mut trace = observer.create_trace("user1", "session1", "用户输入");
//!
//! // 记录 Skill 匹配
//! trace.record_skill_match("weather", 0.95);
//!
//! // 记录工具调用
//! trace.record_tool_call("weather_api", &input, &output);
//!
//! // 完成 Trace
//! trace.complete("最终输出");
//! observer.store_trace(trace);
//!
//! // 查询 Trace
//! let tree = observer.get_span_tree(&trace_id);
//! ```

pub mod session;
pub mod trace;

pub use trace::{
    Span, SpanKind, SpanStatus, SpanTreeNode, TokenUsage, Trace, TraceId, TraceObserver,
    TraceStatus, TraceStore, TraceSummary,
};

pub use session::{SessionObserver, SessionRecord, SessionRecordParams, SessionRequest};
