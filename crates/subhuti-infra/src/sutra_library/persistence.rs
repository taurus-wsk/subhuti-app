//! # 藏经阁持久化端口（PersistencePort）
//!
//! 统一抽象「藏经阁内存之外的那一层持久化」。目前有两种后端：
//!
//! - [`PgStorage`]：PostgreSQL（tsvector 全文检索、JSONB 元数据）
//! - [`SqliteStorage`]：SQLite（无 PG 时的降级后端，实现同接口）
//!
//! 引擎、检索调度器、实体图谱统一依赖 `Option<Arc<dyn PersistencePort>>`，
//! 后端可插拔、可替换，符合开闭原则——新增后端只需实现本 trait。
//!
//! ps: 反馈分析器（FeedbackAnalyzer）的指标持久化仍走具体 `PgStorage`，
//! 属可选分析能力，无 PG 时回退内存模式，不在本端口职责内。

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

use crate::sutra_library::models::*;
use crate::sutra_library::recall::{EdgeKind, GraphPersistence};
use crate::sutra_library::storage::PgStorage;
use crate::sutra_library::{KbChunk, KnowledgeBase};

/// 无向边端点归一化：按字典序排列，保证 (A,B) 与 (B,A) 落到同一行。
///
/// `graph_edges` 的主键是 `(from_entity, to_entity, edge_kind)`，天然带方向；
/// 但 `EntityGraph` 是**无向图**——同一对实体若以相反顺序写入就会存成两行。
/// 后果有二：回灌时双向展开让内存边数成倍虚高；`delete_edge` 也删不干净另一半。
/// 因此统一在存储层做端点排序，让"无向"成为表结构层面的事实约束。
#[inline]
pub(crate) fn normalize_edge_endpoints<'a>(from: &'a str, to: &'a str) -> (&'a str, &'a str) {
    if from <= to {
        (from, to)
    } else {
        (to, from)
    }
}

