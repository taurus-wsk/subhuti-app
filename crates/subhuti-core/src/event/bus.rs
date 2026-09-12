//! # 事件总线
//!
//! 基于 tokio::broadcast 的发布订阅事件总线。
//!
//! ## 特点
//!
//! - **异步**: 所有操作都是异步的
//! - **解耦**: 发布者不需要知道订阅者
//! - **可过滤**: 订阅者可以通过过滤器只接收感兴趣的事件
//! - **可观测**: 内置日志和 Trace 处理器
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use subhuti::event::{EventBus, EventHandler, Event, AgentEventData};
//!
//! // 1. 创建事件总线
//! let bus = EventBus::new(1024);
//!
//! // 2. 订阅事件
//! bus.subscribe(Arc::new(LoggingEventHandler::new())).await;
//!
//! // 3. 发布事件
//! bus.publish(Event::new(AgentEventData::UserMessage {
//!     message: "你好".to_string(),
//! })).await;
//! ```

use super::handler::{EventHandler, EventSubscription};
use super::types::{AgentEventData, Event};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// 事件总线配置
#[derive(Debug, Clone)]
pub struct EventBusConfig {
    /// 广播通道容量（历史事件缓存）
    pub capacity: usize,
    /// 是否启用内置日志处理器
    pub enable_logging: bool,
    /// 是否启用内置 Trace 处理器
    pub enable_trace: bool,
}

impl Default for EventBusConfig {
    fn default() -> Self {
        Self {
            capacity: 1024,
            enable_logging: true,
            enable_trace: true,
        }
    }
}

/// 事件总线
///
/// Agent 内部的事件中枢，所有组件通过事件总线通信。
pub struct EventBus {
    /// 广播发送端
    sender: broadcast::Sender<Event>,
    /// 已注册的处理器
    handlers: RwLock<Vec<RegisteredHandler>>,
    /// 配置
    config: EventBusConfig,
}

struct RegisteredHandler {
    subscription: EventSubscription,
    handler: Arc<dyn EventHandler>,
}

impl EventBus {
    /// 创建事件总线
    pub fn new(capacity: usize) -> Self {
        Self::with_config(EventBusConfig {
            capacity,
            ..Default::default()
        })
    }

    /// 从配置创建事件总线
    pub fn with_config(config: EventBusConfig) -> Self {
        let (sender, _) = broadcast::channel(config.capacity);

        Self {
            sender,
            handlers: RwLock::new(Vec::new()),
            config,
        }
    }

    /// 初始化内置处理器（需要在异步上下文中调用）
    pub async fn init_builtin_handlers(&self) {
        if self.config.enable_logging {
            self.subscribe(Arc::new(super::handler::LoggingEventHandler::default()))
                .await;
        }

        // 注：`TraceEventHandler`（响应 SpanStarted/SpanEnded）已删除——
        // 这两个事件从未被 emit，链路 capture 由 `TraceEventBridge` 负责。
    }

    /// 订阅事件
    ///
    /// 注册一个事件处理器，返回订阅 ID。
    /// 处理器的 filter 决定接收哪些事件。
    pub async fn subscribe(&self, handler: Arc<dyn EventHandler>) -> String {
        let filter = handler.filter();
        let name = handler.name().to_string();
        let subscription = EventSubscription::new(name.clone(), filter);

        let id = subscription.id.clone();

        let registered = RegisteredHandler {
            subscription,
            handler,
        };

        self.handlers.write().await.push(registered);

        tracing::debug!(
            subscription_id = %id,
            handler_name = %name,
            "EventBus: handler subscribed"
        );

        id
    }

    /// 取消订阅
    pub async fn unsubscribe(&self, subscription_id: &str) -> bool {
        let mut handlers = self.handlers.write().await;
        let before = handlers.len();
        handlers.retain(|h| h.subscription.id != subscription_id);
        let removed = before - handlers.len();

        if removed > 0 {
            tracing::debug!(
                subscription_id = %subscription_id,
                "EventBus: handler unsubscribed"
            );
            true
        } else {
            false
        }
    }

