//! # ShortTermMemory - 短期工作记忆
//!
//! 当前对话上下文，默认自动注入 LLM，超限自动归档

use super::{MemoryItem, MemoryStore, SearchResult};
use anyhow::Result;
use std::collections::HashMap;

/// 短期工作记忆
#[derive(Debug)]
pub struct ShortTermMemory {
    /// 记忆容量
    capacity: usize,
    /// 记忆列表
    items: Vec<MemoryItem>,
    /// 会话索引: session_id -> Vec<index>
    session_index: HashMap<String, Vec<usize>>,
}

impl ShortTermMemory {
    /// 创建新的短期记忆
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            items: Vec::with_capacity(capacity + 1),
            session_index: HashMap::new(),
        }
    }

    /// 添加记忆
    pub fn add(&mut self, item: MemoryItem) {
        // 如果超过容量，移除最旧的
        if self.items.len() >= self.capacity {
            self.items.remove(0);
        }

        let idx = self.items.len();
        self.items.push(item.clone());

        // 更新会话索引
        if let Some(ref session_id) = item.session_id {
            self.session_index
                .entry(session_id.clone())
                .or_default()
                .push(idx);
        }
    }

    /// 获取指定会话的所有记忆
    pub fn get_session(&self, session_id: &str) -> Vec<MemoryItem> {
        if let Some(indices) = self.session_index.get(session_id) {
            indices
                .iter()
                .filter_map(|&idx| self.items.get(idx).cloned())
                .collect()
        } else {
            Vec::new()
        }
    }

    /// 排出指定会话的记忆（用于归档）
    pub fn drain_session(&mut self, session_id: &str) -> Vec<MemoryItem> {
        if let Some(indices) = self.session_index.remove(session_id) {
            // 按索引倒序删除
            let mut removed = Vec::new();
            for idx in indices {
                if idx < self.items.len() {
                    removed.push(self.items.remove(idx));
                }
            }
            removed
        } else {
            Vec::new()
        }
    }

    /// 裁剪保留指定数量
    pub fn prune(&mut self, keep_count: usize) -> Vec<MemoryItem> {
        if self.items.len() <= keep_count {
            return Vec::new();
        }

        let removed: Vec<_> = self.items.drain(0..self.items.len() - keep_count).collect();
        self.session_index.clear();
        // 重建索引
        for (idx, item) in self.items.iter().enumerate() {
            if let Some(ref session_id) = item.session_id {
                self.session_index
                    .entry(session_id.clone())
                    .or_default()
                    .push(idx);
            }
        }
        removed
    }

    /// 生成摘要
    pub fn summarize(&self) -> String {
        if self.items.is_empty() {
            return "No short-term memories".to_string();
        }

        let count = self.items.len();
        let first = self.items.first().map(|i| i.content.as_str()).unwrap_or("");
        let last = self.items.last().map(|i| i.content.as_str()).unwrap_or("");

        format!("{} messages, from '{}' to '{}'", count, first, last)
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 获取长度
    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl MemoryStore for ShortTermMemory {
    fn write(&self, _item: MemoryItem) -> Result<()> {
        // ShortTermMemory 使用 add 方法
        Ok(())
    }

    fn read(&self, id: &str) -> Option<MemoryItem> {
        self.items.iter().find(|item| item.id == id).cloned()
    }

    fn delete(&self, _id: &str) -> Result<()> {
        Ok(())
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        self.items
            .iter()
            .filter(|item| item.content.to_lowercase().contains(&query_lower))
            .take(limit)
            .map(|item| SearchResult {
                item: item.clone(),
                score: 1.0, // 简单匹配，分数为1
            })
            .collect()
    }

    fn get_all(&self) -> Vec<MemoryItem> {
        self.items.clone()
    }

    fn clear(&mut self) -> Result<()> {
        self.items.clear();
        self.session_index.clear();
        Ok(())
    }
}
