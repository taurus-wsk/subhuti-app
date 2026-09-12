//! # Memory Layer - 记忆层
//!
//! 职责：所有数据存储、检索、归档、分层治理
//!
//! ## 三层标准记忆
//!
//! - **短期工作记忆 (Session)**: 当前对话上下文，默认自动注入 LLM
//! - **长期归档记忆 (Archive)**: 历史对话沉淀，AI 主动调用搜索
//! - **知识库语义记忆 (Knowledge)**: 向量知识、外部文档，向量检索

pub mod convo_miner;
mod dedup;
mod dynamics;
mod embedding;
pub mod entities;
pub mod fact_checker;
mod knowledge;
pub mod layers;
mod long_term;
mod short_term;

pub use convo_miner::{ConvoExchange, ConvoMiner, MinedMemory};
pub use dedup::{DedupConfig, DedupResult, Deduplicator, KeepStrategy};
pub use dynamics::ConnectionDynamics;
pub use embedding::{EmbeddingConfig, EmbeddingService};
pub use entities::{Entity, EntityExtractor, EntityRegistry, EntitySource, EntityType};
pub use fact_checker::{FactChecker, FactIssue, IssueType};
pub use knowledge::KnowledgeMemory;
pub use layers::{LayerOutput, MemoryLayerConfig, MemoryStack};
pub use long_term::LongTermMemory;
pub use short_term::ShortTermMemory;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    pub short_term_capacity: usize,
    pub archive_threshold: usize,
    pub knowledge_dim: usize,
    pub ttl_seconds: Option<u64>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            short_term_capacity: 10,
            archive_threshold: 20,
            knowledge_dim: 384,
            ttl_seconds: Some(3600 * 24 * 7),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
    pub layer: MemoryLayer,
    pub session_id: Option<String>,
}

impl MemoryItem {
    pub fn new(content: String, layer: MemoryLayer, session_id: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content,
            created_at: Utc::now(),
            metadata: HashMap::new(),
            layer,
            session_id,
        }
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.created_at);
        duration.num_seconds() > ttl_seconds as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    ShortTerm,
    Archive,
    Knowledge,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResult {
    pub item: MemoryItem,
    pub score: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SemanticSearchResult {
    pub content: String,
    pub similarity: f32,
    pub layer: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

pub trait MemoryStore: Send + Sync {
    fn write(&self, item: MemoryItem) -> Result<()>;
    fn read(&self, id: &str) -> Option<MemoryItem>;
    fn delete(&self, id: &str) -> Result<()>;
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResult>;
    fn get_all(&self) -> Vec<MemoryItem>;
    fn clear(&mut self) -> Result<()>;
}

pub struct Memory {
    config: MemoryConfig,
    short_term: Arc<RwLock<ShortTermMemory>>,
    archive: Arc<RwLock<LongTermMemory>>,
    knowledge: Arc<RwLock<KnowledgeMemory>>,
    embedding: RwLock<Option<Arc<EmbeddingService>>>,
    entity_registry: Arc<RwLock<EntityRegistry>>,
    entity_extractor: EntityExtractor,
    convo_miner: ConvoMiner,
    deduplicator: Deduplicator,
    memory_stack: MemoryStack,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("config", &self.config)
            .field("has_embedding", &self.has_embedding())
            .finish()
    }
}

impl Clone for Memory {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            short_term: Arc::clone(&self.short_term),
            archive: Arc::clone(&self.archive),
            knowledge: Arc::clone(&self.knowledge),
            embedding: RwLock::new(self.embedding_service()),
            entity_registry: Arc::clone(&self.entity_registry),
            entity_extractor: self.entity_extractor.clone(),
            convo_miner: self.convo_miner.clone(),
            deduplicator: self.deduplicator.clone(),
            memory_stack: self.memory_stack.clone(),
        }
    }
}

impl Memory {
    pub fn new() -> Self {
        Self::with_config(MemoryConfig::default())
    }

