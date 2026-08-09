//! # SupervisorActor - 监督 Actor
//!
//! 管理一组 NodeActor 的生命周期，提供故障恢复策略。
//!
//! ## 监督策略（参考 Erlang/OTP）
//!
//! | 策略 | 行为 |
//! |------|------|
//! | `Restart` | 重启失败的 Actor（限 max_restarts/within） |
//! | `Resume`  | 忽略错误，Actor 继续运行 |
//! | `Stop`    | 停止整个图执行 |
//!
//! ## 架构
//!
//! ```text
//! ┌──────────────────────────────────────┐
//! │           Supervisor                 │
//! │  ┌──────┐ ┌──────┐ ┌──────┐         │
//! │  │ActorA│ │ActorB│ │ActorC│  ...    │
//! │  └──┬───┘ └──┬───┘ └──┬───┘         │
//! │     │        │        │              │
//! │     └────────┴────────┘              │
//! │              │                       │
//! │     故障检测 + 恢复策略               │
//! └──────────────────────────────────────┘
//! ```

use super::super::node::NodeFn;
use super::message::{ActorHandle, ActorStats, NodeMessage};
use super::node_actor::NodeActor;
use crate::event::{AgentEventData, EventBus};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 监督策略
#[derive(Debug, Clone)]
pub enum SupervisionStrategy {
    /// 重启失败的 Actor
    Restart {
        /// 时间窗口内最大重启次数
        max_restarts: usize,
        /// 时间窗口
        within: Duration,
    },
    /// 忽略错误，Actor 继续运行
    Resume,
    /// 停止整个图执行
    Stop,
}

impl Default for SupervisionStrategy {
    fn default() -> Self {
        Self::Restart {
            max_restarts: 3,
            within: Duration::from_secs(60),
        }
    }
}

/// Actor 重启记录
#[derive(Debug, Default)]
struct RestartRecord {
    timestamps: Vec<Instant>,
}

impl RestartRecord {
    /// 记录一次重启，返回是否超出限制
    fn record_and_check(&mut self, max: usize, within: Duration) -> bool {
        let now = Instant::now();
        self.timestamps.retain(|t| now.duration_since(*t) < within);
        self.timestamps.push(now);
        self.timestamps.len() > max
    }
}

