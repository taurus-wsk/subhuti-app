//! # 领域自有可观测性事件
//!
//! 把「框架事件总线」这一外层关注点翻成领域抽象（依赖倒置）。
//!
//! 此前领域层（`traits.rs` / 各 expert）直接 `use subhuti_core::event::{AgentEventData, EventBus}`，
//! 等于领域（最内层的高层策略）反向依赖了外层框架——这是架构评价里红色 ③ 的可观测性依赖。
//!
//! 现在领域层只认识本模块的两个类型：
//! - [`DomainEvent`]：领域侧「发生了什么事」的纯描述（不依赖任何框架字段语义）。
//! - [`DomainEventPublisher`]：发布端口（抽象）。
//!
//! 具体的「映射到框架 `AgentEventData` 并推到 `EventBus`」由出站适配器
//! [`crate::adapter::outbound::event_publisher_adapter::SubhutiEventPublisher`] 实现。
//! 这样领域层对可观测性的依赖被翻成「依赖自己的抽象」，新增事件只需在两侧各加一个分支。

use async_trait::async_trait;
use serde_json::Value;

/// 领域侧可观测性事件。
///
/// 字段刻意与 `subhuti_core::event::AgentEventData` 中对应变体对齐，
/// 方便适配器做 1:1 映射；但本类型**不** import 任何框架符号。
#[derive(Debug, Clone)]
pub enum DomainEvent {
    /// LLM 调用前（think 阶段）
    LlmCalling {
        messages_count: usize,
        model: Option<String>,
    },
    /// LLM 调用返回（think 阶段收尾，并承载 token 用量用于成本核算）
    ///
    /// 这是 token 成本链路的**唯一源头**：框架侧只有 `LLMResponded.tokens_used`
    /// 会填进 `SpanData.tokens`，进而被 `total_tokens()` 汇总。
    LlmResponded {
        /// 响应正文（写入 span 便于排查；调用方负责截断，避免整篇塞进 telemetry）
        response: String,
        /// provider 返回的 `usage.total_tokens`；`None` 表示该供应商/该路径未提供用量
        tokens_used: Option<u64>,
        duration_ms: u64,
    },
    /// 工具调用前
    ToolCalling { tool_name: String, args: Value },
    /// 工具响应后
    ToolResponded {
        tool_name: String,
        result: String,
        success: bool,
        duration_ms: u64,
    },
    /// 记忆检索
    MemoryRetrieved { query: String, results_count: usize },
}

/// 领域事件发布端口（依赖倒置：领域只依赖此抽象，不依赖框架 EventBus）。
///
/// 由出站适配器实现，内部持有一个 `Arc<subhuti_core::event::EventBus>`，
/// 在 `publish` 里把 [`DomainEvent`] 翻译成框架 `AgentEventData` 并发布。
#[async_trait]
pub trait DomainEventPublisher: Send + Sync {
    /// 发布一条领域事件，附带链路关联的 trace_id / session_id。
    ///
    /// 这两个标识由 [`crate::domain::traits::TraceContext`] 透传，
    /// 适配器据此调用 `EventBus::emit_with_trace`。
    async fn publish(&self, event: DomainEvent, trace_id: &str, session_id: Option<&str>);
}
