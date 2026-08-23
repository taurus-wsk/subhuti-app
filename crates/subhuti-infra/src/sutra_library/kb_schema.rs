//! # 藏经阁知识库标准数据模型
//!
//! 按照 ER 设计：一张 domain_kb = 一个独立藏经阁空间，
//! 切片、知识树、图谱、任务会话、运行日志、待审核知识全部挂在这个空间下。
//!
//! ```text
//! domain_kb (知识库)
//!     ├─ kb_chunk 切片
//!     ├─ knowledge_tree_node 知识树
//!     ├─ kg_entity + kg_relation 知识图谱
//!     ├─ chat_session → chat_message 任务会话
//!     ├─ engine_run_log 执行日志
//!     └─ temp_knowledge_buffer 待审核知识
//! ```

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ─── 领域常量 ────────────────────────────────────────────────

/// 切片状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkStatus {
    Active,
    Archived,
    Deprecated,
}

/// 待审核知识状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BufferStatus {
    Pending,
    Approved,
    Rejected,
}

// ─── 模型结构体 ─────────────────────────────────────────────

/// 知识库（domain_kb）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KnowledgeBase {
    pub id: String,
    pub name: String,
    pub description: String,
    /// 自定义标签，JSON 字符串数组，如 ["运动控制", "伺服"]
    pub tags: serde_json::Value,
    /// 关联的专家 id（可为空字符串表示未关联专家）
    pub expert_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 切片（kb_chunk）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct KbChunk {
    pub id: String,
    pub kb_id: String,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub status: String, // ChunkStatus 的字符串表示
    pub sort_order: i32,
    pub metadata: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 知识树节点（knowledge_tree_node）
///
/// 邻接表结构，parent_id 自关联。
/// 叶子节点（无子节点）可以挂载切片，父节点只做层级分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeTreeNode {
    pub id: String,
    pub kb_id: String,
    /// 父节点 id；根节点 parent_id = null
    pub parent_id: Option<String>,
    /// 节点名称，领域概念名词，如"伺服PID调参"
    pub node_name: String,
    /// 可选备注，描述该分类存放什么知识
    pub node_desc: Option<String>,
    /// 同父节点下的排序序号
    pub sort_index: i32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 树节点 — 切片 多对多关联中间表
///
/// 一个切片可以挂载 0/1/多个树节点；
/// 一个叶子节点可以挂载无数切片。
/// 切片不在此表中 → 游离切片，不参与 SpaceDepth 空间纵深召回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeChunkLink {
    pub id: String,
    pub kb_id: String,
    pub tree_node_id: String,
    pub chunk_id: String,
    pub created_at: i64,
}

/// 图谱实体（kg_entity）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgEntity {
    pub id: String,
    pub kb_id: String,
    pub name: String,
    pub entity_type: String,
    pub metadata: serde_json::Value,
    pub created_at: i64,
}

/// 图谱关系（kg_relation）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgRelation {
    pub id: i64,
    pub kb_id: String,
    pub from_entity: String,
    pub to_entity: String,
    pub relation_type: String,
    pub weight: f32,
    pub created_at: i64,
}

/// 任务会话（chat_session）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub kb_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 聊天消息（chat_message）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub session_id: String,
    pub role: String, // 'user' | 'assistant'
    pub content: String,
    pub created_at: i64,
}

/// 执行日志（engine_run_log）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRunLog {
    pub id: i64,
    pub kb_id: String,
    pub session_id: Option<String>,
    pub query_hash: String,
    pub query: String,
    pub task_success: bool,
    pub output: String,
    pub created_at: i64,
}

/// 待审核知识（temp_knowledge_buffer）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TempKnowledgeBuffer {
    pub id: i64,
    pub kb_id: String,
    pub content: String,
    pub source: String,
    pub status: String, // BufferStatus 的字符串表示
    pub metadata: serde_json::Value,
    pub created_at: i64,
}

// ─── DDL 建表语句 ────────────────────────────────────────────

