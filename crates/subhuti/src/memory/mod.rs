//! # Memory Layer - 记忆层
//!
//! 职责：所有数据存储、检索、归档、分层治理
//!
//! ## 三层标准记忆
//!
//! - **短期工作记忆 (Session)**: 当前对话上下文，默认自动注入 LLM
//! - **长期归档记忆 (Archive)**: 历史对话沉淀，AI 主动调用搜索
//! - **知识库语义记忆 (Knowledge)**: 向量知识、外部文档，向量检索

mod embedding;
mod knowledge;
mod long_term;
mod short_term;
pub mod storage;

pub use embedding::{EmbeddingConfig, EmbeddingService};
pub use knowledge::KnowledgeMemory;
pub use long_term::LongTermMemory;
pub use short_term::ShortTermMemory;
pub use storage::{
    Database, DbConfig, FeedbackRow, HistoryRow, MemoryRow, PersonaData, PersonaRow,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// 记忆配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryConfig {
    /// 短期记忆容量
    pub short_term_capacity: usize,
    /// 长期记忆归档阈值
    pub archive_threshold: usize,
    /// 知识库向量维度
    pub knowledge_dim: usize,
    /// 记忆过期时间（秒）
    pub ttl_seconds: Option<u64>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            short_term_capacity: 10,
            archive_threshold: 20,
            knowledge_dim: 384,
            ttl_seconds: Some(3600 * 24 * 7), // 7天
        }
    }
}

/// 记忆项
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryItem {
    /// 唯一ID
    pub id: String,
    /// 内容
    pub content: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 元数据
    pub metadata: HashMap<String, String>,
    /// 记忆层级
    pub layer: MemoryLayer,
    /// 会话ID（用于关联）
    pub session_id: Option<String>,
}

impl MemoryItem {
    /// 创建新的记忆项
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

    /// 添加元数据
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// 检查是否过期
    pub fn is_expired(&self, ttl_seconds: u64) -> bool {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.created_at);
        duration.num_seconds() > ttl_seconds as i64
    }
}

/// 记忆层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLayer {
    /// 短期工作记忆
    ShortTerm,
    /// 长期归档记忆
    Archive,
    /// 知识库语义记忆
    Knowledge,
}

/// 搜索结果
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResult {
    /// 记忆项
    pub item: MemoryItem,
    /// 相似度分数
    pub score: f32,
}

/// 语义搜索结果（向量相似度）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SemanticSearchResult {
    /// 内容
    pub content: String,
    /// 相似度（0-1，越高越相似）
    pub similarity: f32,
    /// 记忆层
    pub layer: String,
    /// 角色
    pub role: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 记忆存储器接口
pub trait MemoryStore: Send + Sync {
    /// 写入记忆
    fn write(&self, item: MemoryItem) -> Result<()>;
    /// 读取记忆
    fn read(&self, id: &str) -> Option<MemoryItem>;
    /// 删除记忆
    fn delete(&self, id: &str) -> Result<()>;
    /// 搜索记忆（文本）
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResult>;
    /// 获取所有记忆
    fn get_all(&self) -> Vec<MemoryItem>;
    /// 清空
    fn clear(&mut self) -> Result<()>;
}

/// 数据库/DAO 接口
///
/// 数据库操作属于记忆/存储的范畴，放在 Memory 层中。
/// 可以实现各种数据库后端：SQLite、PostgreSQL、MySQL 等。
#[async_trait::async_trait]
pub trait DatabaseStore: Send + Sync {
    /// 执行查询
    async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> Result<Vec<serde_json::Value>>;

    /// 执行写入
    async fn execute(&self, sql: &str, params: Vec<serde_json::Value>) -> Result<u64>;
}

/// 统一记忆管理器
pub struct Memory {
    config: MemoryConfig,
    short_term: Arc<RwLock<ShortTermMemory>>,
    archive: Arc<RwLock<LongTermMemory>>,
    knowledge: Arc<RwLock<KnowledgeMemory>>,
    /// 数据库存储（可选，双写策略，运行时可设置）
    database: RwLock<Option<Arc<Database>>>,
    /// Embedding 服务（可选，用于向量搜索）
    embedding: RwLock<Option<Arc<EmbeddingService>>>,
}

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("config", &self.config)
            .field("has_database", &self.has_database())
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
            database: RwLock::new(self.database()),
            embedding: RwLock::new(self.embedding_service()),
        }
    }
}

impl Memory {
    /// 创建新的 Memory 实例
    pub fn new() -> Self {
        Self::with_config(MemoryConfig::default())
    }

