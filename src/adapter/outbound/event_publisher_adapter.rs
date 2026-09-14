//! # 领域事件发布端口的出站实现
//!
//! 把领域自有的 [`DomainEvent`] 映射到框架 `AgentEventData`，并发布到框架 `EventBus`。
//!
//! 这是架构评价里「红色 ③ 可观测性依赖」被翻转的关键接缝：
//! 领域层只认识 `DomainEventPublisher` 抽象，永远不直接 `use subhuti_core::event`；
//! 本适配器（出站层，允许依赖框架）在此完成「领域事件 → 框架事件」的唯一映射点。

use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::event::{AgentEventData, EventBus};

use crate::domain::events::{DomainEvent, DomainEventPublisher};

/// 基于 Subhuti 框架 `EventBus` 的领域事件发布器。
pub struct SubhutiEventPublisher {
    bus: Arc<EventBus>,
}

impl SubhutiEventPublisher {
    /// 包裹一个框架事件总线。出站层持有 `EventBus` 是合理且必要的（适配器职责所在）。
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl DomainEventPublisher for SubhutiEventPublisher {
    async fn publish(&self, event: DomainEvent, trace_id: &str, session_id: Option<&str>) {
        // 唯一映射点：领域事件 → 框架事件。
        let data: AgentEventData = match event {
            DomainEvent::LlmCalling {
                messages_count,
                model,
            } => AgentEventData::LLMCalling {
                messages_count,
                model,
            },
            // tokens_used 用 Option 表达「供应商没给用量」，但框架事件字段是 u64，
            // 此处 None → 0（与「未知即不计量」等价，汇总结果一致），
            // 真实数字由 Zhipu/OpenAI/Doubao 客户端的 chat_counted 提供。
            DomainEvent::LlmResponded {
                response,
                tokens_used,
                duration_ms,
            } => AgentEventData::LLMResponded {
                response,
                tokens_used: tokens_used.unwrap_or(0),
                duration_ms,
            },
            DomainEvent::ToolCalling { tool_name, args } => {
                AgentEventData::ToolCalling { tool_name, args }
            }
            DomainEvent::ToolResponded {
                tool_name,
                result,
                success,
                duration_ms,
            } => AgentEventData::ToolResponded {
                tool_name,
                result,
                success,
                duration_ms,
            },
            DomainEvent::MemoryRetrieved {
                query,
                results_count,
            } => AgentEventData::MemoryRetrieved {
                query,
                results_count,
            },
        };

        self.bus
            .emit_with_trace(
                data,
                trace_id.to_string(),
                session_id.map(|s| s.to_string()),
            )
            .await;
    }
}