    pub fn with_config(config: MemoryConfig) -> Self {
        Self {
            config: config.clone(),
            short_term: Arc::new(RwLock::new(ShortTermMemory::new(
                config.short_term_capacity,
            ))),
            archive: Arc::new(RwLock::new(LongTermMemory::new())),
            knowledge: Arc::new(RwLock::new(KnowledgeMemory::new(config.knowledge_dim))),
            embedding: RwLock::new(None),
            entity_registry: Arc::new(RwLock::new(EntityRegistry::new())),
            entity_extractor: EntityExtractor::new(),
            convo_miner: ConvoMiner::new(),
            deduplicator: Deduplicator::new(DedupConfig::default()),
            memory_stack: MemoryStack::new(MemoryLayerConfig::default()),
        }
    }

    pub fn with_embedding(self, config: EmbeddingConfig) -> Self {
        *self.embedding.write().unwrap() = Some(Arc::new(EmbeddingService::new(config)));
        self
    }

    pub fn set_embedding(&self, service: Arc<EmbeddingService>) {
        *self.embedding.write().unwrap() = Some(service);
        tracing::info!("Memory: Embedding service connected");
    }

    pub fn embedding_service(&self) -> Option<Arc<EmbeddingService>> {
        self.embedding.read().unwrap().clone()
    }

    pub fn has_embedding(&self) -> bool {
        self.embedding.read().unwrap().is_some()
    }

    pub fn entity_registry(&self) -> &Arc<RwLock<EntityRegistry>> {
        &self.entity_registry
    }

    pub fn entity_extractor(&self) -> &EntityExtractor {
        &self.entity_extractor
    }

    pub fn convo_miner(&self) -> &ConvoMiner {
        &self.convo_miner
    }

    pub fn deduplicator(&self) -> &Deduplicator {
        &self.deduplicator
    }

    pub fn memory_stack(&self) -> &MemoryStack {
        &self.memory_stack
    }

    pub fn write_short_term(&self, content: String, session_id: &str) -> Result<()> {
        let item = MemoryItem::new(
            content.clone(),
            MemoryLayer::ShortTerm,
            Some(session_id.to_string()),
        );
        self.short_term.write().unwrap().add(item);

        if self.short_term.read().unwrap().len() >= self.config.archive_threshold {
            self.archive_from_short_term(session_id)?;
        }
        Ok(())
    }

    pub fn archive_from_short_term(&self, session_id: &str) -> Result<()> {
        let items: Vec<_> = self.short_term.write().unwrap().drain_session(session_id);

        for item in items {
            let mut archive_item = item;
            archive_item.layer = MemoryLayer::Archive;
            self.archive.write().unwrap().add(archive_item);
        }
        Ok(())
    }

    pub fn archive_long_term(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> Result<()> {
        let content = format!("User: {}\nAssistant: {}", user_message, assistant_message);
        let item = MemoryItem::new(content, MemoryLayer::Archive, Some(session_id.to_string()));
        self.archive.write().unwrap().add(item);

        Ok(())
    }

    pub fn search_short_term(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.short_term.read().unwrap().search(query, limit)
    }

    pub fn search_archive(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.archive.read().unwrap().search(query, limit)
    }

    pub fn search_knowledge(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.knowledge.read().unwrap().search(query, limit)
    }

    pub fn add_knowledge(
        &self,
        content: String,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<()> {
        let mut item = MemoryItem::new(content.clone(), MemoryLayer::Knowledge, None);
        if let Some(meta) = metadata {
            item.metadata = meta;
        }
        self.knowledge.write().unwrap().add(item);
        Ok(())
    }

    pub fn prune_short_term(&self, keep_count: usize) -> Vec<MemoryItem> {
        self.short_term.write().unwrap().prune(keep_count)
    }

    pub fn summarize_short_term(&self) -> String {
        self.short_term.read().unwrap().summarize()
    }

    pub fn is_empty(&self) -> bool {
        self.short_term.read().unwrap().is_empty()
            && self.archive.read().unwrap().is_empty()
            && self.knowledge.read().unwrap().is_empty()
    }

    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            short_term_count: self.short_term.read().unwrap().len(),
            archive_count: self.archive.read().unwrap().len(),
            knowledge_count: self.knowledge.read().unwrap().len(),
            total_count: 0,
            zone_counts: std::collections::HashMap::new(),
            importance_counts: std::collections::HashMap::new(),
            avg_strength: 0.0,
            base_stats: BaseStats {
                short_term_count: self.short_term.read().unwrap().len(),
                archive_count: self.archive.read().unwrap().len(),
                knowledge_count: self.knowledge.read().unwrap().len(),
            },
        }
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
    pub short_term_count: usize,
    pub archive_count: usize,
    pub knowledge_count: usize,
    pub total_count: usize,
    pub zone_counts: std::collections::HashMap<String, usize>,
    pub importance_counts: std::collections::HashMap<String, usize>,
    pub avg_strength: f32,
    pub base_stats: BaseStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseStats {
    pub short_term_count: usize,
    pub archive_count: usize,
    pub knowledge_count: usize,
}

impl Default for MemoryItem {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            content: String::new(),
            created_at: Utc::now(),
            metadata: HashMap::new(),
            layer: MemoryLayer::ShortTerm,
            session_id: None,
        }
    }
}

