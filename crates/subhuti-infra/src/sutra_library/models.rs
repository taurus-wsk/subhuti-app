//! # 藏经阁数据模型
//!
//! 通用记忆节点模型，全领域、全分层复用。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 全局唯一通用记忆节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    // 唯一标识
    pub node_id: String,
    pub collection_id: String,
    pub domain: String,
    pub node_type: String,
    pub content_hash: String,

    // 树结构纵向
    pub parent_id: Option<String>,
    pub path: String,
    pub depth: u32,
    pub sort_order: u32,

    // 内容
    pub title: String,
    pub summary: String,
    pub content: String,
    pub metadata: serde_json::Value,

    // 图谱横向关联
    pub refs_out: Vec<RefEdge>,
    pub refs_in: Vec<RefEdge>,

    // 版本快照
    pub version_tag: String,
    pub snapshot_id: Option<String>,

    // 热度生命周期
    pub base_activation: f32,
    pub importance: u8,
    pub access_count: u32,
    pub feedback_score: f32,
    pub last_accessed_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 轻量跨域关联边
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefEdge {
    pub target_node_id: String,
    pub target_collection_id: String,
    pub edge_type: RefType,
    pub weight: f32,
}

/// 关联类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefType {
    Calls,
    Implements,
    DependsOn,
    Uses,
    Related,
}

/// 语义切片（领域适配器输出）
#[derive(Debug, Clone)]
pub struct SemanticChunk {
    pub title: String,
    pub content: String,
    pub node_type: String,
    pub parent_path: Option<String>,
    pub sort_order: u32,
    pub metadata: serde_json::Value,
}

/// 解析上下文（领域适配器输入）
#[derive(Debug, Clone)]
pub struct ParseContext {
    pub collection_id: String,
    pub domain: String,
    pub source_path: Option<String>,
    pub extra: HashMap<String, String>,
}

/// 上下文模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMode {
    Precise,
    Standard,
    Deep,
}

/// 带得分的检索候选项
#[derive(Debug, Clone)]
pub struct ScoredNode {
    pub node: MemoryNode,
    pub score: f32,
    pub score_detail: ScoreDetail,
    pub matched_text: Vec<String>,
}

/// 得分明细（可解释性）
#[derive(Debug, Clone, Serialize)]
pub struct ScoreDetail {
    pub bm25_score: f32,
    pub tree_match_score: f32,
    pub hotness_bonus: f32,
    pub feedback_bonus: f32,
    pub final_score: f32,
}

/// 集合元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Collection {
    pub collection_id: String,
    pub name: String,
    pub domain: String,
    pub description: String,
    pub created_at: i64,
}

/// 快照元信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub snapshot_id: String,
    pub collection_id: String,
    pub name: String,
    pub version_tag: String,
    pub description: String,
    pub created_at: i64,
}

/// 记忆统计
#[derive(Debug, Clone, Serialize)]
pub struct SutraStats {
    pub total_nodes: usize,
    pub hot_nodes: usize,
    pub cold_nodes: usize,
    pub collections: usize,
    pub edges: usize,
    pub snapshots: usize,
}

/// 三级检索结果
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    pub nodes: Vec<ScoredNode>,
    pub source: RetrievalSource,
    pub total_candidates: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalSource {
    SessionMemory,
    HotMemory,
    ColdStorage,
}

/// 检索请求
#[derive(Debug, Clone)]
pub struct RetrievalQuery {
    pub text: String,
    pub collection_id: Option<String>,
    pub domain: Option<String>,
    pub node_type: Option<String>,
    pub limit: usize,
    pub mode: ContextMode,
}
