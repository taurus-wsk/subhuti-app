//! # NodeActor - 节点 Actor
//!
//! 将图节点封装为独立 Actor，实现：
//!
//! - **状态隔离**：每个 Actor 有私有状态，无共享数据竞争
//! - **邮箱模型**：异步消息处理，天然背压控制
//! - **执行统计**：记录执行次数、成功率、耗时
//! - **生命周期管理**：Idle → Running → Idle / Stopped

use super::super::node::{NodeFn, NodeResult};
use super::super::state::GraphState;
use super::message::{ActorHandle, ActorHealth, ActorLifecycle, ActorStats, NodeMessage};
use crate::event::{AgentEventData, EventBus};
use std::sync::Arc;
use tokio::sync::mpsc;

/// NodeActor 邮箱容量
const MAILBOX_CAPACITY: usize = 64;

/// 节点 Actor - 封装图节点为独立计算单元
pub struct NodeActor {
    /// 节点名称
    name: String,
    /// 节点执行函数（可克隆，支持重启）
    func: NodeFn,
    /// 邮箱接收端
    mailbox: mpsc::Receiver<NodeMessage>,
    /// 事件总线（可选）
    event_bus: Option<Arc<EventBus>>,
    /// 生命周期状态
    lifecycle: ActorLifecycle,
    /// 执行统计
    exec_count: u64,
    success_count: u64,
    failure_count: u64,
    total_duration_ms: u64,
    /// 最大重试次数（0 表示不重试）
    max_retry: u32,
}

impl std::fmt::Debug for NodeActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeActor")
            .field("name", &self.name)
            .field("lifecycle", &self.lifecycle)
            .field("exec_count", &self.exec_count)
            .field("success_count", &self.success_count)
            .field("failure_count", &self.failure_count)
            .field("max_retry", &self.max_retry)
            .finish_non_exhaustive()
    }
}

impl NodeActor {
    /// 创建 Actor 及其地址句柄（默认不重试）
    pub fn spawn(name: impl Into<String>, func: NodeFn) -> ActorHandle {
        Self::spawn_with_retry(name, func, 0)
    }

    /// 创建带重试配置的 Actor
    pub fn spawn_with_retry(name: impl Into<String>, func: NodeFn, max_retry: u32) -> ActorHandle {
        Self::spawn_internal(name, func, MAILBOX_CAPACITY, None, max_retry)
    }

    /// 创建带事件总线的 Actor（默认不重试）
    pub fn spawn_with_event_bus(
        name: impl Into<String>,
        func: NodeFn,
        bus: Arc<EventBus>,
    ) -> ActorHandle {
        Self::spawn_with_event_bus_and_retry(name, func, bus, 0)
    }

    /// 创建带事件总线和重试配置的 Actor
    pub fn spawn_with_event_bus_and_retry(
        name: impl Into<String>,
        func: NodeFn,
        bus: Arc<EventBus>,
        max_retry: u32,
    ) -> ActorHandle {
        Self::spawn_internal(name, func, MAILBOX_CAPACITY, Some(bus), max_retry)
    }

    /// 内部创建方法
    fn spawn_internal(
        name: impl Into<String>,
        func: NodeFn,
        capacity: usize,
        event_bus: Option<Arc<EventBus>>,
        max_retry: u32,
    ) -> ActorHandle {
        let name = name.into();
        let (tx, rx) = mpsc::channel(capacity);
        let handle = ActorHandle {
            addr: tx,
            name: name.clone(),
        };

        let actor = Self {
            name,
            func,
            mailbox: rx,
            event_bus,
            lifecycle: ActorLifecycle::Idle,
            exec_count: 0,
            success_count: 0,
            failure_count: 0,
            total_duration_ms: 0,
            max_retry,
        };

        tokio::spawn(actor.run());
        handle
    }

    /// Actor 主循环 - 消费邮箱消息
    async fn run(mut self) {
        tracing::debug!("🎬 Actor '{}' started", self.name);

        while let Some(msg) = self.mailbox.recv().await {
            match msg {
                NodeMessage::Execute { state, reply } => {
                    self.handle_execute(state, reply).await;
                }
                NodeMessage::HealthCheck { reply } => {
                    let health = match self.lifecycle {
                        ActorLifecycle::Idle => ActorHealth::Idle,
                        ActorLifecycle::Running => ActorHealth::Busy,
                        ActorLifecycle::Stopped => ActorHealth::Stopped,
                    };
                    let _ = reply.send(health);
                }
                NodeMessage::GetStats { reply } => {
                    let stats = ActorStats {
                        name: self.name.clone(),
                        exec_count: self.exec_count,
                        success_count: self.success_count,
                        failure_count: self.failure_count,
                        total_duration_ms: self.total_duration_ms,
                        state: self.lifecycle,
                    };
                    let _ = reply.send(stats);
                }
                NodeMessage::Terminate => {
                    tracing::info!("🛑 Actor '{}' terminating", self.name);
                    self.lifecycle = ActorLifecycle::Stopped;
                    break;
                }
            }
        }

        tracing::debug!(
            "🏁 Actor '{}' stopped (execs={}, ok={}, fail={})",
            self.name,
            self.exec_count,
            self.success_count,
            self.failure_count
        );
    }

