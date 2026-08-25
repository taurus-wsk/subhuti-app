//! # GraphPassageStrategy 图谱通路
//!
//! 实体图 BFS 遍历，支持 Manual（手动/LLM）和 Learned（共现熟练度）两类边。
//! 查询链路只读，Learned 边由后台异步任务更新。
//!
//! 底层使用 petgraph::graph::UnGraph 作为图数据结构。

use crate::sutra_library::models::MemoryNode;
use crate::sutra_library::recall::{
    extract_entities_from_node, ChunkUuid, EntityUuid, GraphHit, RelationSource,
};
use crate::sutra_library::storage::PgGraphStorage;
use petgraph::graph::{EdgeIndex, NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

/// 边类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// 手动/LLM 定义的关系（如函数调用、依赖）
    Manual,
    /// 共现熟练度关系（由后台异步学习更新）
    Learned,
}

/// petgraph 边权重
#[derive(Debug, Clone)]
struct EdgeWeight {
    kind: EdgeKind,
    weight: f32,
}

/// 实体图谱（基于 petgraph::graph::UnGraph）
pub struct EntityGraph {
    /// petgraph 无向图: 节点 = EntityUuid, 边 = (EdgeKind, weight)
    graph: RwLock<UnGraph<EntityUuid, EdgeWeight>>,
    /// 实体 → petgraph 节点索引
    node_indices: RwLock<HashMap<EntityUuid, NodeIndex>>,
    /// 实体 → 切片（反向索引）
    entity_to_chunks: RwLock<HashMap<EntityUuid, Vec<ChunkUuid>>>,
    /// 切片 → 实体
    chunk_to_entities: RwLock<HashMap<ChunkUuid, Vec<EntityUuid>>>,
    /// 实体级反馈累计分（用于反馈反哺图谱边权重）
    entity_feedback: RwLock<HashMap<EntityUuid, f32>>,
    /// PG 持久化适配器（可选，有则异步双写）
    pg_graph: RwLock<Option<Arc<PgGraphStorage>>>,
}

