//! # SpaceDepthStrategy 静态空间通路
//!
//! 仅依靠物理目录树（阁‑柜‑抽屉）召回邻居；
//! 实体只做层内排序，**不驱动扩散**。
//!
//! 硬性规则：
//! - 禁止图谱召回结果再投喂空间策略
//! - 空间排序锚点实体集合全程固定，使用原始种子切片实体集合
//! - 排序使用实体反馈加权分，正反馈实体排序靠前

use crate::sutra_library::models::MemoryNode;
use crate::sutra_library::recall::graph::EntityGraph;
use crate::sutra_library::recall::{collect_entities, ChunkUuid, EntityUuid, SpaceHit};
use crate::sutra_library::storage::MemoryStorage;
use std::collections::HashSet;
use std::sync::Arc;

/// 静态空间深度策略
pub struct SpaceDepthStrategy {
    memory: Arc<MemoryStorage>,
    /// 实体图谱（可选，用于反馈加权的实体重合度排序）
    entity_graph: Option<Arc<EntityGraph>>,
}

impl SpaceDepthStrategy {
    pub fn new(memory: Arc<MemoryStorage>) -> Self {
        Self {
            memory,
            entity_graph: None,
        }
    }

    /// 设置实体图谱（启用反馈加权的实体排序）
    pub fn with_entity_graph(mut self, graph: Arc<EntityGraph>) -> Self {
        self.entity_graph = Some(graph);
        self
    }

    /// 执行静态空间扩散
    ///
    /// - `seed_chunks`: 原始种子切片 ID 列表
    /// - `root_entity_set`: 全局固定根实体集合（由 BaseSearch 结果提取）
    /// - `max_tree_distance`: 最大树距离（0=同抽屉, 1=同柜子, 2=同阁）
    /// - `per_level_limit`: 每层截断数
    pub fn expand(
        &self,
        seed_chunks: &[ChunkUuid],
        root_entity_set: &HashSet<EntityUuid>,
        max_tree_distance: u32,
        per_level_limit: usize,
    ) -> Vec<SpaceHit> {
        if max_tree_distance == 0 {
            return Vec::new();
        }

        // 1. 收集种子切片的路径信息
        let seed_nodes: Vec<MemoryNode> = seed_chunks
            .iter()
            .filter_map(|id| self.memory.read_node(id))
            .collect();

        if seed_nodes.is_empty() {
            return Vec::new();
        }

        // 2. 收集所有种子路径的「目录前缀」
        //    树距离 0 = 同 collection 同层（同抽屉）
        //    树距离 1 = 同 collection（同柜子）
        //    树距离 2 = 同 domain（同阁）
        let seed_collections: HashSet<String> =
            seed_nodes.iter().map(|n| n.collection_id.clone()).collect();
        let seed_domains: HashSet<String> = seed_nodes.iter().map(|n| n.domain.clone()).collect();

        // 3. 逐层向外扩散
        let mut all_hits: Vec<SpaceHit> = Vec::new();
        let mut seen: HashSet<ChunkUuid> = seed_chunks.iter().cloned().collect();

        // 距离 1：同 collection 的其他节点（同柜子）
        if max_tree_distance >= 1 {
            let mut level_candidates: Vec<(ChunkUuid, MemoryNode)> = Vec::new();
            for col_id in &seed_collections {
                let all_nodes = self.memory.get_all_nodes();
                for node in &all_nodes {
                    if node.collection_id == *col_id && !seen.contains(&node.node_id) {
                        level_candidates.push((node.node_id.clone(), node.clone()));
                    }
                }
            }
            // 实体排序：按与 root_entity_set 的实体加权重合度降序
            self.sort_by_entity_overlap(&mut level_candidates, root_entity_set);
            level_candidates.truncate(per_level_limit);

            for (chunk_id, _node) in &level_candidates {
                seen.insert(chunk_id.clone());
                all_hits.push(SpaceHit {
                    chunk_id: chunk_id.clone(),
                    bonus_score: 0.15,
                    tree_distance: 1,
                });
            }
        }

        // 距离 2：同 domain 的其他节点（同阁）
        if max_tree_distance >= 2 {
            let mut level_candidates: Vec<(ChunkUuid, MemoryNode)> = Vec::new();
            for domain in &seed_domains {
                let all_nodes = self.memory.get_all_nodes();
                for node in &all_nodes {
                    if node.domain == *domain && !seen.contains(&node.node_id) {
                        level_candidates.push((node.node_id.clone(), node.clone()));
                    }
                }
            }
            self.sort_by_entity_overlap(&mut level_candidates, root_entity_set);
            level_candidates.truncate(per_level_limit);

            for (chunk_id, _node) in &level_candidates {
                seen.insert(chunk_id.clone());
                all_hits.push(SpaceHit {
                    chunk_id: chunk_id.clone(),
                    bonus_score: 0.05,
                    tree_distance: 2,
                });
            }
        }

        all_hits
    }

    /// 按与 root_entity_set 的实体加权重合度降序排序
    ///
    /// 正反馈实体的重合权重更高，使 liked 内容在空间通路中排名更靠前。
    fn sort_by_entity_overlap(
        &self,
        candidates: &mut Vec<(ChunkUuid, MemoryNode)>,
        root_entity_set: &HashSet<EntityUuid>,
    ) {
        if root_entity_set.is_empty() {
            return;
        }
        candidates.sort_by(|a, b| {
            let entities_a = collect_entities(&[a.1.clone()]);
            let entities_b = collect_entities(&[b.1.clone()]);
            let overlap_a: f32 = entities_a
                .iter()
                .filter(|e| root_entity_set.contains(*e))
                .map(|e| self.entity_weight(e))
                .sum();
            let overlap_b: f32 = entities_b
                .iter()
                .filter(|e| root_entity_set.contains(*e))
                .map(|e| self.entity_weight(e))
                .sum();
            overlap_b
                .partial_cmp(&overlap_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// 获取实体权重（反馈加权分，默认 1.0）
    fn entity_weight(&self, entity: &EntityUuid) -> f32 {
        match &self.entity_graph {
            Some(g) => g.get_entity_weighted_score(entity),
            None => 1.0,
        }
    }
}