    /// 处理执行请求（带重试）
    async fn handle_execute(
        &mut self,
        state: GraphState,
        reply: tokio::sync::oneshot::Sender<NodeResult>,
    ) {
        self.lifecycle = ActorLifecycle::Running;
        self.exec_count += 1;

        // 预先 clone trace_id/session_id（execute_with_retry 会 consume state）
        let trace_id = state.get("trace_id");
        let session_id = state.get("session_id");

        // 发布 Actor 开始事件
        self.emit_with_optional_trace(
            AgentEventData::FlowStarted {
                flow_type: format!("actor:{}", self.name),
                input: state.to_json().unwrap_or_default(),
            },
            trace_id.clone(),
            session_id.clone(),
        )
        .await;

        // 执行节点（带重试）
        let start = std::time::Instant::now();
        let mut result = self.execute_with_retry(state).await;
        let duration_ms = start.elapsed().as_millis() as u64;
        result.duration_ms = duration_ms;
        self.total_duration_ms += duration_ms;

        // 统计
        if result.success {
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }

        // 发布 Actor 完成事件（复用预先取出的 trace_id）
        self.emit_with_optional_trace(
            AgentEventData::FlowStepExecuted {
                step_index: self.exec_count as usize - 1,
                step_name: self.name.clone(),
                result: result.output.clone(),
            },
            trace_id,
            session_id,
        )
        .await;

        let _ = reply.send(result);
        self.lifecycle = ActorLifecycle::Idle;
    }

    /// 执行节点 + 指数退避重试（从调度器下沉到这里）
    async fn execute_with_retry(&self, state: GraphState) -> NodeResult {
        let mut last_result: Option<NodeResult> = None;

        for attempt in 0..=self.max_retry {
            let result = self.func.call(state.clone()).await;

            if result.success {
                if attempt > 0 {
                    tracing::info!(
                        "✅ Actor '{}' 重试成功 (attempt {}/{})",
                        self.name,
                        attempt,
                        self.max_retry
                    );
                }
                return result;
            }

            last_result = Some(result);

            if attempt < self.max_retry {
                let delay = std::time::Duration::from_millis(2u64.pow(attempt) * 100);
                tracing::warn!(
                    "❌ Actor '{}' 执行失败，将在 {:?} 后重试 (attempt {}/{})",
                    self.name,
                    delay,
                    attempt,
                    self.max_retry
                );
                tokio::time::sleep(delay).await;
            }
        }

        let final_result = last_result.unwrap_or_else(|| NodeResult::err("未知错误"));
        tracing::error!(
            "💀 Actor '{}' 达到最大重试次数 ({})，放弃执行",
            self.name,
            self.max_retry
        );
        final_result
    }

    /// 发布事件（带预先取出的 trace/session，避免 GraphState 被 consume）
    async fn emit_with_optional_trace(
        &self,
        data: AgentEventData,
        trace_id: Option<String>,
        session_id: Option<String>,
    ) {
        if let Some(ref bus) = self.event_bus {
            match trace_id {
                Some(tid) if !tid.is_empty() => {
                    bus.emit_with_trace(data, tid, session_id).await;
                }
                _ => bus.emit(data).await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::node::NodeResult;
    use crate::graph::state::GraphState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_actor_with_retry_success_after_retry() {
        let attempt_count = Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt_count.clone();

        let handle = NodeActor::spawn_with_retry(
            "retry_node",
            NodeFn::new(move |_state| {
                let count = attempt_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    if count < 2 {
                        NodeResult::err("暂时失败")
                    } else {
                        NodeResult::ok("成功")
                    }
                }
            }),
            3,
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .addr
            .send(NodeMessage::Execute {
                state: GraphState::new(),
                reply: tx,
            })
            .await
            .unwrap();

        let result = rx.await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "成功");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_actor_with_retry_exceed_max() {
        let attempt_count = Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt_count.clone();

        let handle = NodeActor::spawn_with_retry(
            "fail_node",
            NodeFn::new(move |_state| {
                let _count = attempt_clone.fetch_add(1, Ordering::SeqCst);
                async move { NodeResult::err("一直失败") }
            }),
            2,
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .addr
            .send(NodeMessage::Execute {
                state: GraphState::new(),
                reply: tx,
            })
            .await
            .unwrap();

        let result = rx.await.unwrap();
        assert!(!result.success);
        assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_actor_no_retry() {
        let attempt_count = Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt_count.clone();

        let handle = NodeActor::spawn(
            "no_retry_node",
            NodeFn::new(move |_state| {
                let _count = attempt_clone.fetch_add(1, Ordering::SeqCst);
                async move { NodeResult::err("失败") }
            }),
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .addr
            .send(NodeMessage::Execute {
                state: GraphState::new(),
                reply: tx,
            })
            .await
            .unwrap();

        let result = rx.await.unwrap();
        assert!(!result.success);
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_actor_immediate_success() {
        let attempt_count = Arc::new(AtomicUsize::new(0));
        let attempt_clone = attempt_count.clone();

        let handle = NodeActor::spawn_with_retry(
            "success_node",
            NodeFn::new(move |_state| {
                let _count = attempt_clone.fetch_add(1, Ordering::SeqCst);
                async move { NodeResult::ok("立即成功") }
            }),
            3,
        );

        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .addr
            .send(NodeMessage::Execute {
                state: GraphState::new(),
                reply: tx,
            })
            .await
            .unwrap();

        let result = rx.await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "立即成功");
        assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
    }
}