impl subhuti_core::Memory for Memory {
    fn write_short_term(&self, content: &str, tags: Vec<String>) {
        let session_id = tags
            .first()
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        let _ = self.write_short_term(content.to_string(), &session_id);
    }

    fn write_long_term(&self, content: &str, tags: Vec<String>) {
        let session_id = tags
            .first()
            .cloned()
            .unwrap_or_else(|| "default".to_string());
        let _ = self.archive_long_term(&session_id, content, "");
    }

    fn read(&self, id: &str) -> Option<subhuti_core::memory::MemoryItem> {
        let all = self.short_term.read().unwrap().get_all();
        for item in all {
            if item.id == id {
                return Some(subhuti_core::memory::MemoryItem {
                    id: item.id,
                    content: item.content,
                    tags: Vec::new(),
                    metadata: serde_json::Value::Null,
                    created_at: item.created_at,
                });
            }
        }
        None
    }

    fn delete(&self, id: &str) {
        self.short_term.write().unwrap().remove(id);
    }

    fn search(&self, query: &str, limit: usize) -> Vec<subhuti_core::memory::SearchResult> {
        let mut results: Vec<subhuti_core::memory::SearchResult> = Vec::new();

        for sr in self.search_short_term(query, limit) {
            results.push(subhuti_core::memory::SearchResult {
                id: sr.item.id,
                content: sr.item.content,
                score: sr.score,
            });
        }

        for sr in self.search_archive(query, limit) {
            results.push(subhuti_core::memory::SearchResult {
                id: sr.item.id,
                content: sr.item.content,
                score: sr.score,
            });
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    fn get_all(&self) -> Vec<subhuti_core::memory::MemoryItem> {
        let mut items: Vec<subhuti_core::memory::MemoryItem> = Vec::new();

        for item in self.short_term.read().unwrap().get_all() {
            items.push(subhuti_core::memory::MemoryItem {
                id: item.id,
                content: item.content,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
                created_at: item.created_at,
            });
        }

        for item in self.archive.read().unwrap().get_all() {
            items.push(subhuti_core::memory::MemoryItem {
                id: item.id,
                content: item.content,
                tags: Vec::new(),
                metadata: serde_json::Value::Null,
                created_at: item.created_at,
            });
        }

        items
    }

    fn clear(&self) {
        let _ = self.short_term.write().unwrap().clear();
        let _ = self.archive.write().unwrap().clear();
        let _ = self.knowledge.write().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_write_and_search() {
        let memory = Memory::new();
        memory
            .write_short_term("Hello world".to_string(), "session_1")
            .unwrap();

        let results = memory.search_short_term("Hello", 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].item.content, "Hello world");
    }

    #[tokio::test]
    async fn test_memory_stats() {
        let memory = Memory::new();
        memory
            .write_short_term("Test".to_string(), "session_1")
            .unwrap();

        let stats = memory.stats();
        assert_eq!(stats.short_term_count, 1);
    }
}
