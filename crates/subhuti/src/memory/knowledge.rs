//! # KnowledgeMemory - 知识库语义记忆
//!
//! 向量知识、外部文档，向量检索
//!
//! 注意：这是一个简化实现，实际项目中可以使用:
//!   - tantivy (Rust 原生全文搜索引擎)
//!   - meilisearch
//!   - chromadb
//!   - qdrant
//!
//! 等专业向量数据库

use super::{MemoryItem, MemoryStore, SearchResult};
use anyhow::Result;
use std::collections::HashMap;

/// 简化向量表示 (用于演示，实际应使用专业向量数据库)
#[derive(Debug, Clone)]
struct SimpleVector {
    /// 向量数据 (简化：用词袋模型)
    words: HashMap<String, f32>,
}

impl SimpleVector {
    /// 从文本创建简化向量
    fn from_text(text: &str, _dim: usize) -> Self {
        let mut words = HashMap::new();
        let word_count = text.split_whitespace().count() as f32;

        for word in text.split_whitespace() {
            let word_lower = word.to_lowercase();
            *words.entry(word_lower).or_insert(0.0) += 1.0;
        }

        // 归一化
        if word_count > 0.0 {
            for value in words.values_mut() {
                *value /= word_count;
            }
        }

        Self { words }
    }

    /// 计算余弦相似度 (简化)
    fn cosine_similarity(&self, other: &SimpleVector) -> f32 {
        let mut dot_product = 0.0;
        let mut norm_a = 0.0;
        let mut norm_b = 0.0;

        for (word, &value_a) in &self.words {
            if let Some(&value_b) = other.words.get(word) {
                dot_product += value_a * value_b;
            }
            norm_a += value_a * value_a;
        }

        for &value_b in other.words.values() {
            norm_b += value_b * value_b;
        }

        if norm_a > 0.0 && norm_b > 0.0 {
            dot_product / (norm_a.sqrt() * norm_b.sqrt())
        } else {
            0.0
        }
    }
}

/// 知识库记忆
#[derive(Debug)]
pub struct KnowledgeMemory {
    /// 向量维度
    dim: usize,
    /// 记忆列表
    items: Vec<MemoryItem>,
    /// 向量存储
    vectors: Vec<SimpleVector>,
}

impl KnowledgeMemory {
    /// 创建新的知识库记忆
    pub fn new(dim: usize) -> Self {
        Self {
            dim,
            items: Vec::new(),
            vectors: Vec::new(),
        }
    }

    /// 添加知识
    pub fn add(&mut self, item: MemoryItem) {
        let vector = SimpleVector::from_text(&item.content, self.dim);
        self.vectors.push(vector);
        self.items.push(item);
    }

    /// 语义搜索
    pub fn semantic_search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_vector = SimpleVector::from_text(query, self.dim);

        let mut results: Vec<_> = self
            .vectors
            .iter()
            .enumerate()
            .map(|(idx, v)| {
                let score = v.cosine_similarity(&query_vector);
                SearchResult {
                    item: self.items[idx].clone(),
                    score,
                }
            })
            .collect();

        // 按分数排序
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.into_iter().take(limit).collect()
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

impl Default for KnowledgeMemory {
    fn default() -> Self {
        Self::new(384)
    }
}

impl MemoryStore for KnowledgeMemory {
    fn write(&self, item: MemoryItem) -> Result<()> {
        // KnowledgeMemory 使用 add 方法
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
        self.semantic_search(query, limit)
    }

    fn get_all(&self) -> Vec<MemoryItem> {
        self.items.clone()
    }

    fn clear(&mut self) -> Result<()> {
        self.items.clear();
        self.vectors.clear();
        Ok(())
    }
}