/// Supervisor - 管理 NodeActor 集合
pub struct Supervisor {
    /// 监督策略
    strategy: SupervisionStrategy,
    /// 管理的 Actor 句柄
    actors: HashMap<String, ActorHandle>,
    /// 用于重启的节点函数副本
    factories: HashMap<String, NodeFn>,
    /// 每个 Actor 的重试配置
    retry_configs: HashMap<String, u32>,
    /// 重启记录
    restart_records: HashMap<String, RestartRecord>,
    /// 事件总线
    event_bus: Option<Arc<EventBus>>,
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("strategy", &self.strategy)
            .field("actor_count", &self.actors.len())
            .field("actors", &self.actors.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl Supervisor {
    /// 创建 Supervisor
    pub fn new(strategy: SupervisionStrategy) -> Self {
        Self {
            strategy,
            actors: HashMap::new(),
            factories: HashMap::new(),
            retry_configs: HashMap::new(),
            restart_records: HashMap::new(),
            event_bus: None,
        }
    }

    /// 设置事件总线
    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// 注册并启动一个 NodeActor（默认不重试）
    pub fn spawn(&mut self, name: impl Into<String>, func: NodeFn) -> &ActorHandle {
        self.spawn_with_retry(name, func, 0)
    }

    /// 注册并启动一个 NodeActor（带重试配置）
    pub fn spawn_with_retry(
        &mut self,
        name: impl Into<String>,
        func: NodeFn,
        max_retry: u32,
    ) -> &ActorHandle {
        let name = name.into();
        let handle = if let Some(ref bus) = self.event_bus {
            NodeActor::spawn_with_event_bus_and_retry(
                name.clone(),
                func.clone(),
                bus.clone(),
                max_retry,
            )
        } else {
            NodeActor::spawn_with_retry(name.clone(), func.clone(), max_retry)
        };

        self.factories.insert(name.clone(), func);
        self.retry_configs.insert(name.clone(), max_retry);
        self.actors.insert(name.clone(), handle);
        self.actors.get(&name).unwrap()
    }

    /// 获取 Actor 句柄
    pub fn get(&self, name: &str) -> Option<&ActorHandle> {
        self.actors.get(name)
    }

    /// 列出所有 Actor 名称
    pub fn actor_names(&self) -> Vec<String> {
        self.actors.keys().cloned().collect()
    }

    /// Actor 数量
    pub fn len(&self) -> usize {
        self.actors.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.actors.is_empty()
    }

    /// 处理 Actor 失败，返回是否应继续执行
    pub async fn handle_failure(&mut self, actor_name: &str, error: &str) -> bool {
        self.emit(AgentEventData::AgentFailed {
            agent_id: actor_name.to_string(),
            error: error.to_string(),
            duration_ms: 0,
        })
        .await;

        match &self.strategy {
            SupervisionStrategy::Resume => {
                tracing::warn!(
                    "⚠️ Actor '{}' failed, Resume strategy: continuing",
                    actor_name
                );
                true
            }
            SupervisionStrategy::Stop => {
                tracing::error!("⛔ Actor '{}' failed, Stop strategy: halting", actor_name);
                false
            }
            SupervisionStrategy::Restart {
                max_restarts,
                within,
            } => {
                let record = self
                    .restart_records
                    .entry(actor_name.to_string())
                    .or_default();

                if record.record_and_check(*max_restarts, *within) {
                    tracing::error!(
                        "💀 Actor '{}' exceeded max_restarts ({}) within {:?}, giving up",
                        actor_name,
                        max_restarts,
                        within
                    );
                    return false;
                }

                // 重启 Actor
                if let Some(func) = self.factories.get(actor_name).cloned() {
                    tracing::info!("🔄 Restarting Actor '{}'", actor_name);
                    let max_retry = self.retry_configs.get(actor_name).copied().unwrap_or(0);
                    let new_handle = if let Some(ref bus) = self.event_bus {
                        NodeActor::spawn_with_event_bus_and_retry(
                            actor_name,
                            func,
                            bus.clone(),
                            max_retry,
                        )
                    } else {
                        NodeActor::spawn_with_retry(actor_name.to_string(), func, max_retry)
                    };
                    self.actors.insert(actor_name.to_string(), new_handle);
                    true
                } else {
                    tracing::error!(
                        "❌ Cannot restart Actor '{}': factory not found",
                        actor_name
                    );
                    false
                }
            }
        }
    }

    /// 收集所有 Actor 统计信息
    pub async fn collect_stats(&self) -> Vec<ActorStats> {
        let mut stats = Vec::new();
        for handle in self.actors.values() {
            let (tx, rx) = tokio::sync::oneshot::channel();
            if handle
                .addr
                .send(NodeMessage::GetStats { reply: tx })
                .await
                .is_ok()
            {
                if let Ok(s) = rx.await {
                    stats.push(s);
                }
            }
        }
        stats
    }

    /// 终止所有 Actor
    pub async fn shutdown(&self) {
        tracing::info!("🛑 Supervisor shutting down {} actors", self.actors.len());
        for handle in self.actors.values() {
            let _ = handle.addr.send(NodeMessage::Terminate).await;
        }
    }

    async fn emit(&self, data: AgentEventData) {
        if let Some(ref bus) = self.event_bus {
            bus.emit(data).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::node::NodeResult;
    use crate::graph::state::GraphState;

    #[tokio::test]
    async fn test_supervisor_spawn_and_execute() {
        let mut supervisor = Supervisor::new(SupervisionStrategy::default());

        supervisor.spawn(
            "node_a",
            NodeFn::new(|_state| async { NodeResult::ok("result_a") }),
        );

        assert_eq!(supervisor.len(), 1);

        let handle = supervisor.get("node_a").unwrap();
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
        assert_eq!(result.output, "result_a");
    }

    #[tokio::test]
    async fn test_supervisor_restart_strategy() {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Restart {
            max_restarts: 2,
            within: Duration::from_secs(60),
        });

        supervisor.spawn(
            "failing_node",
            NodeFn::new(|_state| async { NodeResult::err("boom") }),
        );

        let should_continue = supervisor.handle_failure("failing_node", "boom").await;
        assert!(should_continue);

        let should_continue = supervisor.handle_failure("failing_node", "boom").await;
        assert!(should_continue);

        let should_continue = supervisor.handle_failure("failing_node", "boom").await;
        assert!(!should_continue);
    }

    #[tokio::test]
    async fn test_supervisor_resume_strategy() {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Resume);

        supervisor.spawn(
            "node_a",
            NodeFn::new(|_state| async { NodeResult::ok("ok") }),
        );

        let should_continue = supervisor.handle_failure("node_a", "error").await;
        assert!(should_continue);
    }

    #[tokio::test]
    async fn test_supervisor_stop_strategy() {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Stop);

        supervisor.spawn(
            "node_a",
            NodeFn::new(|_state| async { NodeResult::ok("ok") }),
        );

        let should_continue = supervisor.handle_failure("node_a", "error").await;
        assert!(!should_continue);
    }

    #[tokio::test]
    async fn test_collect_stats() {
        let mut supervisor = Supervisor::new(SupervisionStrategy::default());

        supervisor.spawn(
            "node_a",
            NodeFn::new(|_state| async { NodeResult::ok("ok") }),
        );

        let handle = supervisor.get("node_a").unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        handle
            .addr
            .send(NodeMessage::Execute {
                state: GraphState::new(),
                reply: tx,
            })
            .await
            .unwrap();
        let _ = rx.await;

        let stats = supervisor.collect_stats().await;
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].name, "node_a");
        assert_eq!(stats[0].exec_count, 1);
        assert_eq!(stats[0].success_count, 1);
    }
}
