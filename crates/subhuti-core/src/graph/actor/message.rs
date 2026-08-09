//! # Actor 消息协议
//!
//! 定义 NodeActor 之间、Supervisor 与 NodeActor 之间的消息类型。
//!
//! ## 消息流
//!
//! ```text
//! Graph/Engine                 NodeActor
//!     │                           │
//!     ├── Execute {state} ──────► │
//!     │                       执行节点函数
//!     │◄── NodeResult ──────────┤
//!     │                           │
//!     ├── HealthCheck ─────────► │
//!     │◄── ActorHealth ─────────┤
//!     │                           │
//!     └── Terminate ───────────► │ (停止)
//! ```

use super::super::node::NodeResult;
use super::super::state::GraphState;
use tokio::sync::{mpsc, oneshot};

/// 节点 Actor 消息
pub enum NodeMessage {
    /// 执行节点
    Execute {
        state: GraphState,
        reply: oneshot::Sender<NodeResult>,
    },
    /// 健康检查
    HealthCheck { reply: oneshot::Sender<ActorHealth> },
    /// 获取执行统计
    GetStats { reply: oneshot::Sender<ActorStats> },
    /// 终止 Actor
    Terminate,
}

/// Actor 地址（邮箱发送端）
pub type ActorAddr = mpsc::Sender<NodeMessage>;

/// Actor 健康状态
#[derive(Debug, Clone)]
pub enum ActorHealth {
    /// 空闲
    Idle,
    /// 执行中
    Busy,
    /// 已失败
    Failed(String),
    /// 已停止
    Stopped,
}

/// Actor 执行统计
#[derive(Debug, Clone, Default)]
pub struct ActorStats {
    /// 节点名称
    pub name: String,
    /// 总执行次数
    pub exec_count: u64,
    /// 成功次数
    pub success_count: u64,
    /// 失败次数
    pub failure_count: u64,
    /// 总耗时（毫秒）
    pub total_duration_ms: u64,
    /// 当前状态
    pub state: ActorLifecycle,
}

/// Actor 生命周期状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActorLifecycle {
    #[default]
    Idle,
    Running,
    Stopped,
}

impl std::fmt::Display for ActorLifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
        }
    }
}

/// Actor 句柄（地址 + 元信息）
#[derive(Clone)]
pub struct ActorHandle {
    /// Actor 地址
    pub addr: ActorAddr,
    /// 节点名称
    pub name: String,
}

impl std::fmt::Debug for ActorHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorHandle")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
