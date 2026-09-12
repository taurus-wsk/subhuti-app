//! # 藏经阁存储层
//!
//! 内存缓存（高速） + PostgreSQL（唯一持久）
//! 内存 BM25 索引用于毫秒级检索，PG tsvector 用于全局全文检索。

use crate::sutra_library::models::*;
use crate::sutra_library::KbChunk;
use crate::sutra_library::KnowledgeBase;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// 内存 BM25 索引（简易实现）
#[derive(Debug, Clone)]
pub struct Bm25Index {
    /// 文档总数
    doc_count: usize,
    /// 词 -> 文档频率（包含该词的文档数）
    df: HashMap<String, usize>,
    /// 文档 -> 词频
    tf: HashMap<String, HashMap<String, usize>>,
    /// 文档内容缓存（用于检索时高亮）
    doc_contents: HashMap<String, String>,
    /// 平均文档长度
    avg_doc_len: f64,
    /// BM25 参数
    k1: f64,
    b: f64,
}

impl Bm25Index {
    pub fn new() -> Self {
        Self {
            doc_count: 0,
            df: HashMap::new(),
            tf: HashMap::new(),
            doc_contents: HashMap::new(),
            avg_doc_len: 0.0,
            k1: 1.2,
            b: 0.75,
        }
    }

    /// 添加文档到索引
    pub fn add_document(&mut self, doc_id: &str, text: &str) {
        let tokens: Vec<String> = text
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let doc_len = tokens.len() as f64;
        let old_total = self.avg_doc_len * self.doc_count as f64;
        self.doc_count += 1;
        self.avg_doc_len = (old_total + doc_len) / self.doc_count as f64;

        // 更新词频
        let mut term_freq: HashMap<String, usize> = HashMap::new();
        let mut seen: HashMap<String, bool> = HashMap::new();
        for token in &tokens {
            *term_freq.entry(token.clone()).or_insert(0) += 1;
            if !seen.contains_key(token) {
                *self.df.entry(token.clone()).or_insert(0) += 1;
                seen.insert(token.clone(), true);
            }
        }
        self.tf.insert(doc_id.to_string(), term_freq);
        self.doc_contents
            .insert(doc_id.to_string(), text.to_string());
    }

    /// 移除文档
    pub fn remove_document(&mut self, doc_id: &str) {
        if let Some(term_freq) = self.tf.remove(doc_id) {
            for term in term_freq.keys() {
                if let Some(count) = self.df.get_mut(term) {
                    *count -= 1;
                    if *count == 0 {
                        self.df.remove(term);
                    }
                }
            }
        }
        self.doc_contents.remove(doc_id);
        // 重新计算 avg_doc_len
        let total_len: f64 = self.tf.values().flat_map(|tf| tf.values()).sum::<usize>() as f64;
        self.doc_count = self.tf.len();
        self.avg_doc_len = if self.doc_count > 0 {
            total_len / self.doc_count as f64
        } else {
            0.0
        };
    }

    /// 搜索 BM25 得分
    pub fn search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        let query_tokens: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if query_tokens.is_empty() || self.doc_count == 0 {
            return Vec::new();
        }

        let mut scores: Vec<(String, f64)> = Vec::new();

