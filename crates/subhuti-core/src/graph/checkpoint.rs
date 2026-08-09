//! # 检查点（Checkpoint）
//!
//! 支持断点续跑：图执行每一步后保存状态，失败后可从检查点恢复。

use super::state::GraphState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 单个检查点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// 检查点 ID
    pub id: String,
    /// 图执行 ID
    pub run_id: String,
    /// 已完成的节点名
    pub completed_node: String,
    /// 下一个待执行的节点
    pub next_node: Option<String>,
    /// 执行到此处时的状态快照
    pub state: GraphState,
    /// 创建时间
    pub timestamp: DateTime<Utc>,
    /// 执行步数
    pub step: usize,
}

/// 检查点存储 trait
#[async_trait::async_trait]
pub trait CheckpointStore: Send + Sync {
    /// 保存检查点
    async fn save(&self, checkpoint: Checkpoint) -> anyhow::Result<()>;

    /// 获取最近检查点
    async fn get_latest(&self, run_id: &str) -> Option<Checkpoint>;

    /// 列出所有检查点
    async fn list(&self, run_id: &str) -> Vec<Checkpoint>;

    /// 清除指定 run 的检查点
    async fn clear(&self, run_id: &str);
}

/// 内存版检查点存储
#[derive(Debug, Default)]
pub struct MemoryCheckpointStore {
    checkpoints: Arc<RwLock<HashMap<String, Vec<Checkpoint>>>>,
}

impl MemoryCheckpointStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl CheckpointStore for MemoryCheckpointStore {
    async fn save(&self, checkpoint: Checkpoint) -> anyhow::Result<()> {
        let mut store = self.checkpoints.write().await;
        store
            .entry(checkpoint.run_id.clone())
            .or_default()
            .push(checkpoint);
        Ok(())
    }

    async fn get_latest(&self, run_id: &str) -> Option<Checkpoint> {
        let store = self.checkpoints.read().await;
        store.get(run_id).and_then(|v| v.last().cloned())
    }

    async fn list(&self, run_id: &str) -> Vec<Checkpoint> {
        let store = self.checkpoints.read().await;
        store.get(run_id).cloned().unwrap_or_default()
    }

    async fn clear(&self, run_id: &str) {
        let mut store = self.checkpoints.write().await;
        store.remove(run_id);
    }
}
