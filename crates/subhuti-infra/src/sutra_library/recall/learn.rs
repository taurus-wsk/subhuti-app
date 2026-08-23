//! # 后台异步学习任务
//!
//! 查询链路只读，学习逻辑在每次召回完成之后后台执行。
//! 包含：共现熟练度更新、定时老化。

use crate::sutra_library::recall::graph::EntityGraph;
use crate::sutra_library::recall::{Candidate, ChunkUuid, EntityUuid};
use std::collections::HashSet;
use std::sync::Arc;

/// 召回完成后，后台执行共现学习
///
/// 1. 取出结果里面全部实体
/// 2. 实体两两共现，更新 Learned 熟练度边
pub async fn post_retrieve_learn_task(
    graph: Arc<EntityGraph>,
    chunk_ids: Vec<ChunkUuid>,
    chunk_to_entities: impl Fn(&[ChunkUuid]) -> HashSet<EntityUuid> + Send + 'static,
) {
    if chunk_ids.is_empty() {
        return;
    }

    // 在后台线程执行共现学习，不阻塞查询链路
    tokio::task::spawn_blocking(move || {
        let entities = chunk_to_entities(&chunk_ids);
        graph.observe_entity_cooccurrence(&entities);
    })
    .await
    .ok();
}

/// 定时老化任务：衰减长期未共现的 Learned 边权重
pub fn decay_learned_edges(graph: &EntityGraph, decay_factor: f32) {
    graph.decay_learned_edges(decay_factor);
}

/// 从候选列表中提取 chunk_ids
pub fn extract_chunk_ids(candidates: &[Candidate]) -> Vec<ChunkUuid> {
    candidates.iter().map(|c| c.chunk_id.clone()).collect()
}

/// 注册切片到图谱
pub fn register_chunks_to_graph(
    graph: &EntityGraph,
    nodes: &[crate::sutra_library::models::MemoryNode],
) {
    for node in nodes {
        graph.register_chunk(node);
    }
}