        for (doc_id, term_freq) in &self.tf {
            let doc_len: f64 = term_freq.values().sum::<usize>() as f64;
            let mut score = 0.0;

            for token in &query_tokens {
                let qf = *term_freq.get(token).unwrap_or(&0) as f64;
                if qf == 0.0 {
                    continue;
                }
                let df = *self.df.get(token).unwrap_or(&1) as f64;
                let idf = ((self.doc_count as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();
                let tf_part = (qf * (self.k1 + 1.0))
                    / (qf + self.k1 * (1.0 - self.b + self.b * doc_len / self.avg_doc_len));
                score += idf * tf_part;
            }

            if score > 0.0 {
                scores.push((doc_id.clone(), score));
            }
        }

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(limit);
        scores.into_iter().map(|(id, s)| (id, s as f32)).collect()
    }
}

impl Default for Bm25Index {
    fn default() -> Self {
        Self::new()
    }
}

/// 内存存储（临时 + 热缓存）
pub struct MemoryStorage {
    // 核心节点存储
    nodes: RwLock<HashMap<String, MemoryNode>>,
    // 路径索引
    path_index: RwLock<HashMap<String, String>>,
    // 集合列表
    collections: RwLock<HashMap<String, Collection>>,
    // 关联边
    edges: RwLock<Vec<RefEdge>>,
    // BM25 索引
    bm25: RwLock<Bm25Index>,
    // 会话临时记忆
    session_memory: RwLock<HashMap<String, Vec<MemoryNode>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            nodes: RwLock::new(HashMap::new()),
            path_index: RwLock::new(HashMap::new()),
            collections: RwLock::new(HashMap::new()),
            edges: RwLock::new(Vec::new()),
            bm25: RwLock::new(Bm25Index::new()),
            session_memory: RwLock::new(HashMap::new()),
        }
    }

    // ─── 节点操作 ───────────────────────────────────────────────

    pub fn write_node(&self, node: &MemoryNode) {
        let content = format!("{} {} {}", node.title, node.summary, node.content);
        self.bm25
            .write()
            .unwrap()
            .add_document(&node.node_id, &content);
        self.path_index
            .write()
            .unwrap()
            .insert(node.path.clone(), node.node_id.clone());
        self.nodes
            .write()
            .unwrap()
            .insert(node.node_id.clone(), node.clone());
    }

    pub fn read_node(&self, node_id: &str) -> Option<MemoryNode> {
        self.nodes.read().unwrap().get(node_id).cloned()
    }

    pub fn delete_node(&self, node_id: &str) {
        self.nodes.write().unwrap().remove(node_id);
        self.bm25.write().unwrap().remove_document(node_id);
    }

    pub fn get_all_nodes(&self) -> Vec<MemoryNode> {
        self.nodes.read().unwrap().values().cloned().collect()
    }

    pub fn find_by_path(&self, path: &str) -> Option<MemoryNode> {
        let node_id = self.path_index.read().unwrap().get(path)?.clone();
        self.read_node(&node_id)
    }

    pub fn find_children(&self, parent_id: &str) -> Vec<MemoryNode> {
        self.nodes
            .read()
            .unwrap()
            .values()
            .filter(|n| n.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect()
    }

    // ─── 集合操作 ───────────────────────────────────────────────

    pub fn create_collection(&self, collection: Collection) {
        self.collections
            .write()
            .unwrap()
            .insert(collection.collection_id.clone(), collection);
    }

    pub fn get_collection(&self, collection_id: &str) -> Option<Collection> {
        self.collections.read().unwrap().get(collection_id).cloned()
    }

    pub fn list_collections(&self) -> Vec<Collection> {
        self.collections.read().unwrap().values().cloned().collect()
    }

    // ─── 检索 ───────────────────────────────────────────────────

    pub fn bm25_search(&self, query: &str, limit: usize) -> Vec<(String, f32)> {
        self.bm25.read().unwrap().search(query, limit)
    }

    pub fn keyword_search(&self, query: &str, limit: usize) -> Vec<MemoryNode> {
        let query_lower = query.to_lowercase();
        let mut results: Vec<MemoryNode> = self
            .nodes
            .read()
            .unwrap()
            .values()
            .filter(|n| {
                n.title.to_lowercase().contains(&query_lower)
                    || n.summary.to_lowercase().contains(&query_lower)
                    || n.content.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();
        results.truncate(limit);
        results
    }

    // ─── 会话临时记忆 ───────────────────────────────────────────

    pub fn add_session_memory(&self, session_id: &str, node: MemoryNode) {
        self.session_memory
            .write()
            .unwrap()
            .entry(session_id.to_string())
            .or_default()
            .push(node);
    }

    pub fn get_session_memory(&self, session_id: &str) -> Vec<MemoryNode> {
        self.session_memory
            .read()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn clear_session(&self, session_id: &str) {
        self.session_memory.write().unwrap().remove(session_id);
    }

    pub fn get_all_session_keys(&self) -> Vec<String> {
        self.session_memory
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    // ─── 统计 ───────────────────────────────────────────────────

    pub fn stats(&self) -> SutraStats {
        let nodes = self.nodes.read().unwrap();
        let hot_count = nodes.values().filter(|n| n.base_activation > 0.6).count();
        SutraStats {
            total_nodes: nodes.len(),
            hot_nodes: hot_count,
            cold_nodes: nodes.len().saturating_sub(hot_count),
            collections: self.collections.read().unwrap().len(),
            edges: self.edges.read().unwrap().len(),
            snapshots: 0,
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

/// PostgreSQL 存储适配器
pub struct PgStorage {
    pool: Arc<sqlx::PgPool>,
}

impl PgStorage {
    pub fn new(pool: Arc<sqlx::PgPool>) -> Self {
        Self { pool }
    }

    /// 获取内部 PG 连接池引用（用于共享给 PgGraphStorage 等）
    pub fn pool(&self) -> &Arc<sqlx::PgPool> {
        &self.pool
    }

    /// 建表（幂等）
    pub async fn ensure_tables(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_collections (
                collection_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                domain TEXT NOT NULL,
                description TEXT,
                created_at BIGINT NOT NULL
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_nodes (
                node_id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                domain TEXT NOT NULL,
                node_type TEXT NOT NULL,
                parent_id TEXT,
                path TEXT NOT NULL,
                depth INT NOT NULL,
                sort_order INT NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                metadata JSONB DEFAULT '{}',
                version_tag TEXT NOT NULL DEFAULT 'current',
                snapshot_id TEXT,
                base_activation REAL NOT NULL DEFAULT 0.5,
                importance SMALLINT NOT NULL DEFAULT 1,
                access_count INT NOT NULL DEFAULT 0,
                feedback_score REAL NOT NULL DEFAULT 0.0,
                last_accessed_at BIGINT NOT NULL,
                created_at BIGINT NOT NULL,
                updated_at BIGINT NOT NULL,
                FOREIGN KEY (collection_id) REFERENCES memory_collections(collection_id),
                UNIQUE(collection_id, version_tag, path, node_type, title)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_edges (
                edge_id TEXT PRIMARY KEY,
                from_node_id TEXT NOT NULL,
                from_collection_id TEXT NOT NULL,
                to_node_id TEXT NOT NULL,
                to_collection_id TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 1.0,
                created_at BIGINT NOT NULL,
                UNIQUE(from_node_id, to_node_id, edge_type)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_snapshots (
                snapshot_id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                name TEXT NOT NULL,
                version_tag TEXT NOT NULL,
                description TEXT,
                created_at BIGINT NOT NULL,
                FOREIGN KEY (collection_id) REFERENCES memory_collections(collection_id)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // 业务索引
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_nodes_collection ON memory_nodes(collection_id)",
        )
        .execute(&*self.pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_parent ON memory_nodes(parent_id)")
            .execute(&*self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_path ON memory_nodes(path)")
            .execute(&*self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_nodes_hash ON memory_nodes(content_hash)")
            .execute(&*self.pool)
            .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_nodes_activation ON memory_nodes(base_activation)",
        )
        .execute(&*self.pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_from ON memory_edges(from_node_id)")
            .execute(&*self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_edges_to ON memory_edges(to_node_id)")
            .execute(&*self.pool)
            .await?;

        // tsvector 全文索引
        sqlx::query(
            r#"
            DO $$ BEGIN
                IF NOT EXISTS (
                    SELECT 1 FROM pg_attribute
                    WHERE attrelid = 'memory_nodes'::regclass AND attname = 'fts_doc'
                ) THEN
                    ALTER TABLE memory_nodes ADD COLUMN fts_doc tsvector;
                END IF;
            END $$;
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // 触发器和函数
        sqlx::query(
            r#"
            CREATE OR REPLACE FUNCTION memory_fts_update() RETURNS trigger AS $$
            BEGIN
                NEW.fts_doc :=
                    setweight(to_tsvector('simple', COALESCE(NEW.title,'')), 'A') ||
                    setweight(to_tsvector('simple', COALESCE(NEW.summary,'')), 'B') ||
                    setweight(to_tsvector('simple', COALESCE(NEW.path,'')), 'C');
                RETURN NEW;
            END
            $$ LANGUAGE plpgsql;
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            DO $$ BEGIN
                IF NOT EXISTS (
                    SELECT 1 FROM pg_trigger WHERE tgname = 'trg_memory_fts'
                ) THEN
                    CREATE TRIGGER trg_memory_fts
                    BEFORE INSERT OR UPDATE ON memory_nodes
                    FOR EACH ROW EXECUTE FUNCTION memory_fts_update();
                END IF;
            END $$;
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // 确保 GIN 索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_nodes_fts ON memory_nodes USING GIN(fts_doc)
            "#,
        )
        .execute(&*self.pool)
        .await?;

        // ─── 知识库标准模式（kb_schema）───
        let pool = &*self.pool;
        for ddl in crate::sutra_library::kb_schema::KB_SCHEMA_DDLS {
            sqlx::query(ddl).execute(pool).await?;
        }
        for idx in crate::sutra_library::kb_schema::KB_SCHEMA_INDEXES {
            sqlx::query(idx).execute(pool).await?;
        }

        tracing::info!("SutraLibrary: PostgreSQL tables initialized (legacy + kb_schema)");
        Ok(())
    }

    // ─── 节点写入 ───────────────────────────────────────────────

    pub async fn write_node(&self, node: &MemoryNode) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO memory_nodes (
                node_id, collection_id, domain, node_type, parent_id,
                path, depth, sort_order, title, summary, content, content_hash,
                metadata, version_tag, snapshot_id, base_activation, importance,
                access_count, feedback_score, last_accessed_at, created_at, updated_at
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)
            ON CONFLICT (collection_id, version_tag, path, node_type, title)
            DO UPDATE SET
                content = EXCLUDED.content,
                content_hash = EXCLUDED.content_hash,
                summary = EXCLUDED.summary,
                metadata = EXCLUDED.metadata,
                updated_at = EXCLUDED.updated_at,
                base_activation = EXCLUDED.base_activation,
                access_count = EXCLUDED.access_count,
                feedback_score = EXCLUDED.feedback_score,
                last_accessed_at = EXCLUDED.last_accessed_at
            "#,
        )
        .bind(&node.node_id)
        .bind(&node.collection_id)
        .bind(&node.domain)
        .bind(&node.node_type)
        .bind(&node.parent_id)
        .bind(&node.path)
        .bind(node.depth as i32)
        .bind(node.sort_order as i32)
        .bind(&node.title)
        .bind(&node.summary)
        .bind(&node.content)
        .bind(&node.content_hash)
        .bind(&node.metadata)
        .bind(&node.version_tag)
        .bind(&node.snapshot_id)
        .bind(node.base_activation)
        .bind(node.importance as i16)
        .bind(node.access_count as i32)
        .bind(node.feedback_score)
        .bind(node.last_accessed_at)
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    // ─── 集合写入 ───────────────────────────────────────────────

    pub async fn write_collection(&self, collection: &Collection) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO memory_collections (collection_id, name, domain, description, created_at)
            VALUES ($1,$2,$3,$4,$5)
            ON CONFLICT (collection_id) DO UPDATE SET
                name = EXCLUDED.name,
                domain = EXCLUDED.domain,
                description = EXCLUDED.description
            "#,
        )
        .bind(&collection.collection_id)
        .bind(&collection.name)
        .bind(&collection.domain)
        .bind(&collection.description)
        .bind(collection.created_at)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    // ─── 检索 ───────────────────────────────────────────────────

    /// PG tsvector 全文检索
    pub async fn search_fts(
        &self,
        query: &str,
        collection_id: Option<&str>,
        domain: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryNode>> {
        let mut sql = String::from(
            r#"
            SELECT * FROM memory_nodes
            WHERE fts_doc @@ plainto_tsquery('simple', $1)
            "#,
        );
        let mut param_idx = 2;

        if collection_id.is_some() {
            sql.push_str(&format!(" AND collection_id = ${}", param_idx));
            param_idx += 1;
        }
        if domain.is_some() {
            sql.push_str(&format!(" AND domain = ${}", param_idx));
            param_idx += 1;
        }

        sql.push_str(&format!(
            " ORDER BY ts_rank(fts_doc, plainto_tsquery('simple', $1)) DESC LIMIT ${}",
            param_idx
        ));

        let mut query_builder = sqlx::query_as::<_, MemoryNode>(&sql)
            .bind(query)
            .bind(limit as i32);
        if let Some(cid) = collection_id {
            query_builder = query_builder.bind(cid);
        }
        if let Some(d) = domain {
            query_builder = query_builder.bind(d);
        }

        let rows = query_builder.fetch_all(&*self.pool).await?;
        Ok(rows)
    }

    /// 按路径查询
    pub async fn find_by_path(&self, path: &str) -> Result<Option<MemoryNode>> {
        let row =
            sqlx::query_as::<_, MemoryNode>("SELECT * FROM memory_nodes WHERE path = $1 LIMIT 1")
                .bind(path)
                .fetch_optional(&*self.pool)
                .await?;
        Ok(row)
    }

    /// 获取子节点
    pub async fn find_children(&self, parent_id: &str) -> Result<Vec<MemoryNode>> {
        let rows = sqlx::query_as::<_, MemoryNode>(
            "SELECT * FROM memory_nodes WHERE parent_id = $1 ORDER BY sort_order",
        )
        .bind(parent_id)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    /// 获取集合所有节点
    pub async fn list_nodes(&self, collection_id: &str) -> Result<Vec<MemoryNode>> {
        let rows = sqlx::query_as::<_, MemoryNode>(
            "SELECT * FROM memory_nodes WHERE collection_id = $1 ORDER BY path",
        )
        .bind(collection_id)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    /// 删除节点
    pub async fn delete_node(&self, node_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM memory_nodes WHERE node_id = $1")
            .bind(node_id)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    // ─── 知识库 CRUD ───────────────────────────────────────────

    /// 获取所有知识库
    pub async fn list_knowledge_bases(&self) -> Result<Vec<KnowledgeBase>> {
        let rows = sqlx::query_as::<_, KnowledgeBase>(
            "SELECT id, name, description, tags, expert_id, created_at, updated_at FROM domain_kb ORDER BY created_at DESC",
        )
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    /// 创建知识库
    pub async fn create_knowledge_base(
        &self,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        sqlx::query(
            "INSERT INTO domain_kb (id, name, description, tags, expert_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(name)
        .bind(description)
        .bind(tags)
        .bind(expert_id)
        .bind(now)
        .bind(now)
        .execute(&*self.pool)
        .await?;
        Ok(KnowledgeBase {
            id,
            name: name.to_string(),
            description: description.to_string(),
            tags: tags.clone(),
            expert_id: expert_id.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// 更新知识库
    pub async fn update_knowledge_base(
        &self,
        id: &str,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        sqlx::query("UPDATE domain_kb SET name = $1, description = $2, tags = $3, expert_id = $4, updated_at = $5 WHERE id = $6")
            .bind(name)
            .bind(description)
            .bind(tags)
            .bind(expert_id)
            .bind(now)
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(KnowledgeBase {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            tags: tags.clone(),
            expert_id: expert_id.to_string(),
            created_at: 0,
            updated_at: now,
        })
    }

    /// 删除知识库（级联删除所有关联数据）
    pub async fn delete_knowledge_base(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM domain_kb WHERE id = $1")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }

    /// 获取知识库下的所有切片
    pub async fn list_chunks(&self, kb_id: &str) -> Result<Vec<KbChunk>> {
        let rows = sqlx::query_as::<_, KbChunk>(
            "SELECT id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at FROM kb_chunk WHERE kb_id = $1 ORDER BY sort_order, created_at",
        )
        .bind(kb_id)
        .fetch_all(&*self.pool)
        .await?;
        Ok(rows)
    }

    /// 创建切片（文档内容）
    pub async fn create_chunk(&self, kb_id: &str, title: &str, content: &str) -> Result<KbChunk> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        use sha2::{Digest, Sha256};
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        sqlx::query(
            "INSERT INTO kb_chunk (id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&id)
        .bind(kb_id)
        .bind(title)
        .bind(content)
        .bind(&content_hash)
        .bind("active")
        .bind(0i32)
        .bind(serde_json::json!({}))
        .bind(now)
        .bind(now)
        .execute(&*self.pool)
        .await?;
        Ok(KbChunk {
            id,
            kb_id: kb_id.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            content_hash,
            status: "active".to_string(),
            sort_order: 0,
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        })
    }

    /// 更新切片
    pub async fn update_chunk(&self, id: &str, title: &str, content: &str) -> Result<KbChunk> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        use sha2::{Digest, Sha256};
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        sqlx::query("UPDATE kb_chunk SET title = $1, content = $2, content_hash = $3, updated_at = $4 WHERE id = $5")
            .bind(title)
            .bind(content)
            .bind(&content_hash)
            .bind(now)
            .bind(id)
            .execute(&*self.pool)
            .await?;
        // 查询更新后的完整数据
        let row = sqlx::query_as::<_, KbChunk>(
            "SELECT id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at FROM kb_chunk WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await?;
        match row {
            Some(chunk) => Ok(chunk),
            None => anyhow::bail!("chunk not found: {}", id),
        }
    }

    /// 删除切片
    pub async fn delete_chunk(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM kb_chunk WHERE id = $1")
            .bind(id)
            .execute(&*self.pool)
            .await?;
        Ok(())
    }
}

/// 为 MemoryNode 实现 sqlx::FromRow（手动）
impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for MemoryNode {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> std::result::Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            node_id: row.try_get("node_id")?,
            collection_id: row.try_get("collection_id")?,
            domain: row.try_get("domain")?,
            node_type: row.try_get("node_type")?,
            content_hash: row.try_get("content_hash")?,
            parent_id: row.try_get("parent_id")?,
            path: row.try_get("path")?,
            depth: {
                let v: i32 = row.try_get("depth")?;
                v as u32
            },
            sort_order: {
                let v: i32 = row.try_get("sort_order")?;
                v as u32
            },
            title: row.try_get("title")?,
            summary: row.try_get("summary")?,
            content: row.try_get("content")?,
            metadata: row.try_get("metadata")?,
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: row.try_get("version_tag")?,
            snapshot_id: row.try_get("snapshot_id")?,
            base_activation: {
                let v: f32 = row.try_get("base_activation")?;
                v
            },
            importance: {
                let v: i16 = row.try_get("importance")?;
                v as u8
            },
            access_count: {
                let v: i32 = row.try_get("access_count")?;
                v as u32
            },
            feedback_score: {
                let v: f32 = row.try_get("feedback_score")?;
                v
            },
            last_accessed_at: row.try_get("last_accessed_at")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

// ─── 图谱 PG 持久化 ─────────────────────────────────────────

/// 实体图谱的 PostgreSQL 持久化适配器
///
/// 存储邻接表、实体-切片索引、实体反馈分。启动时全量加载到内存，
/// 运行时异步双写（内存 + PG），保证重启后资产不丢失。
pub struct PgGraphStorage {
    pool: Arc<sqlx::PgPool>,
}

impl PgGraphStorage {
    pub fn new(pool: Arc<sqlx::PgPool>) -> Self {
        Self { pool }
    }

    /// 建表（幂等）
    pub async fn ensure_tables(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS graph_edges (
                from_entity TEXT NOT NULL,
                to_entity TEXT NOT NULL,
                edge_kind TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 0.3,
                PRIMARY KEY (from_entity, to_entity, edge_kind)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS graph_entity_chunks (
                entity_id TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                PRIMARY KEY (entity_id, chunk_id)
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS graph_entity_feedback (
                entity_id TEXT PRIMARY KEY,
                feedback_score REAL NOT NULL DEFAULT 0.0
            )
            "#,
        )
        .execute(&*self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_graph_edges_to ON graph_edges(to_entity)")
            .execute(&*self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_graph_entity_chunks_chunk ON graph_entity_chunks(chunk_id)")
            .execute(&*self.pool)
            .await?;

        tracing::info!("SutraLibrary: Graph PG tables initialized");
        Ok(())
    }

    /// 全量加载图谱数据到内存（启动时调用）
    pub async fn load_all(
        &self,
        adjacency: &std::collections::HashMap<String, Vec<(String, super::recall::EdgeKind, f32)>>,
        entity_to_chunks: &std::collections::HashMap<String, Vec<String>>,
        chunk_to_entities: &std::collections::HashMap<String, Vec<String>>,
        entity_feedback: &std::collections::HashMap<String, f32>,
    ) -> Result<(
        std::collections::HashMap<String, Vec<(String, super::recall::EdgeKind, f32)>>,
        std::collections::HashMap<String, Vec<String>>,
        std::collections::HashMap<String, Vec<String>>,
        std::collections::HashMap<String, f32>,
    )> {
        let mut adj = adjacency.clone();
        let mut e2c = entity_to_chunks.clone();
        let mut c2e = chunk_to_entities.clone();
        let mut ef = entity_feedback.clone();

        // 加载边
        let edge_rows = sqlx::query_as::<_, (String, String, String, f32)>(
            "SELECT from_entity, to_entity, edge_kind, weight FROM graph_edges",
        )
        .fetch_all(&*self.pool)
        .await?;

        for (from, to, kind_str, weight) in edge_rows {
            let kind = match kind_str.as_str() {
                "Manual" => super::recall::EdgeKind::Manual,
                _ => super::recall::EdgeKind::Learned,
            };
            adj.entry(from.clone())
                .or_default()
                .push((to.clone(), kind, weight));
            adj.entry(to.clone())
                .or_default()
                .push((from.clone(), kind, weight));
        }

        // 加载实体-切片索引
        let ec_rows = sqlx::query_as::<_, (String, String)>(
            "SELECT entity_id, chunk_id FROM graph_entity_chunks",
        )
        .fetch_all(&*self.pool)
        .await?;

        for (entity_id, chunk_id) in ec_rows {
            e2c.entry(entity_id.clone())
                .or_default()
                .push(chunk_id.clone());
            c2e.entry(chunk_id).or_default().push(entity_id);
        }

        // 加载反馈分
        let fb_rows = sqlx::query_as::<_, (String, f32)>(
            "SELECT entity_id, feedback_score FROM graph_entity_feedback",
        )
        .fetch_all(&*self.pool)
        .await?;

        for (entity_id, score) in fb_rows {
            ef.insert(entity_id, score);
        }

        tracing::info!(
            "SutraLibrary: Graph loaded from PG ({} edges, {} entity-chunk mappings, {} feedback scores)",
            adj.len(),
            e2c.len(),
            ef.len()
        );

        Ok((adj, e2c, c2e, ef))
    }

    /// 写入边
    pub async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO graph_edges (from_entity, to_entity, edge_kind, weight)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (from_entity, to_entity, edge_kind)
            DO UPDATE SET weight = EXCLUDED.weight
            "#,
        )
        .bind(from)
        .bind(to)
        .bind(kind)
        .bind(weight)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    /// 删除边
    pub async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM graph_edges WHERE from_entity=$1 AND to_entity=$2 AND edge_kind=$3",
        )
        .bind(from)
        .bind(to)
        .bind(kind)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    /// 写入实体-切片映射
    pub async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO graph_entity_chunks (entity_id, chunk_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(entity_id)
        .bind(chunk_id)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    /// 写入实体反馈分
    pub async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO graph_entity_feedback (entity_id, feedback_score)
            VALUES ($1, $2)
            ON CONFLICT (entity_id)
            DO UPDATE SET feedback_score = EXCLUDED.feedback_score
            "#,
        )
        .bind(entity_id)
        .bind(score)
        .execute(&*self.pool)
        .await?;
        Ok(())
    }

    /// 批量删除边（用于衰减后清理低权重边）
    pub async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        for (from, to, kind) in edges {
            if let Err(e) = self.delete_edge(from, to, kind).await {
                tracing::warn!("SutraLibrary: PG batch delete edge failed: {}", e);
            }
        }
        Ok(())
    }
}
