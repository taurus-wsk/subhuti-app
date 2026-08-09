//! # 事件处理器
//!
//! 定义事件处理 trait 和订阅管理。

use super::types::{AgentEventData, Event};
use async_trait::async_trait;
use std::sync::Arc;

/// 事件处理器 trait
///
/// 实现此 trait 来订阅和处理事件。
///
/// ```rust,ignore
/// use subhuti::event::{EventHandler, Event};
///
/// struct LoggingHandler;
///
/// #[async_trait]
/// impl EventHandler for LoggingHandler {
///     async fn handle(&self, event: &Event) {
///         tracing::info!("Event received: {:?}", event.data);
///     }
///
///     fn filter(&self) -> EventFilter {
///         EventFilter::All
///     }
/// }
/// ```
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// 处理事件
    async fn handle(&self, event: &Event);

    /// 事件过滤器，决定哪些事件需要处理
    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    /// 处理器名称（用于调试和日志）
    fn name(&self) -> &str {
        "unnamed"
    }
}

/// 事件过滤器
#[derive(Clone)]
pub enum EventFilter {
    /// 接收所有事件
    All,
    /// 只接收指定类型的事件
    Types(Vec<&'static str>),
    /// 自定义过滤函数
    Custom(Arc<dyn Fn(&Event) -> bool + Send + Sync>),
}

impl std::fmt::Debug for EventFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::All => write!(f, "EventFilter::All"),
            Self::Types(types) => write!(f, "EventFilter::Types({:?})", types),
            Self::Custom(_) => write!(f, "EventFilter::Custom(<closure>)"),
        }
    }
}

impl EventFilter {
    /// 检查事件是否通过过滤器
    pub fn matches(&self, event: &Event) -> bool {
        match self {
            Self::All => true,
            Self::Types(types) => types.contains(&event.data.event_type()),
            Self::Custom(f) => f(event),
        }
    }
}

impl PartialEq for EventFilter {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::All, Self::All) => true,
            (Self::Types(a), Self::Types(b)) => a == b,
            _ => false,
        }
    }
}

/// 事件订阅
///
/// 代表一个活跃的事件订阅，取消订阅时 drop 即可。
pub struct EventSubscription {
    /// 订阅 ID
    pub id: String,
    /// 处理器名称
    pub handler_name: String,
    /// 过滤器
    pub filter: EventFilter,
}

impl EventSubscription {
    pub fn new(handler_name: impl Into<String>, filter: EventFilter) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            handler_name: handler_name.into(),
            filter,
        }
    }
}

impl std::fmt::Debug for EventSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSubscription")
            .field("id", &self.id)
            .field("handler_name", &self.handler_name)
            .field("filter", &self.filter)
            .finish()
    }
}

/// 便捷函数：创建类型过滤器
pub fn filter_types(types: Vec<&'static str>) -> EventFilter {
    EventFilter::Types(types)
}

/// 便捷函数：创建自定义过滤器
pub fn filter_custom<F>(f: F) -> EventFilter
where
    F: Fn(&Event) -> bool + Send + Sync + 'static,
{
    EventFilter::Custom(Arc::new(f))
}

/// 内置：日志事件处理器
pub struct LoggingEventHandler {
    name: String,
}

impl LoggingEventHandler {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Default for LoggingEventHandler {
    fn default() -> Self {
        Self::new("logging")
    }
}

#[async_trait]
impl EventHandler for LoggingEventHandler {
    async fn handle(&self, event: &Event) {
        let event_type = event.data.event_type();
        let trace_id = event.metadata.trace_id.as_deref().unwrap_or("-");

        // LLM 和工具层事件用 trace 级别，其他用 debug
        match event.data {
            crate::event::types::AgentEventData::LLMCalling { .. }
            | crate::event::types::AgentEventData::LLMResponded { .. }
            | crate::event::types::AgentEventData::LLMStreamChunk { .. }
            | crate::event::types::AgentEventData::ToolCalling { .. }
            | crate::event::types::AgentEventData::ToolResponded { .. } => {
                tracing::trace!(
                    handler = %self.name,
                    event_type = %event_type,
                    trace_id = %trace_id,
                    "[EventBus] {:?}",
                    event.data
                );
            }
            _ => {
                tracing::debug!(
                    handler = %self.name,
                    event_type = %event_type,
                    trace_id = %trace_id,
                    "[EventBus] {:?}",
                    event.data
                );
            }
        }
    }

    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// 内置：Trace 事件处理器（将事件写入 Trace 系统）
pub struct TraceEventHandler {
    name: String,
}

impl TraceEventHandler {
    pub fn new() -> Self {
        Self {
            name: "trace".to_string(),
        }
    }
}

impl Default for TraceEventHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventHandler for TraceEventHandler {
    async fn handle(&self, event: &Event) {
        // 只处理 Span 相关事件
        match &event.data {
            AgentEventData::SpanStarted { span_name, .. } => {
                tracing::info!(
                    handler = %self.name,
                    span = %span_name,
                    trace_id = ?event.metadata.trace_id,
                    "Span started"
                );
            }
            AgentEventData::SpanEnded {
                span_name,
                duration_ms,
                ..
            } => {
                tracing::info!(
                    handler = %self.name,
                    span = %span_name,
                    duration_ms = %duration_ms,
                    trace_id = ?event.metadata.trace_id,
                    "Span ended"
                );
            }
            _ => {}
        }
    }

    fn filter(&self) -> EventFilter {
        EventFilter::Types(vec!["span_started", "span_ended"])
    }

    fn name(&self) -> &str {
        &self.name
    }
}