impl EntityGraph {
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(UnGraph::new_undirected()),
            node_indices: RwLock::new(HashMap::new()),
            entity_to_chunks: RwLock::new(HashMap::new()),
            chunk_to_entities: RwLock::new(HashMap::new()),
            entity_feedback: RwLock::new(HashMap::new()),
            pg_graph: RwLock::new(None),
        }
    }

    // ─── 内部辅助方法 ───────────────────────────────────────────

    /// 获取或创建实体对应的 NodeIndex
    fn get_or_create_node(
        graph: &mut UnGraph<EntityUuid, EdgeWeight>,
        indices: &mut HashMap<EntityUuid, NodeIndex>,
        entity: &EntityUuid,
    ) -> NodeIndex {
        if let Some(&idx) = indices.get(entity) {
            idx
        } else {
            let idx = graph.add_node(entity.clone());
            indices.insert(entity.clone(), idx);
            idx
        }
    }

    /// 获取实体对应的 NodeIndex（只读，不创建）
    fn get_node_index(
        indices: &HashMap<EntityUuid, NodeIndex>,
        entity: &EntityUuid,
    ) -> Option<NodeIndex> {
        indices.get(entity).copied()
    }

    /// 将 petgraph 邻接表转换为 HashMap 格式（用于 PG 持久化）
    fn adjacency_to_hashmap(
        graph: &UnGraph<EntityUuid, EdgeWeight>,
        indices: &HashMap<EntityUuid, NodeIndex>,
    ) -> HashMap<EntityUuid, Vec<(EntityUuid, EdgeKind, f32)>> {
        let mut adj: HashMap<EntityUuid, Vec<(EntityUuid, EdgeKind, f32)>> = HashMap::new();
        for (entity, &idx) in indices {
            let neighbors: Vec<(EntityUuid, EdgeKind, f32)> = graph
                .edges(idx)
                .map(|e| {
                    let target = if e.target() == idx {
                        e.source()
                    } else {
                        e.target()
                    };
                    let target_entity = graph[target].clone();
                    (target_entity, e.weight().kind, e.weight().weight)
                })
                .collect();
            adj.insert(entity.clone(), neighbors);
        }
        adj
    }

    /// 从 HashMap 邻接表重建 petgraph 图
    fn hashmap_to_graph(
        adj: &HashMap<EntityUuid, Vec<(EntityUuid, EdgeKind, f32)>>,
    ) -> (
        UnGraph<EntityUuid, EdgeWeight>,
        HashMap<EntityUuid, NodeIndex>,
    ) {
        let mut graph = UnGraph::new_undirected();
        let mut indices = HashMap::new();

        for (entity, neighbors) in adj {
            let from = Self::get_or_create_node(&mut graph, &mut indices, entity);
            for (neighbor, kind, weight) in neighbors {
                let to = Self::get_or_create_node(&mut graph, &mut indices, neighbor);
                graph.add_edge(
                    from,
                    to,
                    EdgeWeight {
                        kind: *kind,
                        weight: *weight,
                    },
                );
            }
        }

        (graph, indices)
    }

    // ─── PG 持久化 ─────────────────────────────────────────────

    /// 设置 PG 持久化适配器，并异步加载已有数据到内存
    pub async fn load_pg_data(&self, pg: Arc<PgGraphStorage>) {
        *self.pg_graph.write().unwrap() = Some(pg.clone());

        // 先复制当前内存数据（释放锁），再异步加载 PG 数据
        let (adj_copy, e2c_copy, c2e_copy, ef_copy) = {
            let graph = self.graph.read().unwrap();
            let indices = self.node_indices.read().unwrap();
            let adj = Self::adjacency_to_hashmap(&graph, &indices);
            let e2c = self.entity_to_chunks.read().unwrap().clone();
            let c2e = self.chunk_to_entities.read().unwrap().clone();
            let ef = self.entity_feedback.read().unwrap().clone();
            (adj, e2c, c2e, ef)
        };

        match pg.load_all(&adj_copy, &e2c_copy, &c2e_copy, &ef_copy).await {
            Ok((adj, e2c, c2e, ef)) => {
                let (new_graph, new_indices) = Self::hashmap_to_graph(&adj);
                *self.graph.write().unwrap() = new_graph;
                *self.node_indices.write().unwrap() = new_indices;
                *self.entity_to_chunks.write().unwrap() = e2c;
                *self.chunk_to_entities.write().unwrap() = c2e;
                *self.entity_feedback.write().unwrap() = ef;
                tracing::info!("SutraLibrary: EntityGraph PG data loaded successfully");
            }
            Err(e) => {
                tracing::warn!(
                    "SutraLibrary: EntityGraph PG load failed, using memory only: {}",
                    e
                );
            }
        }
    }

    /// 合并另一个 EntityGraph 的全部数据到当前实例
    pub fn merge_data_from(&self, source: &EntityGraph) {
        // 合并邻接表（通过 petgraph 边操作）
        {
            let source_graph = source.graph.read().unwrap();
            let source_indices = source.node_indices.read().unwrap();
            let mut target_graph = self.graph.write().unwrap();
            let mut target_indices = self.node_indices.write().unwrap();

            for (entity, &src_idx) in source_indices.iter() {
                let target_from =
                    Self::get_or_create_node(&mut target_graph, &mut target_indices, entity);
                for edge in source_graph.edges(src_idx) {
                    let target_idx = if edge.target() == src_idx {
                        edge.source()
                    } else {
                        edge.target()
                    };
                    let target_entity = source_graph[target_idx].clone();
                    let target_to = Self::get_or_create_node(
                        &mut target_graph,
                        &mut target_indices,
                        &target_entity,
                    );
                    // 检查是否已有相同类型的边
                    let has_edge = target_graph.edges(target_from).any(|e| {
                        let other = if e.target() == target_from {
                            e.source()
                        } else {
                            e.target()
                        };
                        other == target_to && e.weight().kind == edge.weight().kind
                    });
                    if !has_edge {
                        target_graph.add_edge(
                            target_from,
                            target_to,
                            EdgeWeight {
                                kind: edge.weight().kind,
                                weight: edge.weight().weight,
                            },
                        );
                    }
                }
            }
        }
        // 合并实体 → 切片索引
        {
            let source_e2c = source.entity_to_chunks.read().unwrap();
            let mut target_e2c = self.entity_to_chunks.write().unwrap();
            for (entity, chunks) in source_e2c.iter() {
                let entry = target_e2c.entry(entity.clone()).or_default();
                for chunk in chunks {
                    if !entry.contains(chunk) {
                        entry.push(chunk.clone());
                    }
                }
            }
        }
        // 合并切片 → 实体索引
        {
            let source_c2e = source.chunk_to_entities.read().unwrap();
            let mut target_c2e = self.chunk_to_entities.write().unwrap();
            for (chunk, entities) in source_c2e.iter() {
                let entry = target_c2e.entry(chunk.clone()).or_default();
                for entity in entities {
                    if !entry.contains(entity) {
                        entry.push(entity.clone());
                    }
                }
            }
        }
        // 合并反馈分（以 source 为准）
        {
            let source_fb = source.entity_feedback.read().unwrap();
            let mut target_fb = self.entity_feedback.write().unwrap();
            for (entity, score) in source_fb.iter() {
                target_fb.insert(entity.clone(), *score);
            }
        }
    }

    // ─── 核心操作 ───────────────────────────────────────────────

    /// 注册切片到图谱（提取实体并建立索引）
    pub fn register_chunk(&self, node: &MemoryNode) {
        let entities = extract_entities_from_node(node);
        let chunk_id = node.node_id.clone();

        // 更新 chunk → entities
        self.chunk_to_entities
            .write()
            .unwrap()
            .insert(chunk_id.clone(), entities.clone());

        // 更新 entity → chunks
        let mut e2c = self.entity_to_chunks.write().unwrap();
        for entity in &entities {
            e2c.entry(entity.clone())
                .or_default()
                .push(chunk_id.clone());
        }
        drop(e2c);

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            let ent = entities.clone();
            let cid = chunk_id.clone();
            tokio::task::spawn(async move {
                for entity in &ent {
                    if let Err(e) = pg.write_entity_chunk(entity, &cid).await {
                        tracing::warn!("SutraLibrary: PG register_chunk failed: {}", e);
                    }
                }
            });
        }
    }

    /// 添加手动边（由领域解析器 extract_edges 调用）
    pub fn add_manual_edge(&self, from: &EntityUuid, to: &EntityUuid, weight: f32) {
        let mut graph = self.graph.write().unwrap();
        let mut indices = self.node_indices.write().unwrap();
        let from_idx = Self::get_or_create_node(&mut graph, &mut indices, from);
        let to_idx = Self::get_or_create_node(&mut graph, &mut indices, to);
        graph.add_edge(
            from_idx,
            to_idx,
            EdgeWeight {
                kind: EdgeKind::Manual,
                weight,
            },
        );
        drop((graph, indices));

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            let f = from.clone();
            let t = to.clone();
            tokio::task::spawn(async move {
                if let Err(e) = pg.write_edge(&f, &t, "Manual", weight).await {
                    tracing::warn!("SutraLibrary: PG add_manual_edge failed: {}", e);
                }
            });
        }
    }

    /// 添加学习边（由后台学习任务调用）
    pub fn add_learned_edge(&self, from: &EntityUuid, to: &EntityUuid, weight: f32) {
        let mut graph = self.graph.write().unwrap();
        let mut indices = self.node_indices.write().unwrap();
        let from_idx = Self::get_or_create_node(&mut graph, &mut indices, from);
        let to_idx = Self::get_or_create_node(&mut graph, &mut indices, to);

        // 检查是否已有 Manual 边，如果有则跳过
        let has_manual = graph.edges(from_idx).any(|e| {
            let other = if e.target() == from_idx {
                e.source()
            } else {
                e.target()
            };
            other == to_idx && e.weight().kind == EdgeKind::Manual
        });
        if has_manual {
            return;
        }

        let has_manual_rev = graph.edges(to_idx).any(|e| {
            let other = if e.target() == to_idx {
                e.source()
            } else {
                e.target()
            };
            other == from_idx && e.weight().kind == EdgeKind::Manual
        });
        if has_manual_rev {
            return;
        }

        graph.add_edge(
            from_idx,
            to_idx,
            EdgeWeight {
                kind: EdgeKind::Learned,
                weight,
            },
        );
        drop((graph, indices));

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            let f = from.clone();
            let t = to.clone();
            tokio::task::spawn(async move {
                if let Err(e) = pg.write_edge(&f, &t, "Learned", weight).await {
                    tracing::warn!("SutraLibrary: PG add_learned_edge failed: {}", e);
                }
            });
        }
    }

    /// 获取切片关联的实体
    pub fn get_entities_of_chunks(&self, chunk_ids: &[ChunkUuid]) -> HashSet<EntityUuid> {
        let c2e = self.chunk_to_entities.read().unwrap();
        let mut set = HashSet::new();
        for id in chunk_ids {
            if let Some(entities) = c2e.get(id) {
                for e in entities {
                    set.insert(e.clone());
                }
            }
        }
        set
    }

    /// 获取实体关联的切片
    pub fn get_chunks_of_entities(&self, entity_ids: &[EntityUuid]) -> Vec<ChunkUuid> {
        let e2c = self.entity_to_chunks.read().unwrap();
        let mut chunks = Vec::new();
        let mut seen = HashSet::new();
        for eid in entity_ids {
            if let Some(ids) = e2c.get(eid) {
                for id in ids {
                    if seen.insert(id.clone()) {
                        chunks.push(id.clone());
                    }
                }
            }
        }
        chunks
    }

    /// 获取邻居实体（通过 petgraph）
    pub fn get_neighbors(
        &self,
        entity_id: &EntityUuid,
        enable_manual: bool,
        enable_learned: bool,
    ) -> Vec<(EntityUuid, EdgeKind, f32)> {
        let graph = self.graph.read().unwrap();
        let indices = self.node_indices.read().unwrap();
        let node_idx = match Self::get_node_index(&indices, entity_id) {
            Some(idx) => idx,
            None => return Vec::new(),
        };

        graph
            .edges(node_idx)
            .filter(|e| {
                let kind = e.weight().kind;
                match kind {
                    EdgeKind::Manual => enable_manual,
                    EdgeKind::Learned => enable_learned,
                }
            })
            .map(|e| {
                let target = if e.target() == node_idx {
                    e.source()
                } else {
                    e.target()
                };
                let target_entity = graph[target].clone();
                (target_entity, e.weight().kind, e.weight().weight)
            })
            .collect()
    }

    /// 观察实体共现（用于学习任务）
    pub fn observe_entity_cooccurrence(&self, entities: &HashSet<EntityUuid>) {
        let entity_list: Vec<&EntityUuid> = entities.iter().collect();
        let mut edge_updates: Vec<(EntityUuid, EntityUuid, f32)> = Vec::new();

        for i in 0..entity_list.len() {
            for j in (i + 1)..entity_list.len() {
                let from = entity_list[i].clone();
                let to = entity_list[j].clone();

                let actual_weight = {
                    let mut graph = self.graph.write().unwrap();
                    let mut indices = self.node_indices.write().unwrap();
                    let from_idx = Self::get_or_create_node(&mut graph, &mut indices, &from);
                    let to_idx = Self::get_or_create_node(&mut graph, &mut indices, &to);

                    // 查找已有的 Learned 边
                    let mut found = false;
                    let mut new_weight = 0.3;
                    let mut edge_to_update: Option<EdgeIndex> = None;

                    for e in graph.edges(from_idx) {
                        let other = if e.target() == from_idx {
                            e.source()
                        } else {
                            e.target()
                        };
                        if other == to_idx && e.weight().kind == EdgeKind::Learned {
                            found = true;
                            edge_to_update = Some(e.id());
                            new_weight = (e.weight().weight + 0.1).min(1.0);
                            break;
                        }
                    }

                    if let Some(eid) = edge_to_update {
                        if let Some(ew) = graph.edge_weight_mut(eid) {
                            ew.weight = new_weight;
                        }
                    } else if !found {
                        // 检查是否有 Manual 边（有则跳过）
                        let has_manual = graph.edges(from_idx).any(|e| {
                            let other = if e.target() == from_idx {
                                e.source()
                            } else {
                                e.target()
                            };
                            other == to_idx && e.weight().kind == EdgeKind::Manual
                        });
                        if !has_manual {
                            graph.add_edge(
                                from_idx,
                                to_idx,
                                EdgeWeight {
                                    kind: EdgeKind::Learned,
                                    weight: 0.3,
                                },
                            );
                        }
                    }

                    new_weight
                };
                drop(self.graph.write().unwrap());
                drop(self.node_indices.write().unwrap());

                edge_updates.push((from, to, actual_weight));
            }
        }

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            tokio::task::spawn(async move {
                for (from, to, weight) in &edge_updates {
                    if let Err(e) = pg.write_edge(from, to, "Learned", *weight).await {
                        tracing::warn!(
                            "SutraLibrary: PG observe_entity_cooccurrence failed: {}",
                            e
                        );
                    }
                }
            });
        }
    }

    /// 获取边的两个端点实体（用于 PG 删除记录）
    fn get_edge_endpoints(
        &self,
        graph: &UnGraph<EntityUuid, EdgeWeight>,
        eid: EdgeIndex,
    ) -> Option<(EntityUuid, EntityUuid)> {
        graph
            .edge_endpoints(eid)
            .map(|(a, b)| (graph[a].clone(), graph[b].clone()))
    }

    /// 衰减长期未使用的 Learned 边权重
    pub fn decay_learned_edges(&self, decay_factor: f32) {
        let mut removed_edges: Vec<(EntityUuid, EntityUuid)> = Vec::new();
        {
            let mut graph = self.graph.write().unwrap();
            let indices = self.node_indices.read().unwrap();
            let mut edges_to_remove: Vec<EdgeIndex> = Vec::new();

            for (_entity, &idx) in indices.iter() {
                // 先收集所有 Learned 边的 ID
                let learned_edge_ids: Vec<EdgeIndex> = graph
                    .edges(idx)
                    .filter(|e| e.weight().kind == EdgeKind::Learned)
                    .map(|e| e.id())
                    .collect();

                for eid in &learned_edge_ids {
                    if let Some(ew) = graph.edge_weight_mut(*eid) {
                        ew.weight *= decay_factor;
                        if ew.weight <= 0.05 {
                            edges_to_remove.push(*eid);
                            // 记录移除的边（用于 PG 删除）
                            // 获取边的两个端点
                            if let Some((from, to)) = self.get_edge_endpoints(&graph, *eid) {
                                removed_edges.push((from, to));
                            }
                        }
                    }
                }
            }

            // 删除已衰减到阈值的边
            for eid in edges_to_remove {
                graph.remove_edge(eid);
            }
        }

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            tokio::task::spawn(async move {
                for (from, to) in &removed_edges {
                    if let Err(e) = pg.delete_edge(from, to, "Learned").await {
                        tracing::warn!("SutraLibrary: PG decay_learned_edges delete failed: {}", e);
                    }
                }
            });
        }
    }

    // ─── 反馈反哺 ───────────────────────────────────────────────

    /// 应用用户反馈到实体（like/dislike 反哺图谱边权重）
    pub fn apply_feedback_to_entities(&self, chunk_id: &ChunkUuid, positive: bool) {
        let entities: Vec<EntityUuid> = {
            let c2e = self.chunk_to_entities.read().unwrap();
            c2e.get(chunk_id).cloned().unwrap_or_default()
        };

        if entities.is_empty() {
            return;
        }

        let delta = if positive { 0.15 } else { -0.15 };
        let mut fb_updates: Vec<(EntityUuid, f32)> = Vec::new();

        // 更新实体级反馈分
        {
            let mut ef = self.entity_feedback.write().unwrap();
            for entity in &entities {
                let score = ef.entry(entity.clone()).or_insert(0.0);
                *score = (*score + delta).clamp(-1.0, 1.0);
                fb_updates.push((entity.clone(), *score));
            }
        }

        // 反哺邻接边权重（通过 petgraph）
        {
            let mut graph = self.graph.write().unwrap();
            let indices = self.node_indices.read().unwrap();
            for entity in &entities {
                if let Some(&idx) = indices.get(entity) {
                    let edge_ids: Vec<EdgeIndex> = graph.edges(idx).map(|e| e.id()).collect();
                    for eid in edge_ids {
                        if let Some(ew) = graph.edge_weight_mut(eid) {
                            match ew.kind {
                                EdgeKind::Learned => {
                                    ew.weight = (ew.weight + delta * 0.5).clamp(0.0, 1.0);
                                }
                                EdgeKind::Manual => {
                                    ew.weight = (ew.weight + delta * 0.1).clamp(0.0, 1.0);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 异步 PG 持久化
        if let Some(pg) = &*self.pg_graph.read().unwrap() {
            let pg = pg.clone();
            tokio::task::spawn(async move {
                for (entity, score) in &fb_updates {
                    if let Err(e) = pg.write_entity_feedback(entity, *score).await {
                        tracing::warn!("SutraLibrary: PG apply_feedback_to_entities failed: {}", e);
                    }
                }
            });
        }
    }

    /// 获取实体反馈分（用于空间排序的实体权重调整）
    pub fn get_entity_feedback(&self, entity: &EntityUuid) -> f32 {
        let ef = self.entity_feedback.read().unwrap();
        ef.get(entity).copied().unwrap_or(0.0)
    }

    /// 获取实体的反馈加权权重（用于空间排序的实体重合度计算）
    pub fn get_entity_weighted_score(&self, entity: &EntityUuid) -> f32 {
        let base: f32 = 1.0;
        let feedback = self.get_entity_feedback(entity);
        base + feedback * 0.5
    }
}

impl Default for EntityGraph {
    fn default() -> Self {
        Self::new()
    }
}

/// 图谱扩展配置（用于 expand 调用）
#[derive(Debug, Clone)]
pub struct GraphExpandConfig {
    pub max_depth: usize,
    pub max_result: usize,
    pub decay_factor: f32,
    pub enable_manual_edge: bool,
    pub enable_learned_edge: bool,
    pub learned_scale: f32,
}

impl From<crate::sutra_library::recall::LibraryRetrieveConfig> for GraphExpandConfig {
    fn from(cfg: crate::sutra_library::recall::LibraryRetrieveConfig) -> Self {
        Self {
            max_depth: cfg.graph_max_depth,
            max_result: cfg.graph_max_global_result,
            decay_factor: cfg.graph_decay_factor,
            enable_manual_edge: cfg.enable_manual_edge,
            enable_learned_edge: cfg.enable_learned_edge,
            learned_scale: cfg.learned_edge_scale,
        }
    }
}

/// 图谱通路策略
pub struct GraphPassageStrategy {
    graph: Arc<EntityGraph>,
}

impl GraphPassageStrategy {
    pub fn new(graph: Arc<EntityGraph>) -> Self {
        Self { graph }
    }

    pub fn graph(&self) -> &Arc<EntityGraph> {
        &self.graph
    }

    /// 获取切片关联的实体集合（委托给 EntityGraph）
    pub fn get_entities_of_chunks(&self, chunk_ids: &[ChunkUuid]) -> HashSet<EntityUuid> {
        self.graph.get_entities_of_chunks(chunk_ids)
    }

    /// 执行图谱 BFS 扩展
    ///
    /// - `seed_chunks`: 种子切片
    /// - `cfg`: 扩展配置
    /// - `score_scale`: 全局倍率（专门用于压低二阶图谱分数）
    pub fn expand(
        &self,
        seed_chunks: &[ChunkUuid],
        cfg: &GraphExpandConfig,
        score_scale: f32,
    ) -> Vec<GraphHit> {
        // 1. 种子切片提取实体集合，初始化 BFS 队列
        let seed_entities = self.graph.get_entities_of_chunks(seed_chunks);
        if seed_entities.is_empty() {
            return Vec::new();
        }

        let mut visited_entities: HashSet<EntityUuid> = HashSet::new();
        let mut queue: VecDeque<(EntityUuid, usize, Vec<EntityUuid>, f32)> = VecDeque::new();
        for entity in &seed_entities {
            visited_entities.insert(entity.clone());
            queue.push_back((entity.clone(), 0, vec![entity.clone()], 1.0));
        }

        let mut visited_chunks: HashSet<ChunkUuid> = seed_chunks.iter().cloned().collect();
        let mut result_chunks: HashMap<ChunkUuid, GraphHit> = HashMap::new();

        // 2. BFS 遍历实体图（通过 petgraph 的 get_neighbors）
        while let Some((current, depth, path, current_weight)) = queue.pop_front() {
            if depth >= cfg.max_depth {
                continue;
            }

            let neighbors =
                self.graph
                    .get_neighbors(&current, cfg.enable_manual_edge, cfg.enable_learned_edge);

            for (neighbor, kind, weight) in &neighbors {
                let edge_base = match kind {
                    EdgeKind::Manual => *weight,
                    EdgeKind::Learned => *weight * cfg.learned_scale,
                };

                let hop_decay = cfg.decay_factor.powi(depth as i32);
                let bonus_score = edge_base * hop_decay * score_scale;

                if bonus_score <= 0.001 {
                    continue;
                }

                let neighbor_chunks = self.graph.get_chunks_of_entities(&[neighbor.clone()]);
                for chunk_id in &neighbor_chunks {
                    if visited_chunks.insert(chunk_id.clone()) {
                        let mut entity_path = path.clone();
                        entity_path.push(neighbor.clone());

                        let hit = GraphHit {
                            chunk_id: chunk_id.clone(),
                            bonus_score,
                            edge_sources: vec![RelationSource {
                                from_entity: current.clone(),
                                to_entity: neighbor.clone(),
                                kind: *kind,
                                weight: *weight,
                            }],
                            entity_path: Some(entity_path),
                        };
                        result_chunks.insert(chunk_id.clone(), hit);
                    } else if let Some(existing) = result_chunks.get_mut(chunk_id) {
                        if bonus_score > existing.bonus_score {
                            existing.bonus_score = bonus_score;
                            existing.edge_sources.push(RelationSource {
                                from_entity: current.clone(),
                                to_entity: neighbor.clone(),
                                kind: *kind,
                                weight: *weight,
                            });
                        }
                    }
                }

                if visited_entities.insert(neighbor.clone()) {
                    let mut new_path = path.clone();
                    new_path.push(neighbor.clone());
                    queue.push_back((
                        neighbor.clone(),
                        depth + 1,
                        new_path,
                        current_weight * hop_decay,
                    ));
                }
            }
        }

        // 3. 全局截断
        let mut hits: Vec<GraphHit> = result_chunks.into_values().collect();
        hits.sort_by(|a, b| {
            b.bonus_score
                .partial_cmp(&a.bonus_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(cfg.max_result);

        hits
    }
}
