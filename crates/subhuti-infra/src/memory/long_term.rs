//! # LongTermMemory - 长期归档记忆
//!
//! 历史对话沉淀，不会自动进上下文，AI 必须主动调用工具搜索

use super::{MemoryItem, MemoryStore, SearchResult};
use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;

/// 长期归档记忆
#[derive(Debug)]
pub struct LongTermMemory {
    /// 记忆列表
    items: Vec<MemoryItem>,
    /// 索引: session_id -> Vec<index>
    session_index: HashMap<String, Vec<usize>>,
    /// 关键词索引
    keyword_index: HashMap<String, Vec<usize>>,
}

impl LongTermMemory {
    /// 创建新的长期记忆
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            session_index: HashMap::new(),
            keyword_index: HashMap::new(),
        }
    }

    /// 添加记忆
    pub fn add(&mut self, item: MemoryItem) {
        let idx = self.items.len();
        self.items.push(item.clone());

        // 更新会话索引
        if let Some(ref session_id) = item.session_id {
            self.session_index
                .entry(session_id.clone())
                .or_default()
                .push(idx);
        }

        // 更新关键词索引
        for word in item.content.split_whitespace() {
            if word.len() > 2 {
                self.keyword_index
                    .entry(word.to_lowercase())
                    .or_default()
                    .push(idx);
            }
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

    /// 按时间范围获取记忆
    pub fn get_by_time_range(
        &self,
        _start: chrono::DateTime<Utc>,
        _end: chrono::DateTime<Utc>,
    ) -> Vec<MemoryItem> {
        // TODO: 实现时间范围查询
        self.items.clone()
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

impl Default for LongTermMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStore for LongTermMemory {
    fn write(&self, item: MemoryItem) -> Result<()> {
        // LongTermMemory 使用 add 方法
        let _ = item;
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
                score: 1.0,
            })
            .collect()
    }

    fn get_all(&self) -> Vec<MemoryItem> {
        self.items.clone()
    }

    fn clear(&mut self) -> Result<()> {
        self.items.clear();
        self.session_index.clear();
        self.keyword_index.clear();
        Ok(())
    }
}