    /// 发布事件
    ///
    /// 将事件广播给所有匹配的订阅者。
    /// 注意：此函数不会阻塞，事件处理是异步的。
    pub async fn publish(&self, event: Event) {
        let event_type = event.data.event_type();
        let trace_id = event.metadata.trace_id.clone();

        // 通过 broadcast channel 发送
        // 忽略发送错误（可能没有订阅者）
        let _ = self.sender.send(event.clone());

        // 分发给所有匹配的处理器
        let handlers = self.handlers.read().await;
        let matched_count = handlers
            .iter()
            .filter(|h| h.subscription.filter.matches(&event))
            .count();

        if matched_count > 0 {
            tracing::trace!(
                event_type = %event_type,
                matched_handlers = %matched_count,
                trace_id = ?trace_id,
                "EventBus: dispatching event"
            );
        }

        for handler in handlers.iter() {
            if handler.subscription.filter.matches(&event) {
                let h = handler.handler.clone();
                let e = event.clone();
                tokio::spawn(async move {
                    h.handle(&e).await;
                });
            }
        }
    }

    /// 发布事件（便捷方法，自动创建 Event 包装）
    pub async fn emit(&self, data: AgentEventData) {
        self.publish(Event::new(data)).await;
    }

    /// 发布带 trace 上下文的事件
    pub async fn emit_with_trace(
        &self,
        data: AgentEventData,
        trace_id: impl Into<String>,
        session_id: Option<String>,
    ) {
        let event = Event::new(data)
            .with_trace(trace_id)
            .with_session(session_id.unwrap_or_default());
        self.publish(event).await;
    }

    /// 获取订阅者数量
    pub async fn subscriber_count(&self) -> usize {
        self.handlers.read().await.len()
    }

    /// 获取广播接收端（用于自定义消费）
    pub fn subscribe_raw(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// 清空所有订阅
    pub async fn clear(&self) {
        let mut handlers = self.handlers.write().await;
        let count = handlers.len();
        handlers.clear();
        tracing::debug!("EventBus: cleared {} handlers", count);
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("capacity", &self.config.capacity)
            .field("subscriber_count", &self.sender.receiver_count())
            .finish()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::handler::EventFilter;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingHandler {
        counter: Arc<AtomicUsize>,
        filter: EventFilter,
    }

    #[async_trait]
    impl EventHandler for CountingHandler {
        async fn handle(&self, _event: &Event) {
            self.counter.fetch_add(1, Ordering::SeqCst);
        }

        fn filter(&self) -> EventFilter {
            self.filter.clone()
        }

        fn name(&self) -> &str {
            "counting"
        }
    }

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = EventBus::new(64);
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(CountingHandler {
            counter: counter.clone(),
            filter: EventFilter::All,
        });

        bus.subscribe(handler).await;

        bus.emit(AgentEventData::UserMessage {
            message: "test".to_string(),
        })
        .await;

        // 等待异步处理
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_filtered_subscription() {
        let bus = EventBus::new(64);
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(CountingHandler {
            counter: counter.clone(),
            filter: EventFilter::Types(vec!["llm_calling", "llm_responded"]),
        });

        bus.subscribe(handler).await;

        // 发送不匹配的事件
        bus.emit(AgentEventData::UserMessage {
            message: "test".to_string(),
        })
        .await;

        // 发送匹配的事件
        bus.emit(AgentEventData::LLMCalling {
            messages_count: 1,
            model: None,
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_unsubscribe() {
        let bus = EventBus::new(64);
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(CountingHandler {
            counter: counter.clone(),
            filter: EventFilter::All,
        });

        let sub_id = bus.subscribe(handler).await;

        bus.emit(AgentEventData::UserMessage {
            message: "before".to_string(),
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        bus.unsubscribe(&sub_id).await;

        bus.emit(AgentEventData::UserMessage {
            message: "after".to_string(),
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