/// 藏经阁持久化端口
///
/// 覆盖两大部分：
/// - 记忆树核心存储（集合 / 节点 / 检索 / 知识库 CRUD）
/// - 实体图谱持久化（邻接表 / 实体-切片索引 / 实体反馈）
#[async_trait]
pub trait PersistencePort: Send + Sync {
    // ─── 核心存储 ──────────────────────────────────────────────
    async fn ensure_tables(&self) -> Result<()>;
    async fn write_node(&self, node: &MemoryNode) -> Result<()>;
    async fn write_collection(&self, collection: &Collection) -> Result<()>;
    async fn search_fts(
        &self,
        query: &str,
        collection_id: Option<&str>,
        domain: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryNode>>;
    async fn find_by_path(&self, path: &str) -> Result<Option<MemoryNode>>;
    async fn find_children(&self, parent_id: &str) -> Result<Vec<MemoryNode>>;
    async fn list_nodes(&self, collection_id: &str) -> Result<Vec<MemoryNode>>;

    /// 列出**全部**节点（跨集合）。启动回灌（hydrate）时使用。
    ///
    /// 默认返回空，未覆盖此方法的后端视为"不提供全量回灌"。
    async fn list_all_nodes(&self) -> Result<Vec<MemoryNode>> {
        Ok(Vec::new())
    }

    /// 列出全部集合。启动回灌（hydrate）时使用。
    async fn list_all_collections(&self) -> Result<Vec<Collection>> {
        Ok(Vec::new())
    }
    async fn delete_node(&self, node_id: &str) -> Result<()>;
    async fn list_knowledge_bases(&self) -> Result<Vec<KnowledgeBase>>;
    async fn create_knowledge_base(
        &self,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase>;
    async fn update_knowledge_base(
        &self,
        id: &str,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase>;
    async fn delete_knowledge_base(&self, id: &str) -> Result<()>;
    async fn list_chunks(&self, kb_id: &str) -> Result<Vec<KbChunk>>;
    async fn create_chunk(&self, kb_id: &str, title: &str, content: &str) -> Result<KbChunk>;
    async fn update_chunk(&self, id: &str, title: &str, content: &str) -> Result<KbChunk>;
    async fn delete_chunk(&self, id: &str) -> Result<()>;

    // ─── 实体图谱 ──────────────────────────────────────────────
    async fn load_all(
        &self,
        adjacency: &HashMap<String, Vec<(String, EdgeKind, f32)>>,
        entity_to_chunks: &HashMap<String, Vec<String>>,
        chunk_to_entities: &HashMap<String, Vec<String>>,
        entity_feedback: &HashMap<String, f32>,
    ) -> Result<(
        HashMap<String, Vec<(String, EdgeKind, f32)>>,
        HashMap<String, Vec<String>>,
        HashMap<String, Vec<String>>,
        HashMap<String, f32>,
    )>;
    async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()>;
    async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()>;
    async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()>;
    async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()>;
    async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()>;
}

// ─── PostgreSQL 后端 ──────────────────────────────────────────────
//
// 委托给 PgStorage 的内置方法（sutra_library::storage 中实现）。
// 由于固有方法的优先级高于 trait 方法，`PgStorage::xxx(self, ..)` 会命中固有实现，不会递归。

#[async_trait]
impl PersistencePort for PgStorage {
    async fn ensure_tables(&self) -> Result<()> {
        PgStorage::ensure_tables(self).await
    }
    async fn write_node(&self, node: &MemoryNode) -> Result<()> {
        PgStorage::write_node(self, node).await
    }
    async fn write_collection(&self, collection: &Collection) -> Result<()> {
        PgStorage::write_collection(self, collection).await
    }
    async fn search_fts(
        &self,
        query: &str,
        collection_id: Option<&str>,
        domain: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryNode>> {
        PgStorage::search_fts(self, query, collection_id, domain, limit).await
    }
    async fn find_by_path(&self, path: &str) -> Result<Option<MemoryNode>> {
        PgStorage::find_by_path(self, path).await
    }
    async fn find_children(&self, parent_id: &str) -> Result<Vec<MemoryNode>> {
        PgStorage::find_children(self, parent_id).await
    }
    async fn list_nodes(&self, collection_id: &str) -> Result<Vec<MemoryNode>> {
        PgStorage::list_nodes(self, collection_id).await
    }
    async fn list_all_nodes(&self) -> Result<Vec<MemoryNode>> {
        PgStorage::list_all_nodes(self).await
    }
    async fn list_all_collections(&self) -> Result<Vec<Collection>> {
        PgStorage::list_all_collections(self).await
    }
    async fn delete_node(&self, node_id: &str) -> Result<()> {
        PgStorage::delete_node(self, node_id).await
    }
    async fn list_knowledge_bases(&self) -> Result<Vec<KnowledgeBase>> {
        PgStorage::list_knowledge_bases(self).await
    }
    async fn create_knowledge_base(
        &self,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        PgStorage::create_knowledge_base(self, name, description, tags, expert_id).await
    }
    async fn update_knowledge_base(
        &self,
        id: &str,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        PgStorage::update_knowledge_base(self, id, name, description, tags, expert_id).await
    }
    async fn delete_knowledge_base(&self, id: &str) -> Result<()> {
        PgStorage::delete_knowledge_base(self, id).await
    }
    async fn list_chunks(&self, kb_id: &str) -> Result<Vec<KbChunk>> {
        PgStorage::list_chunks(self, kb_id).await
    }
    async fn create_chunk(&self, kb_id: &str, title: &str, content: &str) -> Result<KbChunk> {
        PgStorage::create_chunk(self, kb_id, title, content).await
    }
    async fn update_chunk(&self, id: &str, title: &str, content: &str) -> Result<KbChunk> {
        PgStorage::update_chunk(self, id, title, content).await
    }
    async fn delete_chunk(&self, id: &str) -> Result<()> {
        PgStorage::delete_chunk(self, id).await
    }

    async fn load_all(
        &self,
        adjacency: &HashMap<String, Vec<(String, EdgeKind, f32)>>,
        entity_to_chunks: &HashMap<String, Vec<String>>,
        chunk_to_entities: &HashMap<String, Vec<String>>,
        entity_feedback: &HashMap<String, f32>,
    ) -> Result<(
        HashMap<String, Vec<(String, EdgeKind, f32)>>,
        HashMap<String, Vec<String>>,
        HashMap<String, Vec<String>>,
        HashMap<String, f32>,
    )> {
        PgStorage::load_all(
            self,
            adjacency,
            entity_to_chunks,
            chunk_to_entities,
            entity_feedback,
        )
        .await
    }
    async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()> {
        PgStorage::write_edge(self, from, to, kind, weight).await
    }
    async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()> {
        PgStorage::delete_edge(self, from, to, kind).await
    }
    async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()> {
        PgStorage::write_entity_chunk(self, entity_id, chunk_id).await
    }
    async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()> {
        PgStorage::write_entity_feedback(self, entity_id, score).await
    }
    async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        PgStorage::batch_delete_edges(self, edges).await
    }
}

// ─── SQLite 降级后端 ──────────────────────────────────────────────

/// SQLite 持久化适配器
///
/// 作为「无 PostgreSQL」时的降级后端，提供与 PG 相同的持久化能力：
/// - 记忆树核心表 + 知识库表（TEXT 存 JSON，LIKE 全文检索）
/// - 实体图谱表
///
/// 数据库文件路径由 `use file::path/to/sutra.sqlite` 指定；
/// 传入 `:memory:` 则纯内存（进程结束即丢）。
pub struct SqliteStorage {
    pool: sqlx::SqlitePool,
}

impl SqliteStorage {
    /// 打开（或创建）指定路径的 SQLite 数据库
    ///
    /// 使用懒连接（lazy connect），不阻塞当前线程，首次实际读写时才真正建链；
    /// 因此 `create_sutra_engine`（同步函数）可安全地在无 PG 时降级到本后端。
    /// 传入 `:memory:` 则纯内存（进程结束即丢），此时强制单连接，
    /// 避免多个连接各自持有互相独立的空内存库。
    pub fn open(path: &str) -> Result<Self> {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
        use std::str::FromStr;
        use std::time::Duration;

        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // ⚠️ create_if_missing 必须为 true：sqlx 默认 false，
        // 文件不存在时会在首次执行 SQL 时报 `(code: 14) unable to open database file`。
        // WAL + busy_timeout 让多进程（HTTP / MCP）并发读写同一库时不会立刻 SQLITE_BUSY。
        let opts = SqliteConnectOptions::from_str(path)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));

        let max_conn = if path == ":memory:" { 1 } else { 4 };
        let pool = SqlitePoolOptions::new()
            .max_connections(max_conn)
            .connect_lazy_with(opts);
        Ok(Self { pool })
    }

    /// 以内存库方式创建（数据不落盘，仅用于测试/调试）
    pub fn open_in_memory() -> Result<Self> {
        Self::open(":memory:")
    }
}

/// 从 sqlite 行解码 MemoryNode
fn row_to_memory_node(row: &sqlx::sqlite::SqliteRow) -> Result<MemoryNode> {
    use sqlx::Row;
    Ok(MemoryNode {
        node_id: row.try_get("node_id")?,
        collection_id: row.try_get("collection_id")?,
        domain: row.try_get("domain")?,
        node_type: row.try_get("node_type")?,
        content_hash: row.try_get("content_hash")?,
        parent_id: row.try_get("parent_id")?,
        path: row.try_get("path")?,
        depth: {
            let v: i64 = row.try_get("depth")?;
            v as u32
        },
        sort_order: {
            let v: i64 = row.try_get("sort_order")?;
            v as u32
        },
        title: row.try_get("title")?,
        summary: row.try_get("summary")?,
        content: row.try_get("content")?,
        metadata: {
            let s: String = row.try_get("metadata")?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
        },
        refs_out: Vec::new(),
        refs_in: Vec::new(),
        version_tag: row.try_get("version_tag")?,
        snapshot_id: row.try_get("snapshot_id")?,
        base_activation: row.try_get("base_activation")?,
        importance: {
            let v: i64 = row.try_get("importance")?;
            v as u8
        },
        access_count: {
            let v: i64 = row.try_get("access_count")?;
            v as u32
        },
        feedback_score: row.try_get("feedback_score")?,
        last_accessed_at: row.try_get("last_accessed_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 从 sqlite 行解码 KnowledgeBase
fn row_to_kb(row: &sqlx::sqlite::SqliteRow) -> Result<KnowledgeBase> {
    use sqlx::Row;
    Ok(KnowledgeBase {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        tags: {
            let s: String = row.try_get("tags")?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Array(vec![]))
        },
        expert_id: row.try_get("expert_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

/// 从 sqlite 行解码 KbChunk
fn row_to_chunk(row: &sqlx::sqlite::SqliteRow) -> Result<KbChunk> {
    use sqlx::Row;
    Ok(KbChunk {
        id: row.try_get("id")?,
        kb_id: row.try_get("kb_id")?,
        title: row.try_get("title")?,
        content: row.try_get("content")?,
        content_hash: row.try_get("content_hash")?,
        status: row.try_get("status")?,
        sort_order: row.try_get("sort_order")?,
        metadata: {
            let s: String = row.try_get("metadata")?;
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
        },
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[async_trait]
impl PersistencePort for SqliteStorage {
    async fn ensure_tables(&self) -> Result<()> {
        let ddl = [
            // 记忆树
            r#"
            CREATE TABLE IF NOT EXISTS memory_collections (
                collection_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                domain TEXT NOT NULL,
                description TEXT,
                created_at INTEGER NOT NULL
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS memory_nodes (
                node_id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                domain TEXT NOT NULL,
                node_type TEXT NOT NULL,
                parent_id TEXT,
                path TEXT NOT NULL,
                depth INTEGER NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                title TEXT NOT NULL,
                summary TEXT NOT NULL,
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                metadata TEXT DEFAULT '{}',
                version_tag TEXT NOT NULL DEFAULT 'current',
                snapshot_id TEXT,
                base_activation REAL NOT NULL DEFAULT 0.5,
                importance INTEGER NOT NULL DEFAULT 1,
                access_count INTEGER NOT NULL DEFAULT 0,
                feedback_score REAL NOT NULL DEFAULT 0.0,
                last_accessed_at INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS memory_edges (
                edge_id TEXT PRIMARY KEY,
                from_node_id TEXT NOT NULL,
                from_collection_id TEXT NOT NULL,
                to_node_id TEXT NOT NULL,
                to_collection_id TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 1.0,
                created_at INTEGER NOT NULL,
                UNIQUE(from_node_id, to_node_id, edge_type)
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS memory_snapshots (
                snapshot_id TEXT PRIMARY KEY,
                collection_id TEXT NOT NULL,
                name TEXT NOT NULL,
                version_tag TEXT NOT NULL,
                description TEXT,
                created_at INTEGER NOT NULL
            )"#,
            // 知识库
            r#"
            CREATE TABLE IF NOT EXISTS domain_kb (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                tags TEXT NOT NULL DEFAULT '[]',
                expert_id TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS kb_chunk (
                id TEXT PRIMARY KEY,
                kb_id TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'active',
                sort_order INTEGER NOT NULL DEFAULT 0,
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )"#,
            // 实体图谱
            r#"
            CREATE TABLE IF NOT EXISTS graph_edges (
                from_entity TEXT NOT NULL,
                to_entity TEXT NOT NULL,
                edge_kind TEXT NOT NULL,
                weight REAL NOT NULL DEFAULT 0.3,
                PRIMARY KEY (from_entity, to_entity, edge_kind)
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS graph_entity_chunks (
                entity_id TEXT NOT NULL,
                chunk_id TEXT NOT NULL,
                PRIMARY KEY (entity_id, chunk_id)
            )"#,
            r#"
            CREATE TABLE IF NOT EXISTS graph_entity_feedback (
                entity_id TEXT PRIMARY KEY,
                feedback_score REAL NOT NULL DEFAULT 0.0
            )"#,
        ];
        for sql in ddl {
            sqlx::query(sql).execute(&self.pool).await?;
        }
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_sq_nodes_collection ON memory_nodes(collection_id)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sq_nodes_parent ON memory_nodes(parent_id)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sq_nodes_path ON memory_nodes(path)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_sq_graph_edges_to ON graph_edges(to_entity)")
            .execute(&self.pool)
            .await?;

        // 同名同域的集合只应存在一条。历史版本每次注册都用新 UUID 重新落库，
        // 已在库里堆出大量重复行（实测 `blender_knowledge` 重复 45 次）。
        // 这里做一次幂等收敛：每组 (name, domain) 只保留 rowid 最大的一条，
        // 且**绝不删除仍被 memory_nodes 引用的行**（宁可留重复，也不能孤儿化节点）。
        sqlx::query(
            r#"
            DELETE FROM memory_collections
            WHERE rowid NOT IN (
                SELECT MAX(rowid) FROM memory_collections GROUP BY name, domain
            )
              AND collection_id NOT IN (
                SELECT DISTINCT collection_id FROM memory_nodes
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // 唯一索引是兜底闸门。若上面的引用保护仍留下重复行，建索引会失败——
        // 此时只告警、不阻断启动（真正的防重复闸门在 engine 层复用逻辑）。
        if let Err(e) = sqlx::query(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_sq_collections_name_domain
             ON memory_collections(name, domain)",
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(
                "SutraLibrary: 集合唯一索引未建立（存在被引用的重复集合，已跳过）：{}",
                e
            );
        }

        tracing::info!("SutraLibrary: SQLite tables initialized");
        Ok(())
    }

    async fn write_node(&self, node: &MemoryNode) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO memory_nodes (
                node_id, collection_id, domain, node_type, parent_id,
                path, depth, sort_order, title, summary, content, content_hash,
                metadata, version_tag, snapshot_id, base_activation, importance,
                access_count, feedback_score, last_accessed_at, created_at, updated_at
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
            ON CONFLICT(node_id) DO UPDATE SET
                content = excluded.content,
                content_hash = excluded.content_hash,
                summary = excluded.summary,
                metadata = excluded.metadata,
                updated_at = excluded.updated_at,
                base_activation = excluded.base_activation,
                access_count = excluded.access_count,
                feedback_score = excluded.feedback_score,
                last_accessed_at = excluded.last_accessed_at
            "#,
        )
        .bind(&node.node_id)
        .bind(&node.collection_id)
        .bind(&node.domain)
        .bind(&node.node_type)
        .bind(&node.parent_id)
        .bind(&node.path)
        .bind(node.depth as i64)
        .bind(node.sort_order as i64)
        .bind(&node.title)
        .bind(&node.summary)
        .bind(&node.content)
        .bind(&node.content_hash)
        .bind(node.metadata.to_string())
        .bind(&node.version_tag)
        .bind(&node.snapshot_id)
        .bind(node.base_activation)
        .bind(node.importance as i64)
        .bind(node.access_count as i64)
        .bind(node.feedback_score)
        .bind(node.last_accessed_at)
        .bind(node.created_at)
        .bind(node.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 写入集合（按 `(name, domain)` 幂等）。
    ///
    /// 不再只按 `collection_id` upsert——那样每次新 UUID 都会新增一行
    /// （实测把 `blender_knowledge` 堆了 45 份）。现在同名同域已存在就跳过；
    /// 集合 ID 以库中已有的那条为准，避免同一份知识挂在两个 ID 下被重复召回。
    async fn write_collection(&self, collection: &Collection) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO memory_collections (collection_id, name, domain, description, created_at)
            SELECT ?,?,?,?,?
            WHERE NOT EXISTS (
                SELECT 1 FROM memory_collections WHERE name = ? AND domain = ?
            )
            ON CONFLICT(collection_id) DO UPDATE SET
                name = excluded.name,
                domain = excluded.domain,
                description = excluded.description
            "#,
        )
        .bind(&collection.collection_id)
        .bind(&collection.name)
        .bind(&collection.domain)
        .bind(&collection.description)
        .bind(collection.created_at)
        .bind(&collection.name)
        .bind(&collection.domain)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn search_fts(
        &self,
        query: &str,
        collection_id: Option<&str>,
        domain: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryNode>> {
        let subq = "%".to_string() + query + "%";
        let mut sql = String::from(
            r#"
            SELECT * FROM memory_nodes
            WHERE (title LIKE ?1 COLLATE NOCASE OR content LIKE ?1 COLLATE NOCASE OR summary LIKE ?1 COLLATE NOCASE)
            "#,
        );
        let mut idx = 2;
        if collection_id.is_some() {
            sql.push_str(&format!(" AND collection_id = ?{}", idx));
            idx += 1;
        }
        if domain.is_some() {
            sql.push_str(&format!(" AND domain = ?{}", idx));
            idx += 1;
        }
        sql.push_str(&format!(" ORDER BY length(content) LIMIT ?{}", idx));

        let mut q = sqlx::query(&sql).bind(&subq);
        if let Some(cid) = collection_id {
            q = q.bind(cid);
        }
        if let Some(d) = domain {
            q = q.bind(d);
        }
        q = q.bind(limit as i64);

        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(row_to_memory_node)
            .collect::<Result<Vec<_>>>()?)
    }

    async fn find_by_path(&self, path: &str) -> Result<Option<MemoryNode>> {
        let row = sqlx::query("SELECT * FROM memory_nodes WHERE path = ?1 LIMIT 1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(row_to_memory_node).transpose()?)
    }

    async fn find_children(&self, parent_id: &str) -> Result<Vec<MemoryNode>> {
        let rows =
            sqlx::query("SELECT * FROM memory_nodes WHERE parent_id = ?1 ORDER BY sort_order")
                .bind(parent_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .iter()
            .map(row_to_memory_node)
            .collect::<Result<Vec<_>>>()?)
    }

    async fn list_nodes(&self, collection_id: &str) -> Result<Vec<MemoryNode>> {
        let rows = sqlx::query("SELECT * FROM memory_nodes WHERE collection_id = ?1 ORDER BY path")
            .bind(collection_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(row_to_memory_node)
            .collect::<Result<Vec<_>>>()?)
    }

    async fn list_all_nodes(&self) -> Result<Vec<MemoryNode>> {
        let rows = sqlx::query("SELECT * FROM memory_nodes")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(row_to_memory_node)
            .collect::<Result<Vec<_>>>()?)
    }

    async fn list_all_collections(&self) -> Result<Vec<Collection>> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT collection_id, name, domain, description, created_at FROM memory_collections",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                Ok(Collection {
                    collection_id: r.try_get("collection_id")?,
                    name: r.try_get("name")?,
                    domain: r.try_get("domain")?,
                    description: r.try_get("description")?,
                    created_at: r.try_get("created_at")?,
                })
            })
            .collect::<Result<Vec<_>>>()?)
    }

    async fn delete_node(&self, node_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM memory_nodes WHERE node_id = ?1")
            .bind(node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_knowledge_bases(&self) -> Result<Vec<KnowledgeBase>> {
        let rows = sqlx::query(
            "SELECT id, name, description, tags, expert_id, created_at, updated_at FROM domain_kb ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_kb).collect::<Result<Vec<_>>>()?)
    }

    async fn create_knowledge_base(
        &self,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        sqlx::query(
            "INSERT INTO domain_kb (id, name, description, tags, expert_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(description)
        .bind(tags.to_string())
        .bind(expert_id)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
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

    async fn update_knowledge_base(
        &self,
        id: &str,
        name: &str,
        description: &str,
        tags: &serde_json::Value,
        expert_id: &str,
    ) -> Result<KnowledgeBase> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        sqlx::query(
            "UPDATE domain_kb SET name = ?1, description = ?2, tags = ?3, expert_id = ?4, updated_at = ?5 WHERE id = ?6",
        )
        .bind(name)
        .bind(description)
        .bind(tags.to_string())
        .bind(expert_id)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
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

    async fn delete_knowledge_base(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM domain_kb WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_chunks(&self, kb_id: &str) -> Result<Vec<KbChunk>> {
        let rows = sqlx::query(
            "SELECT id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at FROM kb_chunk WHERE kb_id = ?1 ORDER BY sort_order, created_at",
        )
        .bind(kb_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_chunk).collect::<Result<Vec<_>>>()?)
    }

    async fn create_chunk(&self, kb_id: &str, title: &str, content: &str) -> Result<KbChunk> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        use sha2::{Digest, Sha256};
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        sqlx::query(
            "INSERT INTO kb_chunk (id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at) VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(kb_id)
        .bind(title)
        .bind(content)
        .bind(&content_hash)
        .bind("active")
        .bind(0i64)
        .bind(serde_json::json!({}).to_string())
        .bind(now)
        .bind(now)
        .execute(&self.pool)
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

    async fn update_chunk(&self, id: &str, title: &str, content: &str) -> Result<KbChunk> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        use sha2::{Digest, Sha256};
        let content_hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        sqlx::query(
            "UPDATE kb_chunk SET title = ?1, content = ?2, content_hash = ?3, updated_at = ?4 WHERE id = ?5",
        )
        .bind(title)
        .bind(content)
        .bind(&content_hash)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(
            "SELECT id, kb_id, title, content, content_hash, status, sort_order, metadata, created_at, updated_at FROM kb_chunk WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => Ok(row_to_chunk(&r)?),
            None => anyhow::bail!("chunk not found: {}", id),
        }
    }

    async fn delete_chunk(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM kb_chunk WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn load_all(
        &self,
        adjacency: &HashMap<String, Vec<(String, EdgeKind, f32)>>,
        entity_to_chunks: &HashMap<String, Vec<String>>,
        chunk_to_entities: &HashMap<String, Vec<String>>,
        entity_feedback: &HashMap<String, f32>,
    ) -> Result<(
        HashMap<String, Vec<(String, EdgeKind, f32)>>,
        HashMap<String, Vec<String>>,
        HashMap<String, Vec<String>>,
        HashMap<String, f32>,
    )> {
        let mut adj = adjacency.clone();
        let mut e2c = entity_to_chunks.clone();
        let mut c2e = chunk_to_entities.clone();
        let mut ef = entity_feedback.clone();

        use sqlx::Row;
        let edge_rows =
            sqlx::query("SELECT from_entity, to_entity, edge_kind, weight FROM graph_edges")
                .fetch_all(&self.pool)
                .await?;
        for row in &edge_rows {
            let from: String = row.try_get("from_entity")?;
            let to: String = row.try_get("to_entity")?;
            let kind_str: String = row.try_get("edge_kind")?;
            let weight: f32 = row.try_get("weight")?;
            let kind = match kind_str.as_str() {
                // 兼容历史脏数据：早期写入端曾用小写 "manual"，
                // 不识别会被 fallback 分支误判为可衰减的 Learned 边。
                "Manual" | "manual" => EdgeKind::Manual,
                _ => EdgeKind::Learned,
            };
            adj.entry(from.clone())
                .or_default()
                .push((to.clone(), kind, weight));
            adj.entry(to.clone())
                .or_default()
                .push((from.clone(), kind, weight));
        }

        let ec_rows = sqlx::query("SELECT entity_id, chunk_id FROM graph_entity_chunks")
            .fetch_all(&self.pool)
            .await?;
        for row in &ec_rows {
            let entity_id: String = row.try_get("entity_id")?;
            let chunk_id: String = row.try_get("chunk_id")?;
            e2c.entry(entity_id.clone())
                .or_default()
                .push(chunk_id.clone());
            c2e.entry(chunk_id).or_default().push(entity_id);
        }

        let fb_rows = sqlx::query("SELECT entity_id, feedback_score FROM graph_entity_feedback")
            .fetch_all(&self.pool)
            .await?;
        for row in &fb_rows {
            let entity_id: String = row.try_get("entity_id")?;
            let score: f32 = row.try_get("feedback_score")?;
            ef.insert(entity_id, score);
        }

        tracing::info!(
            "SutraLibrary: Graph loaded from SQLite ({} edges, {} entity-chunk mappings, {} feedback scores)",
            adj.len(),
            e2c.len(),
            ef.len()
        );

        Ok((adj, e2c, c2e, ef))
    }

    async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()> {
        // 无向图：端点排序后再落盘，(A,B) 与 (B,A) 共用一行
        let (from, to) = normalize_edge_endpoints(from, to);
        sqlx::query(
            r#"
            INSERT INTO graph_edges (from_entity, to_entity, edge_kind, weight)
            VALUES (?,?,?,?)
            ON CONFLICT(from_entity, to_entity, edge_kind) DO UPDATE SET weight = excluded.weight
            "#,
        )
        .bind(from)
        .bind(to)
        .bind(kind)
        .bind(weight)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()> {
        let (from, to) = normalize_edge_endpoints(from, to);
        sqlx::query(
            "DELETE FROM graph_edges WHERE from_entity=?1 AND to_entity=?2 AND edge_kind=?3",
        )
        .bind(from)
        .bind(to)
        .bind(kind)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO graph_entity_chunks (entity_id, chunk_id) VALUES (?,?) ON CONFLICT DO NOTHING",
        )
        .bind(entity_id)
        .bind(chunk_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO graph_entity_feedback (entity_id, feedback_score)
            VALUES (?,?)
            ON CONFLICT(entity_id) DO UPDATE SET feedback_score = excluded.feedback_score
            "#,
        )
        .bind(entity_id)
        .bind(score)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        for (from, to, kind) in edges {
            if let Err(e) = PersistencePort::delete_edge(self, from, to, kind).await {
                tracing::warn!("SutraLibrary: SQLite batch delete edge failed: {}", e);
            }
        }
        Ok(())
    }
}

// ─── SQLite 图谱持久化端口 ────────────────────────────────────────
//
// `EntityGraph` 依赖的是窄接口 `GraphPersistence`。SQLite 后端的图谱能力
// 已在 `PersistencePort` 实现里齐备，这里逐一转发，避免 SQL 重复。
// 有此后端，无 PG 降级模式下运行时双写（实体-切片 / 建边 / 反馈）同样落盘。

#[async_trait]
impl GraphPersistence for SqliteStorage {
    async fn load_all(
        &self,
        adjacency: &HashMap<String, Vec<(String, EdgeKind, f32)>>,
        entity_to_chunks: &HashMap<String, Vec<String>>,
        chunk_to_entities: &HashMap<String, Vec<String>>,
        entity_feedback: &HashMap<String, f32>,
    ) -> Result<(
        HashMap<String, Vec<(String, EdgeKind, f32)>>,
        HashMap<String, Vec<String>>,
        HashMap<String, Vec<String>>,
        HashMap<String, f32>,
    )> {
        <SqliteStorage as PersistencePort>::load_all(
            self,
            adjacency,
            entity_to_chunks,
            chunk_to_entities,
            entity_feedback,
        )
        .await
    }
    async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()> {
        <SqliteStorage as PersistencePort>::write_edge(self, from, to, kind, weight).await
    }
    async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()> {
        <SqliteStorage as PersistencePort>::delete_edge(self, from, to, kind).await
    }
    async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()> {
        <SqliteStorage as PersistencePort>::write_entity_chunk(self, entity_id, chunk_id).await
    }
    async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()> {
        <SqliteStorage as PersistencePort>::write_entity_feedback(self, entity_id, score).await
    }
    async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        <SqliteStorage as PersistencePort>::batch_delete_edges(self, edges).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node(collection_id: &str, content: &str) -> MemoryNode {
        let now = chrono::Utc::now().timestamp();
        MemoryNode {
            node_id: uuid::Uuid::new_v4().to_string(),
            collection_id: collection_id.to_string(),
            domain: "general".to_string(),
            node_type: "note".to_string(),
            content_hash: String::new(),
            parent_id: None,
            path: format!("/{}", content.split_whitespace().next().unwrap_or("node")),
            depth: 0,
            sort_order: 0,
            title: content.to_string(),
            summary: String::new(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: "current".to_string(),
            snapshot_id: None,
            base_activation: 0.5,
            importance: 1,
            access_count: 0,
            feedback_score: 0.0,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    /// SQLite 降级后端核心闭环：建表 → 写节点 → 检索 → 知识库 CRUD
    #[tokio::test]
    async fn sqlite_fallback_crud_roundtrip() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        storage.ensure_tables().await.unwrap();

        // 集合 + 节点落盘
        let collection = Collection {
            collection_id: "c1".to_string(),
            name: "测试库".to_string(),
            domain: "general".to_string(),
            description: "无 PG 降级测试".to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        storage.write_collection(&collection).await.unwrap();
        let node = sample_node(&collection.collection_id, "Tokio 异步运行时");
        storage.write_node(&node).await.unwrap();

        // FTS 命中
        let hits = storage
            .search_fts("Tokio", Some(&collection.collection_id), None, 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].content, node.content);

        // 路径查找
        let by_path = storage.find_by_path(&node.path).await.unwrap();
        assert!(by_path.is_some());

        // 知识库 CRUD
        let kb = storage
            .create_knowledge_base(
                "rust-规范",
                "个人编码规范",
                &serde_json::json!(["rust"]),
                "expert-1",
            )
            .await
            .unwrap();
        let chunk = storage
            .create_chunk(&kb.id, "命名", "使用 snake_case")
            .await
            .unwrap();
        assert_eq!(storage.list_chunks(&kb.id).await.unwrap().len(), 1);
        assert_eq!(storage.list_knowledge_bases().await.unwrap().len(), 1);

        storage
            .update_chunk(&chunk.id, "命名规范", "一律 snake_case")
            .await
            .unwrap();
        storage.delete_chunk(&chunk.id).await.unwrap();
        storage.delete_knowledge_base(&kb.id).await.unwrap();
        assert_eq!(storage.list_knowledge_bases().await.unwrap().len(), 0);
    }

    /// 图谱持久化（SQLite 后端）：运行时双写落盘 → `load_all` 可回灌。
    ///
    /// 锁定 `GraphPersistence for SqliteStorage` 的转发链路。
    /// 回归背景：此前 SQLite 降级模式下 `EntityGraph` 无持久化后端，
    /// 实体-切片 / 边 / 反馈分只写内存，图谱三张表恒 0。
    #[tokio::test]
    async fn sqlite_graph_persistence_roundtrip() {
        let storage = SqliteStorage::open_in_memory().unwrap();
        storage.ensure_tables().await.unwrap();

        // 运行时双写：实体-切片映射 / 边 / 反馈分
        GraphPersistence::write_entity_chunk(&storage, "entity:A", "chunk:1")
            .await
            .unwrap();
        GraphPersistence::write_entity_chunk(&storage, "entity:B", "chunk:1")
            .await
            .unwrap();
        GraphPersistence::write_edge(&storage, "entity:A", "entity:B", "Learned", 0.5)
            .await
            .unwrap();
        GraphPersistence::write_entity_feedback(&storage, "entity:A", 0.7)
            .await
            .unwrap();

        let (adj, e2c, c2e, ef) = GraphPersistence::load_all(
            &storage,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .await
        .unwrap();

        assert!(adj.contains_key("entity:A"), "邻接表应含 entity:A");
        assert!(
            e2c.get("entity:A")
                .map(|v| v.contains(&"chunk:1".to_string()))
                .unwrap_or(false),
            "实体→切片索引应含 entity:A → chunk:1"
        );
        assert_eq!(
            c2e.get("chunk:1").map(|v| v.len()).unwrap_or(0),
            2,
            "切片→实体索引应含 2 个实体"
        );
        assert!(
            (ef.get("entity:A").copied().unwrap_or(0.0) - 0.7).abs() < 1e-6,
            "反馈分应回灌为 0.7"
        );
    }

    /// `EntityGraph` 接入 SQLite 后端后，运行时双写能落盘（端到端接线验证）。
    ///
    /// 契约：`load_persisted_data(Arc<SqliteStorage>)` 后调用图操作，
    /// 变更应异步写入 SQLite。回归背景：SQLite 模式下 `graph_store` 恒为
    /// `None`，双写分支永不进入，图谱表恒 0（只有启动回灌、无运行时增量）。
    #[tokio::test]
    async fn entity_graph_persists_to_sqlite() {
        use crate::sutra_library::recall::EntityGraph;
        use std::sync::Arc;

        let db_path = std::env::temp_dir().join(format!(
            "subhuti_graph_persist_{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        let storage = Arc::new(SqliteStorage::open(db_path.to_str().unwrap()).unwrap());
        storage.ensure_tables().await.unwrap();

        let graph = EntityGraph::new();
        graph.load_persisted_data(storage.clone()).await;

        // 手动边不依赖实体抽取正则，直击"双写是否接线"这一点
        graph.add_manual_edge(&"entity:A".to_string(), &"entity:B".to_string(), 0.5);

        // 双写由 tokio::spawn 异步执行，让渡一次调度等它落盘
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let (adj, _, _, _) = GraphPersistence::load_all(
            &*storage,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .await
        .unwrap();

        assert!(
            adj.contains_key("entity:A"),
            "SQLite 应落盘 entity:A→entity:B 的边，实际为空（运行时双写未接线）"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    /// 回灌后：边由「实体索引 + 当前建边规则」**派生**，而不是照搬库里的邻接表。
    ///
    /// 契约变更（09-14）：边不再从 `graph_edges` 回灌。库里那份邻接表可能是旧
    /// 规则（含大量 2 字碎片共现）生成的，照搬进来等于把历史噪声固化，所以
    /// 改为以 `graph_entity_chunks` 为唯一真相重建。本测试同时锁住两点：
    /// 1. 内存边数 = 由实体索引按共现规则派生的边数（不会因双向展开而虚高）；
    /// 2. 库里那些与实体索引不符的边**不会**进内存。
    #[tokio::test]
    async fn hydrate_derives_edges_from_entity_index() {
        use crate::sutra_library::recall::{EntityGraph, GraphPersistence};

        let path = "/tmp/subhuti_graph_hydrate_count_test.sqlite";
        let _ = std::fs::remove_file(path);
        let storage = std::sync::Arc::new(SqliteStorage::open(path).unwrap());
        storage.ensure_tables().await.unwrap();

        // 库里故意放 3 条边（模拟旧规则的产物）
        for (a, b) in [
            ("entity:AAA", "entity:BBB"),
            ("entity:BBB", "entity:CCC"),
            ("entity:CCC", "entity:AAA"),
        ] {
            GraphPersistence::write_edge(&*storage, a, b, "Manual", 0.5)
                .await
                .unwrap();
        }
        // 实体索引里只有两个实体挂在同一切片 → 按共现规则只应派生 1 条边
        for e in ["entity:AAA", "entity:BBB"] {
            GraphPersistence::write_entity_chunk(&*storage, e, "chunk:1")
                .await
                .unwrap();
        }

        let graph = EntityGraph::new();
        graph.load_persisted_data(storage.clone()).await;
        assert_eq!(
            graph.edge_count(),
            1,
            "边应由实体索引派生（2 个共现实体 → 1 条），而不是照搬库里的 3 条"
        );
        assert_eq!(
            graph.entity_chunk_pairs(),
            2,
            "实体-切片索引回灌后不应翻倍（基线用当前内存会让 load_all 叠加）"
        );

        let _ = std::fs::remove_file(path);
    }

    /// 无向语义：端点顺序不同不应产生两行，反向删除也应生效。
    ///
    /// `graph_edges` 主键带方向，而 `EntityGraph` 是无向图——不做端点归一化的话，
    /// 同一对实体会存成 (A,B) 和 (B,A) 两行，回灌时双向展开使边数成倍虚高，
    /// `delete_edge` 也只删得掉一半，重启后"复活"。
    #[tokio::test]
    async fn undirected_edge_is_stored_once_regardless_of_order() {
        let path = "/tmp/subhuti_graph_undirected_test.sqlite";
        let _ = std::fs::remove_file(path);
        let storage = SqliteStorage::open(path).unwrap();
        storage.ensure_tables().await.unwrap();

        // 同一对实体、相反顺序各写一次（后写的权重更高）
        GraphPersistence::write_edge(&storage, "entity:A", "entity:B", "Manual", 0.5)
            .await
            .unwrap();
        GraphPersistence::write_edge(&storage, "entity:B", "entity:A", "Manual", 0.9)
            .await
            .unwrap();

        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM graph_edges")
            .fetch_one(&storage.pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "无向边应只落一行（端点未归一化会让同一对实体存两行）");

        let (w,): (f32,) = sqlx::query_as("SELECT weight FROM graph_edges LIMIT 1")
            .fetch_one(&storage.pool)
            .await
            .unwrap();
        assert!(
            (w - 0.9).abs() < 1e-6,
            "反向写入应更新同一行的权重，实际 {}",
            w
        );

        // 反向删除必须能删掉它
        GraphPersistence::delete_edge(&storage, "entity:B", "entity:A", "Manual")
            .await
            .unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM graph_edges")
            .fetch_one(&storage.pool)
            .await
            .unwrap();
        assert_eq!(n, 0, "反向 delete_edge 应能删除归一化后的边");

        let _ = std::fs::remove_file(path);
    }

    /// 历史脏数据兼容：小写 "manual" 必须读回 `Manual`，而不是 fallback 的 `Learned`。
    ///
    /// 早期写入端用过小写（见 `EntityGraph::upsert_edge` 的双写调用）。
    /// 若被误判成 Learned，这些静态/共现边会被衰减任务当作可淘汰的学习边清掉。
    #[tokio::test]
    async fn legacy_lowercase_manual_kind_reads_back_as_manual() {
        let path = "/tmp/subhuti_graph_legacy_kind_test.sqlite";
        let _ = std::fs::remove_file(path);
        let storage = SqliteStorage::open(path).unwrap();
        storage.ensure_tables().await.unwrap();

        // 直接注入一行旧版格式（小写 kind）
        sqlx::query(
            "INSERT INTO graph_edges (from_entity, to_entity, edge_kind, weight) VALUES (?,?,?,?)",
        )
        .bind("entity:X")
        .bind("entity:Y")
        .bind("manual")
        .bind(0.4f32)
        .execute(&storage.pool)
        .await
        .unwrap();

        let (adj, _, _, _) = GraphPersistence::load_all(
            &storage,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        )
        .await
        .unwrap();

        let kinds: Vec<EdgeKind> = adj
            .get("entity:X")
            .expect("entity:X 应有邻接项")
            .iter()
            .map(|(_, k, _)| *k)
            .collect();
        assert!(
            kinds.iter().any(|k| matches!(k, EdgeKind::Manual)),
            "小写 manual 应被识别为 Manual，实际: {:?}",
            kinds
        );

        let _ = std::fs::remove_file(path);
    }
}
