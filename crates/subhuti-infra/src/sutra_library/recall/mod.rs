//! # 藏经阁召回引擎（高层整合）
//!
//! 四大召回数据源 + 五阶段流水线：
//! 1. BaseSearch 基础检索
//! 2. SpaceDepthStrategy 静态空间通路
//! 3. GraphPassageStrategy 一阶图谱BFS
//! 4. GraphPassageStrategy 二阶图谱BFS（可选）
//!
//! 架构原则：职责单一、通路解耦、可开关、查询只读，学习逻辑后置异步

pub mod base_search;
pub mod config;
pub mod entity_extractor;
pub mod graph;
pub mod learn;
pub mod pipeline;
pub mod query;
pub mod scoring;
pub mod space;
pub mod tantivy_index;

pub use base_search::BaseSearch;
pub use config::LibraryRetrieveConfig;
pub use entity_extractor::EntityExtractor;
pub use graph::{EdgeKind, EntityGraph, GraphPassageStrategy};
pub use pipeline::library_retrieve;
pub use query::QueryAnalyzer;
pub use scoring::NormalizeStrategy;
pub use space::SpaceDepthStrategy;
pub use tantivy_index::{SearchFilters, TantivyIndex};

use crate::sutra_library::models::MemoryNode;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// 切片 ID（复用 MemoryNode.node_id）
pub type ChunkUuid = String;

/// 实体 ID
pub type EntityUuid = String;

// ─── 通路返回数据结构 ───────────────────────────────────────

/// 基础检索单条结果
#[derive(Debug, Clone)]
pub struct BaseHit {
    pub chunk_id: ChunkUuid,
    pub base_score: f32,
}

/// 空间策略返回结果
#[derive(Debug, Clone)]
pub struct SpaceHit {
    pub chunk_id: ChunkUuid,
    pub bonus_score: f32,
    pub tree_distance: u32,
}

/// 图谱策略返回结果
#[derive(Debug, Clone)]
pub struct GraphHit {
    pub chunk_id: ChunkUuid,
    pub bonus_score: f32,
    pub edge_sources: Vec<RelationSource>,
    pub entity_path: Option<Vec<EntityUuid>>,
}

/// 图谱边的关系来源
#[derive(Debug, Clone)]
pub struct RelationSource {
    pub from_entity: EntityUuid,
    pub to_entity: EntityUuid,
    pub kind: EdgeKind,
    pub weight: f32,
}

/// 最终合并后的通用候选项
#[derive(Debug, Clone)]
pub struct Candidate {
    pub chunk_id: ChunkUuid,
    pub base_score: f32,
    pub bonus_score: f32,
    /// 记录来自哪些扩展通路，用于调试
    pub source_flags: Vec<RetrieveSource>,
}

/// 召回来源标记
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetrieveSource {
    Base,
    Space,
    GraphFirstPass,
    GraphSecondPass,
}

// ─── 全局实体提取器实例（懒加载） ───────────────────────────

use std::sync::OnceLock;

fn global_extractor() -> &'static EntityExtractor {
    static EXTRACTOR: OnceLock<EntityExtractor> = OnceLock::new();
    EXTRACTOR.get_or_init(|| EntityExtractor::new())
}

/// 从 MemoryNode 提取实体 ID 列表（使用增强实体提取器）
pub fn extract_entities_from_node(node: &MemoryNode) -> Vec<EntityUuid> {
    global_extractor().extract_from_node(node)
}

/// 从多个节点收集实体集合
pub fn collect_entities(nodes: &[MemoryNode]) -> HashSet<EntityUuid> {
    global_extractor().collect_entities(nodes)
}
