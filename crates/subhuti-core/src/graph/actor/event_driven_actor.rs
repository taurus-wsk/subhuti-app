//! # EventDrivenActor - 事件驱动 Actor
//!
//! 豆包设计哲学："Actor 是细胞，靠事件驱动"
//!
//! 与 NodeActor 的区别：
//!
//! | NodeActor（邮箱模型） | EventDrivenActor（事件驱动）|
//! |----------------------|---------------------------|
//! | 通过 mpsc 邮箱接收消息 | 通过 EventBus 订阅事件 |
//! | 通过 oneshot 返回结果 | 通过 EventBus 发布结果事件 |
//! | 调度器需要知道 Actor 地址 | 调度器只需发事件，完全解耦 |
//! | 状态由调度器注入 | 状态通过事件消息传递 |
//! | 适合单机紧耦合 | 适合分布式/动态插拔 |
//!
//! ## 事件流
//!
//! ```text
//! 调度器 ──NodeExecuteRequested(+state)──► EventBus ──► EventDrivenActor
//!                                                              │
//!                                                        执行节点函数
//!                                                              │
//! EventDrivenActor ──NodeCompleted(+state_updates)──► EventBus ──► 调度器
//! ```
//!
//! ## 状态传递哲学
//!
//! 在 Actor 模型中，Actor 的状态是私有的，只能通过消息传递来改变。
//! 这里遵循这一哲学：
//! - 调度器把"当前状态"作为消息的一部分发给 Actor
//! - Actor 执行后，把"状态更新"作为消息的一部分发回调度器
//! - 调度器合并状态更新，再传给下一个 Actor
//! - Actor 本身是无状态的（每次执行都是独立实例）

use super::super::node::NodeFn;
use super::super::state::GraphState;
use crate::event::{AgentEventData, Event, EventBus, EventHandler};
use std::sync::Arc;

/// 事件驱动 Actor
///
/// 订阅 NodeExecuteRequested 事件，执行节点函数，发布 NodeCompleted 事件。
/// 完全通过事件总线通信，不持有任何地址引用。
/// 状态通过事件消息传递，Actor 自身无状态。
pub struct EventDrivenActor {
    /// 节点名称
    name: String,
    /// 节点执行函数
    func: NodeFn,
    /// 事件总线
    event_bus: Arc<EventBus>,
    /// 当前运行 ID（用于过滤事件）
    run_id: String,
}

impl std::fmt::Debug for EventDrivenActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventDrivenActor")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl EventDrivenActor {
    /// 创建事件驱动 Actor（异步，确保订阅完成）
    pub async fn new(
        name: impl Into<String>,
        func: NodeFn,
        event_bus: Arc<EventBus>,
        run_id: impl Into<String>,
    ) -> (Arc<Self>, String) {
        let actor = Arc::new(Self {
            name: name.into(),
            func,
            event_bus: event_bus.clone(),
            run_id: run_id.into(),
        });

        // 同步订阅，确保订阅完成后才返回
        let handler = actor.clone() as Arc<dyn EventHandler>;
        let subscription_id = event_bus.subscribe(handler).await;
        tracing::debug!(
            "🎬 EventDrivenActor '{}' 已订阅 (id={})",
            actor.name,
            subscription_id
        );

        (actor, subscription_id)
    }
}

#[async_trait::async_trait]
impl EventHandler for EventDrivenActor {
    async fn handle(&self, event: &Event) {
        match &event.data {
            AgentEventData::NodeExecuteRequested {
                run_id,
                node_name,
                step,
                state,
            } if node_name == &self.name && run_id == &self.run_id => {
                tracing::debug!(
                    "📥 Actor '{}' 收到执行请求: run={}, step={}, state_keys={:?}",
                    self.name,
                    run_id,
                    step,
                    state.keys()
                );

                // 从入站事件继承 trace_id/session_id（由调度器 emit_execute_request 注入）
                let trace_id = event.metadata.trace_id.clone();
                let session_id = event.metadata.session_id.clone();

                // 从事件中重建状态（Actor 模型：状态通过消息传递）
                let mut graph_state = GraphState::new();
                graph_state.merge(state.clone());

                // 执行节点函数
                let start = std::time::Instant::now();
                let result = self.func.call(graph_state).await;
                let duration_ms = start.elapsed().as_millis() as u64;

                // 根据执行结果发布事件（携带 trace_id 上下文，供 TraceEventBridge 桥接）
                if result.success {
                    tracing::debug!(
                        "📤 Actor '{}' 执行成功: run={}, duration={}ms",
                        self.name,
                        run_id,
                        duration_ms
                    );

                    self.emit_with_trace(
                        AgentEventData::NodeCompleted {
                            run_id: run_id.clone(),
                            node_name: self.name.clone(),
                            actor_name: self.name.clone(),
                            output: result.output.clone(),
                            success: true,
                            duration_ms,
                            next_nodes: vec![],
                            state_updates: result.state_updates.clone(),
                        },
                        trace_id.as_deref(),
                        session_id.as_deref(),
                    )
                    .await;
                } else {
                    tracing::warn!(
                        "📤 Actor '{}' 执行失败: run={}, error={:?}",
                        self.name,
                        run_id,
                        result.error
                    );

                    self.emit_with_trace(
                        AgentEventData::NodeFailed {
                            run_id: run_id.clone(),
                            node_name: self.name.clone(),
                            error: result.error.clone().unwrap_or_default(),
                            duration_ms,
                        },
                        trace_id.as_deref(),
                        session_id.as_deref(),
                    )
                    .await;
                }
            }
            _ => {}
        }
    }

    fn name(&self) -> &str {
        "EventDrivenActor"
    }
}

impl EventDrivenActor {
    /// 带 trace_id 上下文发布事件（trace_id 为空时降级为普通 emit）
    async fn emit_with_trace(
        &self,
        data: AgentEventData,
        trace_id: Option<&str>,
        session_id: Option<&str>,
    ) {
        match trace_id {
            Some(tid) if !tid.is_empty() => {
                self.event_bus
                    .emit_with_trace(data, tid.to_string(), session_id.map(|s| s.to_string()))
                    .await;
            }
            _ => {
                self.event_bus.emit(data).await;
            }
        }
    }
}