    /// 使用配置创建
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
        }
    }

    /// 设置数据库存储（构造时用）
    pub fn with_database(self, database: Arc<Database>) -> Self {
        *self.database.write().unwrap() = Some(database);
        self
    }

    /// 运行时设置数据库连接
    pub fn set_database(&self, db: Arc<Database>) {
        *self.database.write().unwrap() = Some(db);
        tracing::info!("Memory: Database connected for memory persistence");
    }

    /// 获取数据库存储（如果有）
    pub fn database(&self) -> Option<Arc<Database>> {
        self.database.read().unwrap().clone()
    }

    /// 检查是否有数据库
    pub fn has_database(&self) -> bool {
        self.database.read().unwrap().is_some()
    }

    /// 设置 embedding 服务
    pub fn with_embedding(self, config: EmbeddingConfig) -> Self {
        *self.embedding.write().unwrap() = Some(Arc::new(EmbeddingService::new(config)));
        self
    }

    /// 运行时设置 embedding 服务
    pub fn set_embedding(&self, service: Arc<EmbeddingService>) {
        *self.embedding.write().unwrap() = Some(service);
        tracing::info!("Memory: Embedding service connected");
    }

    /// 获取 embedding 服务
    pub fn embedding_service(&self) -> Option<Arc<EmbeddingService>> {
        self.embedding.read().unwrap().clone()
    }

    /// 检查是否有 embedding 服务
    pub fn has_embedding(&self) -> bool {
        self.embedding.read().unwrap().is_some()
    }

    /// 写入短期记忆
    pub fn write_short_term(&self, content: String, session_id: &str) -> Result<()> {
        let item = MemoryItem::new(
            content.clone(),
            MemoryLayer::ShortTerm,
            Some(session_id.to_string()),
        );
        self.short_term.write().unwrap().add(item);

        // 双写：写入数据库 + 异步生成 embedding
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
                        // 如果有 embedding 服务，异步生成向量
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

        // 检查是否需要归档
        if self.short_term.read().unwrap().len() >= self.config.archive_threshold {
            self.archive_from_short_term(session_id)?;
        }
        Ok(())
    }

    /// 从短期记忆归档到长期记忆
    pub fn archive_from_short_term(&self, session_id: &str) -> Result<()> {
        let items: Vec<_> = self.short_term.write().unwrap().drain_session(session_id);

        for item in items {
            let mut archive_item = item;
            archive_item.layer = MemoryLayer::Archive;
            self.archive.write().unwrap().add(archive_item);
        }
        Ok(())
    }

    /// 归档对话消息对到长期记忆
    ///
    /// 滑动窗口机制的核心方法：
    /// 当短期记忆滑动窗口超限时，被挤出的对话对会调用此方法归档到长期记忆
    pub fn archive_long_term(
        &self,
        session_id: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> Result<()> {
        // 组合成一条归档记录
        let content = format!("User: {}\nAssistant: {}", user_message, assistant_message);
        let item = MemoryItem::new(
            content.clone(),
            MemoryLayer::Archive,
            Some(session_id.to_string()),
        );
        self.archive.write().unwrap().add(item);

        // 双写：写入数据库归档
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

    /// 搜索短期记忆
    pub fn search_short_term(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.short_term.read().unwrap().search(query, limit)
    }

    /// 搜索长期记忆
    pub fn search_archive(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.archive.read().unwrap().search(query, limit)
    }

    /// 搜索知识库
    pub fn search_knowledge(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        self.knowledge.read().unwrap().search(query, limit)
    }

    /// 语义搜索（向量相似度）
    ///
    /// 需要同时配置数据库和 embedding 服务
    /// 返回按相似度从高到低排序的结果
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

        // 生成查询向量
        let query_embedding = emb.embed(query).await?;
        let emb_str = EmbeddingService::to_pgvector_string(&query_embedding);

        // 数据库向量搜索
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

    /// 添加知识到知识库
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

    /// 裁剪短期记忆
    pub fn prune_short_term(&self, keep_count: usize) -> Vec<MemoryItem> {
        self.short_term.write().unwrap().prune(keep_count)
    }

    /// 获取短期记忆摘要
    pub fn summarize_short_term(&self) -> String {
        self.short_term.read().unwrap().summarize()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.short_term.read().unwrap().is_empty()
            && self.archive.read().unwrap().is_empty()
            && self.knowledge.read().unwrap().is_empty()
    }

    /// 获取统计信息
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            short_term_count: self.short_term.read().unwrap().len(),
            archive_count: self.archive.read().unwrap().len(),
            knowledge_count: self.knowledge.read().unwrap().len(),
        }
    }
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

/// 记忆统计
#[derive(Debug, Clone, Serialize)]
pub struct MemoryStats {
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
