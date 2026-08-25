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
pub mod hybrid_search;
mod knowledge;
pub mod knowledge_graph;
pub mod layers;
mod long_term;
mod short_term;
pub mod storage;

pub use convo_miner::{ConvoExchange, ConvoMiner, MinedMemory};
pub use dedup::{DedupConfig, DedupResult, Deduplicator, KeepStrategy};
pub use dynamics::ConnectionDynamics;
pub use embedding::{EmbeddingConfig, EmbeddingService};
pub use entities::{Entity, EntityExtractor, EntityRegistry, EntitySource, EntityType};
pub use fact_checker::{FactChecker, FactIssue, IssueType};
pub use hybrid_search::{HybridSearchResult, HybridSearcher};
pub use knowledge::KnowledgeMemory;
pub use knowledge_graph::{KnowledgeGraph, KnowledgeGraphStats, QueryDirection, Triple};
pub use layers::{LayerOutput, MemoryLayerConfig, MemoryStack};
pub use long_term::LongTermMemory;
pub use short_term::ShortTermMemory;
pub use storage::{
    Database, DbConfig, FeedbackRow, HistoryRow, MemoryRow, PersonaData, PersonaRow,
};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
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

#[async_trait::async_trait]
pub trait DatabaseStore: Send + Sync {
    async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>>;

    async fn execute(&self, sql: &str, params: Vec<serde_json::Value>) -> Result<u64>;
}