/// 所有 DDL 按依赖顺序排列（先建父表，再建子表）
pub const KB_SCHEMA_DDLS: &[&str] = &[
    // 1. domain_kb
    r#"
    CREATE TABLE IF NOT EXISTS domain_kb (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        tags JSONB NOT NULL DEFAULT '[]',
        expert_id TEXT NOT NULL DEFAULT '',
        created_at BIGINT NOT NULL,
        updated_at BIGINT NOT NULL
    )"#,
    // 2. kb_chunk
    r#"
    CREATE TABLE IF NOT EXISTS kb_chunk (
        id TEXT PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        title TEXT NOT NULL DEFAULT '',
        content TEXT NOT NULL,
        content_hash TEXT NOT NULL DEFAULT '',
        status TEXT NOT NULL DEFAULT 'active',
        sort_order INT NOT NULL DEFAULT 0,
        metadata JSONB NOT NULL DEFAULT '{}',
        created_at BIGINT NOT NULL,
        updated_at BIGINT NOT NULL
    )"#,
    // 3. knowledge_tree_node（邻接表，parent_id 自关联）
    r#"
    CREATE TABLE IF NOT EXISTS knowledge_tree_node (
        id TEXT PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        parent_id TEXT REFERENCES knowledge_tree_node(id) ON DELETE CASCADE,
        node_name VARCHAR(255) NOT NULL,
        node_desc TEXT,
        sort_index INT NOT NULL DEFAULT 0,
        created_at BIGINT NOT NULL,
        updated_at BIGINT NOT NULL
    )"#,
    // 4. tree_chunk_link（树节点-切片 多对多中间表）
    r#"
    CREATE TABLE IF NOT EXISTS tree_chunk_link (
        id TEXT PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        tree_node_id TEXT NOT NULL REFERENCES knowledge_tree_node(id) ON DELETE CASCADE,
        chunk_id TEXT NOT NULL REFERENCES kb_chunk(id) ON DELETE CASCADE,
        created_at BIGINT NOT NULL,
        UNIQUE(tree_node_id, chunk_id)
    )"#,
    // 5. kg_entity
    r#"
    CREATE TABLE IF NOT EXISTS kg_entity (
        id TEXT PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        entity_type TEXT NOT NULL DEFAULT 'default',
        metadata JSONB NOT NULL DEFAULT '{}',
        created_at BIGINT NOT NULL
    )"#,
    // 5. kg_relation
    r#"
    CREATE TABLE IF NOT EXISTS kg_relation (
        id BIGSERIAL PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        from_entity TEXT NOT NULL REFERENCES kg_entity(id) ON DELETE CASCADE,
        to_entity TEXT NOT NULL REFERENCES kg_entity(id) ON DELETE CASCADE,
        relation_type TEXT NOT NULL,
        weight REAL NOT NULL DEFAULT 0.3,
        created_at BIGINT NOT NULL
    )"#,
    // 6. chat_session
    r#"
    CREATE TABLE IF NOT EXISTS chat_session (
        id TEXT PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        created_at BIGINT NOT NULL,
        updated_at BIGINT NOT NULL
    )"#,
    // 7. chat_message
    r#"
    CREATE TABLE IF NOT EXISTS chat_message (
        id BIGSERIAL PRIMARY KEY,
        session_id TEXT NOT NULL REFERENCES chat_session(id) ON DELETE CASCADE,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        created_at BIGINT NOT NULL
    )"#,
    // 8. engine_run_log
    r#"
    CREATE TABLE IF NOT EXISTS engine_run_log (
        id BIGSERIAL PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        session_id TEXT REFERENCES chat_session(id) ON DELETE SET NULL,
        query_hash TEXT NOT NULL DEFAULT '',
        query TEXT NOT NULL,
        task_success BOOLEAN NOT NULL DEFAULT true,
        output TEXT NOT NULL DEFAULT '',
        created_at BIGINT NOT NULL
    )"#,
    // 9. temp_knowledge_buffer
    r#"
    CREATE TABLE IF NOT EXISTS temp_knowledge_buffer (
        id BIGSERIAL PRIMARY KEY,
        kb_id TEXT NOT NULL REFERENCES domain_kb(id) ON DELETE CASCADE,
        content TEXT NOT NULL,
        source TEXT NOT NULL DEFAULT 'manual',
        status TEXT NOT NULL DEFAULT 'pending',
        metadata JSONB NOT NULL DEFAULT '{}',
        created_at BIGINT NOT NULL
    )"#,
    // 兼容迁移：为已存在的 domain_kb 表补充列（幂等）
    // tags 列早期建表时可能缺失（CREATE TABLE IF NOT EXISTS 不会改已存在表），须显式补充
    r#"ALTER TABLE domain_kb ADD COLUMN IF NOT EXISTS tags JSONB NOT NULL DEFAULT '[]'"#,
    r#"ALTER TABLE domain_kb ADD COLUMN IF NOT EXISTS expert_id TEXT NOT NULL DEFAULT ''"#,
];

/// 索引 DDL（建表完成后执行）
pub const KB_SCHEMA_INDEXES: &[&str] = &[
    // kb_chunk 索引
    "CREATE INDEX IF NOT EXISTS idx_kb_chunk_kb ON kb_chunk(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_kb_chunk_status ON kb_chunk(status)",
    "CREATE INDEX IF NOT EXISTS idx_kb_chunk_hash ON kb_chunk(content_hash)",
    // knowledge_tree_node 索引
    "CREATE INDEX IF NOT EXISTS idx_ktn_kb ON knowledge_tree_node(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_ktn_parent ON knowledge_tree_node(parent_id)",
    "CREATE INDEX IF NOT EXISTS idx_ktn_name ON knowledge_tree_node(node_name)",
    // tree_chunk_link 索引
    "CREATE INDEX IF NOT EXISTS idx_tcl_kb ON tree_chunk_link(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_tcl_node ON tree_chunk_link(tree_node_id)",
    "CREATE INDEX IF NOT EXISTS idx_tcl_chunk ON tree_chunk_link(chunk_id)",
    // kg_entity 索引
    "CREATE INDEX IF NOT EXISTS idx_kg_entity_kb ON kg_entity(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_kg_entity_name ON kg_entity(name)",
    // kg_relation 索引
    "CREATE INDEX IF NOT EXISTS idx_kg_relation_kb ON kg_relation(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_kg_relation_from ON kg_relation(from_entity)",
    "CREATE INDEX IF NOT EXISTS idx_kg_relation_to ON kg_relation(to_entity)",
    // chat_session 索引
    "CREATE INDEX IF NOT EXISTS idx_chat_session_kb ON chat_session(kb_id)",
    // chat_message 索引
    "CREATE INDEX IF NOT EXISTS idx_chat_message_session ON chat_message(session_id)",
    // engine_run_log 索引
    "CREATE INDEX IF NOT EXISTS idx_engine_run_log_kb ON engine_run_log(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_engine_run_log_session ON engine_run_log(session_id)",
    "CREATE INDEX IF NOT EXISTS idx_engine_run_log_ts ON engine_run_log(created_at)",
    // temp_knowledge_buffer 索引
    "CREATE INDEX IF NOT EXISTS idx_temp_kb_kb ON temp_knowledge_buffer(kb_id)",
    "CREATE INDEX IF NOT EXISTS idx_temp_kb_status ON temp_knowledge_buffer(status)",
];