pub struct Memory {
    config: MemoryConfig,
    short_term: Arc<RwLock<ShortTermMemory>>,
    archive: Arc<RwLock<LongTermMemory>>,
    knowledge: Arc<RwLock<KnowledgeMemory>>,
    database: RwLock<Option<Arc<Database>>>,
    embedding: RwLock<Option<Arc<EmbeddingService>>>,
    entity_registry: Arc<RwLock<EntityRegistry>>,
    entity_extractor: EntityExtractor,
    convo_miner: ConvoMiner,
    hybrid_searcher: HybridSearcher,
    deduplicator: Deduplicator,
    memory_stack: MemoryStack,
    knowledge_graph: RwLock<Option<Arc<KnowledgeGraph>>>,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("config", &self.config)
            .field("has_database", &self.has_database())
            .field("has_embedding", &self.has_embedding())
            .field("has_knowledge_graph", &self.has_knowledge_graph())
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
            database: RwLock::new(self.database()),
            embedding: RwLock::new(self.embedding_service()),
            entity_registry: Arc::clone(&self.entity_registry),
            entity_extractor: self.entity_extractor.clone(),
            convo_miner: self.convo_miner.clone(),
            hybrid_searcher: self.hybrid_searcher.clone(),
            deduplicator: self.deduplicator.clone(),
            memory_stack: self.memory_stack.clone(),
            knowledge_graph: RwLock::new(self.knowledge_graph()),
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
            database: RwLock::new(None),
            embedding: RwLock::new(None),
            entity_registry: Arc::new(RwLock::new(EntityRegistry::new())),
            entity_extractor: EntityExtractor::new(),
            convo_miner: ConvoMiner::new(),
            hybrid_searcher: HybridSearcher::default(),
            deduplicator: Deduplicator::new(DedupConfig::default()),
            memory_stack: MemoryStack::new(MemoryLayerConfig::default()),
            knowledge_graph: RwLock::new(None),
        }
    }

    pub fn with_database(self, database: Arc<Database>) -> Self {
        *self.database.write().unwrap() = Some(database);
        self
    }

    pub fn set_database(&self, db: Arc<Database>) {
        *self.database.write().unwrap() = Some(db.clone());
        let kg = KnowledgeGraph::new(db);
        *self.knowledge_graph.write().unwrap() = Some(Arc::new(kg));
        tracing::info!("Memory: Database connected, knowledge graph initialized");
    }

    pub fn database(&self) -> Option<Arc<Database>> {
        self.database.read().unwrap().clone()
    }

    pub fn has_database(&self) -> bool {
        self.database.read().unwrap().is_some()
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

    pub fn knowledge_graph(&self) -> Option<Arc<KnowledgeGraph>> {
        self.knowledge_graph.read().unwrap().clone()
    }

    pub fn has_knowledge_graph(&self) -> bool {
        self.knowledge_graph.read().unwrap().is_some()
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

    pub fn hybrid_searcher(&self) -> &HybridSearcher {
        &self.hybrid_searcher
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

        let has_db = self.has_database();
        let _has_emb = self.has_embedding();
        if has_db {
            let db_clone = self.database().unwrap();
            let emb_clone = self.embedding_service();
            let session = session_id.to_string();
            let content_clone = content.clone();
            tokio::task::spawn(async move {
                match db_clone
                    .add_memory(
                        "default",
                        Some(&session),
                        "user",
                        &content_clone,
                        &serde_json::json!({}),
                        "short_term",
                        None,
                    )
                    .await
                {
                    Ok(memory_id) => {
                        if let Some(emb_service) = emb_clone {
                            let content_for_emb = content_clone.clone();
                            let db_for_emb = db_clone.clone();
                            tokio::task::spawn(async move {
                                match emb_service.embed(&content_for_emb).await {
                                    Ok(embedding) => {
                                        let emb_str =
                                            EmbeddingService::to_pgvector_string(&embedding);
                                        if let Err(e) =
                                            db_for_emb.update_embedding(memory_id, &emb_str).await
                                        {
                                            tracing::warn!(
                                                "Memory: Failed to update embedding: {}",
                                                e
                                            );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Memory: Failed to generate embedding: {}",
                                            e
                                        );
                                    }
                                }
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Memory: Failed to write short_term to DB: {}", e);
                    }
                }
            });
        }

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
        let item = MemoryItem::new(
            content.clone(),
            MemoryLayer::Archive,
            Some(session_id.to_string()),
        );
        self.archive.write().unwrap().add(item);

        if let Some(db) = self.database() {
            let db_clone = db;
            let session = session_id.to_string();
            tokio::task::spawn(async move {
                if let Err(e) = db_clone
                    .add_memory(
                        "default",
                        Some(&session),
                        "assistant",
                        &content,
                        &serde_json::json!({}),
                        "archive",
                        None,
                    )
                    .await
                {
                    tracing::warn!("Memory: Failed to write archive to DB: {}", e);
                }
            });
        }

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

    pub async fn search_semantic(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticSearchResult>> {
        let db = self
            .database()
            .ok_or_else(|| anyhow::anyhow!("Database not configured"))?;
        let emb = self
            .embedding_service()
            .ok_or_else(|| anyhow::anyhow!("Embedding service not configured"))?;

        let query_embedding = emb.embed(query).await?;
        let emb_str = EmbeddingService::to_pgvector_string(&query_embedding);

        let results = db
            .search_semantic("default", &emb_str, limit as i32)
            .await?;

        Ok(results
            .into_iter()
            .map(|(row, similarity)| SemanticSearchResult {
                content: row.content,
                similarity,
                layer: row.layer,
                role: row.role,
                created_at: row.created_at,
            })
            .collect())
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

#[derive(Debug, Clone)]
struct DefaultShortTermMemory {
    items: Vec<subhuti_core::memory::MemoryItem>,
    capacity: usize,
}

impl DefaultShortTermMemory {
    fn new(capacity: usize) -> Self {
        Self {
            items: Vec::new(),
            capacity,
        }
    }

    fn write(&mut self, item: subhuti_core::memory::MemoryItem) {
        self.items.push(item);
        while self.items.len() > self.capacity {
            self.items.remove(0);
        }
    }

    fn read(&self, id: &str) -> Option<&subhuti_core::memory::MemoryItem> {
        self.items.iter().find(|i| i.id == id)
    }

    fn get_all(&self) -> Vec<subhuti_core::memory::MemoryItem> {
        self.items.clone()
    }
}

#[derive(Debug, Clone)]
struct DefaultLongTermMemory {
    items: std::collections::HashMap<String, subhuti_core::memory::MemoryItem>,
}

impl DefaultLongTermMemory {
    fn new() -> Self {
        Self {
            items: std::collections::HashMap::new(),
        }
    }

    fn write(&mut self, item: subhuti_core::memory::MemoryItem) {
        self.items.insert(item.id.clone(), item);
    }

    fn read(&self, id: &str) -> Option<&subhuti_core::memory::MemoryItem> {
        self.items.get(id)
    }

    fn delete(&mut self, id: &str) {
        self.items.remove(id);
    }

    fn get_all(&self) -> Vec<subhuti_core::memory::MemoryItem> {
        self.items.values().cloned().collect()
    }
}

pub struct DefaultMemory {
    config: subhuti_core::memory::MemoryConfig,
    short_term: Arc<RwLock<DefaultShortTermMemory>>,
    archive: Arc<RwLock<DefaultLongTermMemory>>,
}

impl DefaultMemory {
    pub fn new() -> Self {
        Self::with_config(subhuti_core::memory::MemoryConfig::default())
    }

    pub fn with_config(config: subhuti_core::memory::MemoryConfig) -> Self {
        Self {
            config: config.clone(),
            short_term: Arc::new(RwLock::new(DefaultShortTermMemory::new(
                config.short_term_capacity,
            ))),
            archive: Arc::new(RwLock::new(DefaultLongTermMemory::new())),
        }
    }
}

#[async_trait]
impl subhuti_core::memory::Memory for DefaultMemory {
    fn write_short_term(&self, content: &str, tags: Vec<String>) {
        let item = subhuti_core::memory::MemoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            tags,
            metadata: serde_json::Value::Null,
            created_at: Utc::now(),
        };
        self.short_term.write().unwrap().write(item);
    }

    fn write_long_term(&self, content: &str, tags: Vec<String>) {
        let item = subhuti_core::memory::MemoryItem {
            id: uuid::Uuid::new_v4().to_string(),
            content: content.to_string(),
            tags,
            metadata: serde_json::Value::Null,
            created_at: Utc::now(),
        };
        self.archive.write().unwrap().write(item);
    }

    fn read(&self, id: &str) -> Option<subhuti_core::memory::MemoryItem> {
        if let Some(item) = self.short_term.read().unwrap().read(id) {
            return Some(item.clone());
        }
        self.archive.read().unwrap().read(id).cloned()
    }

    fn delete(&self, id: &str) {
        self.archive.write().unwrap().delete(id);
    }

    fn search(&self, query: &str, limit: usize) -> Vec<subhuti_core::memory::SearchResult> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<subhuti_core::memory::SearchResult> = Vec::new();

        for item in self.short_term.read().unwrap().get_all() {
            if item.content.to_lowercase().contains(&query_lower) {
                results.push(subhuti_core::memory::SearchResult {
                    id: item.id,
                    content: item.content,
                    score: 1.0,
                });
            }
        }

        for item in self.archive.read().unwrap().get_all() {
            if item.content.to_lowercase().contains(&query_lower) {
                results.push(subhuti_core::memory::SearchResult {
                    id: item.id,
                    content: item.content,
                    score: 0.8,
                });
            }
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
        let mut items = self.short_term.read().unwrap().get_all();
        items.extend(self.archive.read().unwrap().get_all());
        items
    }

    fn clear(&self) {
        *self.short_term.write().unwrap() =
            DefaultShortTermMemory::new(self.config.short_term_capacity);
        *self.archive.write().unwrap() = DefaultLongTermMemory::new();
    }
}

pub struct SqliteMemoryStore {
    pool: Arc<sqlx::SqlitePool>,
}

impl SqliteMemoryStore {
    pub async fn new(url: &str) -> subhuti_core::Result<Self> {
        let pool = sqlx::SqlitePool::connect(url)
            .await
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        Self::init_tables(&pool).await?;
        Ok(Self {
            pool: Arc::new(pool),
        })
    }

    pub fn arc(url: &str) -> Arc<Self> {
        let pool =
            sqlx::SqlitePool::connect_lazy(url).expect("Failed to create lazy connection pool");
        let store = Self {
            pool: Arc::new(pool),
        };
        Arc::new(store)
    }

    async fn init_tables(pool: &sqlx::SqlitePool) -> subhuti_core::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_items (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                metadata TEXT NOT NULL,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        Ok(())
    }

    pub async fn write_item(
        &self,
        item: &subhuti_core::memory::MemoryItem,
    ) -> subhuti_core::Result<()> {
        let tags_json = serde_json::to_string(&item.tags)?;
        let metadata_json = serde_json::to_string(&item.metadata)?;

        sqlx::query(
            r#"
            INSERT OR REPLACE INTO memory_items (id, content, tags, metadata, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(&item.id)
        .bind(&item.content)
        .bind(&tags_json)
        .bind(&metadata_json)
        .bind(item.created_at.to_rfc3339())
        .execute(&*self.pool)
        .await
        .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

        Ok(())
    }

    pub async fn read_item(
        &self,
        id: &str,
    ) -> subhuti_core::Result<Option<subhuti_core::memory::MemoryItem>> {
        let rows = sqlx::query(
            r#"
            SELECT id, content, tags, metadata, created_at FROM memory_items WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

        if rows.is_empty() {
            return Ok(None);
        }

        let row = &rows[0];
        let id: String = row
            .try_get(0)
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        let content: String = row
            .try_get(1)
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        let tags_json: String = row
            .try_get(2)
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        let metadata_json: String = row
            .try_get(3)
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        let created_at_str: String = row
            .try_get(4)
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

        let tags = serde_json::from_str(&tags_json).unwrap_or_default();
        let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .unwrap_or_else(|_| Utc::now().into())
            .with_timezone(&Utc);

        Ok(Some(subhuti_core::memory::MemoryItem {
            id,
            content,
            tags,
            metadata,
            created_at,
        }))
    }

    pub async fn delete_item(&self, id: &str) -> subhuti_core::Result<()> {
        sqlx::query("DELETE FROM memory_items WHERE id = ?")
            .bind(id)
            .execute(&*self.pool)
            .await
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        Ok(())
    }

    pub async fn search_items(
        &self,
        query: &str,
        limit: usize,
    ) -> subhuti_core::Result<Vec<subhuti_core::memory::MemoryItem>> {
        let rows = sqlx::query(
            r#"
            SELECT id, content, tags, metadata, created_at FROM memory_items
            WHERE content LIKE ?
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(format!("%{}%", query))
        .bind(limit as i64)
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            let id: String = row
                .try_get(0)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let content: String = row
                .try_get(1)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let tags_json: String = row
                .try_get(2)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let metadata_json: String = row
                .try_get(3)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let created_at_str: String = row
                .try_get(4)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

            let tags = serde_json::from_str(&tags_json).unwrap_or_default();
            let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap_or_else(|_| Utc::now().into())
                .with_timezone(&Utc);

            results.push(subhuti_core::memory::MemoryItem {
                id,
                content,
                tags,
                metadata,
                created_at,
            });
        }

        Ok(results)
    }

    pub async fn get_all_items(
        &self,
    ) -> subhuti_core::Result<Vec<subhuti_core::memory::MemoryItem>> {
        let rows = sqlx::query(
            r#"
            SELECT id, content, tags, metadata, created_at FROM memory_items
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&*self.pool)
        .await
        .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            let id: String = row
                .try_get(0)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let content: String = row
                .try_get(1)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let tags_json: String = row
                .try_get(2)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let metadata_json: String = row
                .try_get(3)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
            let created_at_str: String = row
                .try_get(4)
                .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;

            let tags = serde_json::from_str(&tags_json).unwrap_or_default();
            let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .unwrap_or_else(|_| Utc::now().into())
                .with_timezone(&Utc);

            results.push(subhuti_core::memory::MemoryItem {
                id,
                content,
                tags,
                metadata,
                created_at,
            });
        }

        Ok(results)
    }
}

#[async_trait]
impl subhuti_core::memory::DatabaseStore for SqliteMemoryStore {
    async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> subhuti_core::Result<Vec<serde_json::Value>> {
        let mut query = sqlx::query(sql);
        for param in params {
            match param {
                serde_json::Value::String(s) => query = query.bind(s),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        query = query.bind(i);
                    } else if let Some(f) = n.as_f64() {
                        query = query.bind(f);
                    }
                }
                serde_json::Value::Bool(b) => query = query.bind(b),
                _ => {}
            }
        }

        let _rows = query
            .fetch_all(&*self.pool)
            .await
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        Ok(Vec::new())
    }

    async fn execute(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> subhuti_core::Result<u64> {
        let mut query = sqlx::query(sql);
        for param in params {
            match param {
                serde_json::Value::String(s) => query = query.bind(s),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        query = query.bind(i);
                    } else if let Some(f) = n.as_f64() {
                        query = query.bind(f);
                    }
                }
                serde_json::Value::Bool(b) => query = query.bind(b),
                _ => {}
            }
        }

        let result = query
            .execute(&*self.pool)
            .await
            .map_err(|e| subhuti_core::Error::Any(anyhow::anyhow!("SQL error: {}", e)))?;
        Ok(result.rows_affected())
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
