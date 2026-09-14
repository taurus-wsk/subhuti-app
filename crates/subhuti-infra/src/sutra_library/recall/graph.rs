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
use anyhow::Result;
use async_trait::async_trait;
use petgraph::graph::{EdgeIndex, NodeIndex, UnGraph};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

/// 实体图谱持久化端口
///
/// `EntityGraph` 运行时双写（注册切片、建边、反馈）只依赖这 6 个操作，
/// 不应绑定整个藏经阁存储面（KB CRUD 等），因此从 `PersistencePort` 中
/// 按接口隔离原则拆出这条窄接口。
///
/// 已实现：`PgGraphStorage`（PG 后端）、`SqliteStorage`（SQLite 降级后端）。
/// 未注入任何实现时，图谱退化为纯内存（进程退出即丢）。
///
/// **验证状态**：SQLite 路径已端到端验证（运行时双写 + 回灌 + 边数收敛）；
/// PG 路径仅有实现与签名对齐、**未做真机验证**（本机无 PG 环境）。
/// 两端共用 `persistence.rs::normalize_edge_endpoints`，数据正确性约束一致。
#[async_trait]
pub trait GraphPersistence: Send + Sync {
    /// 全量加载图谱（邻接表 / 实体→切片 / 切片→实体 / 实体反馈分）
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

/// `PgGraphStorage` 直接转发到自身固有方法（无歧义：它未实现 `PersistencePort`）
#[async_trait]
impl GraphPersistence for PgGraphStorage {
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
        PgGraphStorage::load_all(
            self,
            adjacency,
            entity_to_chunks,
            chunk_to_entities,
            entity_feedback,
        )
        .await
    }
    async fn write_edge(&self, from: &str, to: &str, kind: &str, weight: f32) -> Result<()> {
        PgGraphStorage::write_edge(self, from, to, kind, weight).await
    }
    async fn delete_edge(&self, from: &str, to: &str, kind: &str) -> Result<()> {
        PgGraphStorage::delete_edge(self, from, to, kind).await
    }
    async fn write_entity_chunk(&self, entity_id: &str, chunk_id: &str) -> Result<()> {
        PgGraphStorage::write_entity_chunk(self, entity_id, chunk_id).await
    }
    async fn write_entity_feedback(&self, entity_id: &str, score: f32) -> Result<()> {
        PgGraphStorage::write_entity_feedback(self, entity_id, score).await
    }
    async fn batch_delete_edges(&self, edges: &[(String, String, String)]) -> Result<()> {
        PgGraphStorage::batch_delete_edges(self, edges).await
    }
}

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
    /// 图谱持久化适配器（可选，有则异步双写）
    ///
    /// 依赖的是窄接口 `GraphPersistence` 而非具体 PG 类型，
    /// 因此 PG / SQLite 两种后端都能接入，无后端时退化为纯内存。
    graph_store: RwLock<Option<Arc<dyn GraphPersistence>>>,
}

impl EntityGraph {
    pub fn new() -> Self {
        Self {
            graph: RwLock::new(UnGraph::new_undirected()),
            node_indices: RwLock::new(HashMap::new()),
            entity_to_chunks: RwLock::new(HashMap::new()),
            chunk_to_entities: RwLock::new(HashMap::new()),
            entity_feedback: RwLock::new(HashMap::new()),
            graph_store: RwLock::new(None),
        }
    }

    // ─── 内部辅助方法 ───────────────────────────────────────────

    /// 导出全部边（供非 PG 后端持久化，如 SQLite 降级模式）
    ///
    /// PG 模式下 `add_manual_edge` 会自己异步双写，但 SQLite 降级路径没有这条链路，
    /// 图谱边就只在内存里、进程退出即丢。导出后由引擎侧统一落库。
    pub fn export_edges(&self) -> Vec<(EntityUuid, EntityUuid, EdgeKind, f32)> {
        let graph = self.graph.read().unwrap();
        let indices = self.node_indices.read().unwrap();
        let mut out = Vec::new();
        for (entity, &idx) in indices.iter() {
            for e in graph.edges(idx) {
                let target_idx = if e.target() == idx {
                    e.source()
                } else {
                    e.target()
                };
                if let Some(target) = graph.node_weight(target_idx) {
                    let w = e.weight();
                    out.push((entity.clone(), target.clone(), w.kind, w.weight));
                }
            }
        }
        out
    }

    /// 批量导入边（启动回灌用；已存在同类型边会再叠一条，由调用方保证幂等）
    pub fn import_edges(&self, edges: &[(EntityUuid, EntityUuid, EdgeKind, f32)]) {
        for (from, to, kind, weight) in edges {
            if from == to {
                continue;
            }
            match kind {
                EdgeKind::Manual => self.add_manual_edge(from, to, *weight),
                EdgeKind::Learned => self.add_learned_edge(from, to, *weight),
            }
        }
    }

    /// 注册切片与实体的反向索引（启动回灌用）
    pub fn register_chunk_entities(&self, chunk_id: &ChunkUuid, entities: Vec<EntityUuid>) {
        {
            let mut c2e = self.chunk_to_entities.write().unwrap();
            c2e.insert(chunk_id.clone(), entities.clone());
        }
        {
            let mut e2c = self.entity_to_chunks.write().unwrap();
            for e in entities {
                e2c.entry(e).or_default().push(chunk_id.clone());
            }
        }
    }

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

    // 注：原 `hashmap_to_graph`（从邻接表重建图 + (min,max) 去重）已删除。
    // 边的真相改为「实体索引 + 当前建边规则」（见 `load_persisted_data`
    // 与 `hydrate_graph`），不再存在"读库里的边、需要防重复"这条路径。

    // ─── 图谱持久化 ────────────────────────────────────────────

    /// 设置图谱持久化适配器，并从持久化数据**重建**内存图谱
    ///
    /// 适配器可为 PG（`PgGraphStorage`）或 SQLite 降级（`SqliteStorage`），
    /// 由调用方按后端可用性选择注入。
    pub async fn load_persisted_data(&self, store: Arc<dyn GraphPersistence>) {
        *self.graph_store.write().unwrap() = Some(store.clone());

        // 语义是「从持久化重建」，不是「与当前内存合并」。
        // `load_all` 会把传入的基线 map 与库中数据**叠加**，若基线用当前内存快照，
        // 边与实体-切片索引就会成倍虚高（实测 entity_chunk 98 → 196、边 336 → 672）。
        // 因此基线一律传空，内存图谱随后被整体替换。
        let empty_adj: HashMap<EntityUuid, Vec<(EntityUuid, EdgeKind, f32)>> = HashMap::new();
        let empty_e2c: HashMap<EntityUuid, Vec<ChunkUuid>> = HashMap::new();
        let empty_c2e: HashMap<ChunkUuid, Vec<EntityUuid>> = HashMap::new();
        let empty_ef: HashMap<EntityUuid, f32> = HashMap::new();

        match store
            .load_all(&empty_adj, &empty_e2c, &empty_c2e, &empty_ef)
            .await
        {
            Ok((_adj, e2c, c2e, ef)) => {
                // 边的**唯一真相**是「切片 → 实体」索引 + 当前建边规则，而不是
                // 库里那份可能由旧规则生成的邻接表。所以这里丢弃 `adj`，按当下
                // 的规则从 `c2e` 重建：建边规则一旦收紧，图谱会自动自愈，
                // 不需要再写迁移脚本去清历史边。
                let chunk_ids: HashSet<ChunkUuid> = c2e.keys().cloned().collect();
                let semantic_sets: Vec<Vec<EntityUuid>> = c2e
                    .values()
                    .map(|ents| {
                        ents.iter()
                            .filter(|e| !chunk_ids.contains(*e))
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                    .collect();

                *self.entity_to_chunks.write().unwrap() = e2c;
                *self.chunk_to_entities.write().unwrap() = c2e;
                *self.entity_feedback.write().unwrap() = ef;
                *self.graph.write().unwrap() = UnGraph::new_undirected();
                self.node_indices.write().unwrap().clear();

                for ents in &semantic_sets {
                    self.link_cooccurrence(ents);
                }

                tracing::info!(
                    "SutraLibrary: EntityGraph 持久化数据加载完成（边按当前规则从实体索引重建）"
                );
            }
            Err(e) => {
                tracing::warn!(
                    "SutraLibrary: EntityGraph 持久化加载失败，退回纯内存模式: {}",
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
    /// 实体-切片索引规模（供可观测性统计）
    ///
    /// 该索引只经由 PG 落盘，SQLite 降级模式下 DB 表恒空，
    /// 但启动回灌会原地重建，所以要看内存里的真实规模。
    pub fn entity_chunk_pairs(&self) -> usize {
        self.chunk_to_entities
            .read()
            .unwrap()
            .values()
            .map(|v| v.len())
            .sum()
    }

    /// 当前图谱边数（供可观测性统计）
    pub fn edge_count(&self) -> usize {
        self.graph.read().unwrap().edge_count()
    }

    pub fn register_chunk(&self, node: &MemoryNode) {
        // 实体抽取走的是领域正则：
        // - 长文本会触发灾难性回溯（实测冷启动卡死十余秒仍不返回）
        // - 回灌时对每个节点都跑一遍，拖慢启动（实测 6 秒还没回灌完）
        // 实体只需要从开头部分抽取即可，这里截到 400 字——
        // 长篇知识文档的实体索引价值本来也低（它靠全文检索，不靠实体扩召回）。
        const MAX_INDEX_CHARS: usize = 400;
        let node = if node.content.chars().count() > MAX_INDEX_CHARS {
            let mut trimmed = node.clone();
            trimmed.content = node.content.chars().take(MAX_INDEX_CHARS).collect();
            trimmed
        } else {
            node.clone()
        };
        let entities = extract_entities_from_node(&node);
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

        // 同一条记忆里共现的实体两两建边。
        //
        // 为什么需要：领域解析器的 `extract_entity_relations` 只对显式关系
        // （"A 依赖 B" 之类）生效，而沉淀下来的短事实几乎没有这种句式，
        // 结果图谱边恒为 0，图谱通路等于没启用。
        // 共现是弱信号但稳定可用，配合冷热衰减能自然淘汰噪声边。
        //
        // 但只拿**语义实体**建边：节点自身 UUID 天然唯一，给它建边只会指向
        // 自己的碎片，纯属浪费边数（实测每个节点都挂着 7 条这种边）。
        let semantic: Vec<EntityUuid> = entities
            .iter()
            .filter(|e| **e != chunk_id)
            .cloned()
            .collect();
        self.link_cooccurrence(&semantic);

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            let ent = entities.clone();
            let cid = chunk_id.clone();
            tokio::task::spawn(async move {
                for entity in &ent {
                    if let Err(e) = store.write_entity_chunk(entity, &cid).await {
                        tracing::warn!("SutraLibrary: 图谱 register_chunk 落盘失败: {}", e);
                    }
                }
            });
        }
    }

    /// 从一条记忆的候选实体里挑出**有区分度**、可用于共现建边的实体
    ///
    /// 实体抽取走的是 2-4 字字符滑窗，会产出大量无区分度的碎片
    /// （「模块职责」被切成 块职 / 块职责 / 模块职 / 模块职责）。这些碎片一旦
    /// 跨节点共现，就成了把语义无关记忆连起来的枢纽——09-14 用真实 MCP 实测
    /// 复现：一条与查询毫无关系的记忆，仅凭共享 2 字碎片「探针」就被 BFS 拉了
    /// 进来，并在排序里顶掉了精确命中。
    ///
    /// 两条规则：
    /// 1. **最短 3 字**：2 字碎片（「探针」「使用」「固定」「模块」）出现频率最高、
    ///    区分度最低，是噪声边的主要来源；
    /// 2. **剔除被同批更长实体包含的碎片**：同一条记忆里既然已有「模块职责」，
    ///    就不再为「块职」「块职责」建边，只保留最长形式。
    pub fn select_cooccur_entities(entities: &[EntityUuid]) -> Vec<EntityUuid> {
        const MIN_COOC_CHARS: usize = 3;
        let mut out: Vec<EntityUuid> = entities
            .iter()
            .filter(|e| e.chars().count() >= MIN_COOC_CHARS)
            .filter(|e| {
                !entities.iter().any(|other| {
                    other.chars().count() > e.chars().count() && other.contains(e.as_str())
                })
            })
            .cloned()
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// 为一组共现实体建立（或强化）关联边
    ///
    /// 已存在的边不重复添加，只把权重提升到不低于本次值——
    /// 否则同一实体对每次共现都会堆一条平行边，图谱被噪声淹没。
    pub fn link_cooccurrence(&self, entities: &[EntityUuid]) {
        // 实体过多时两两建边是 O(n²)，截断保护
        const MAX_COOC: usize = 8;
        let filtered = Self::select_cooccur_entities(entities);
        let list: Vec<&EntityUuid> = filtered.iter().take(MAX_COOC).collect();
        if list.len() < 2 {
            return;
        }
        for i in 0..list.len() {
            for j in (i + 1)..list.len() {
                if list[i] == list[j] {
                    continue;
                }
                self.upsert_edge(list[i], list[j], 0.3);
            }
        }
    }

    /// 新增边；若边已存在则仅提升权重（避免平行边堆积）
    pub fn upsert_edge(&self, from: &EntityUuid, to: &EntityUuid, weight: f32) {
        {
            let mut graph = self.graph.write().unwrap();
            let mut indices = self.node_indices.write().unwrap();
            let f = Self::get_or_create_node(&mut graph, &mut indices, from);
            let t = Self::get_or_create_node(&mut graph, &mut indices, to);
            match graph.find_edge(f, t) {
                Some(e) => {
                    if let Some(w) = graph.edge_weight_mut(e) {
                        if w.weight < weight {
                            w.weight = weight;
                        }
                    }
                }
                None => {
                    graph.add_edge(
                        f,
                        t,
                        EdgeWeight {
                            kind: EdgeKind::Manual,
                            weight,
                        },
                    );
                }
            }
        }

        // 异步双写（与 add_manual_edge 同口径）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            let f = from.clone();
            let t = to.clone();
            tokio::task::spawn(async move {
                if let Err(e) = store.write_edge(&f, &t, "Manual", weight).await {
                    tracing::warn!("SutraLibrary: 图谱 write_edge 落盘失败: {}", e);
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

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            let f = from.clone();
            let t = to.clone();
            tokio::task::spawn(async move {
                if let Err(e) = store.write_edge(&f, &t, "Manual", weight).await {
                    tracing::warn!("SutraLibrary: 图谱 add_manual_edge 落盘失败: {}", e);
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

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            let f = from.clone();
            let t = to.clone();
            tokio::task::spawn(async move {
                if let Err(e) = store.write_edge(&f, &t, "Learned", weight).await {
                    tracing::warn!("SutraLibrary: 图谱 add_learned_edge 落盘失败: {}", e);
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

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            tokio::task::spawn(async move {
                for (from, to, weight) in &edge_updates {
                    if let Err(e) = store.write_edge(from, to, "Learned", *weight).await {
                        tracing::warn!(
                            "SutraLibrary: 图谱 observe_entity_cooccurrence 落盘失败: {}",
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

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            tokio::task::spawn(async move {
                for (from, to) in &removed_edges {
                    if let Err(e) = store.delete_edge(from, to, "Learned").await {
                        tracing::warn!(
                            "SutraLibrary: 图谱 decay_learned_edges 删除落盘失败: {}",
                            e
                        );
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

        // 异步双写（PG / SQLite 由注入的适配器决定）
        if let Some(store) = &*self.graph_store.read().unwrap() {
            let store = store.clone();
            tokio::task::spawn(async move {
                for (entity, score) in &fb_updates {
                    if let Err(e) = store.write_entity_feedback(entity, *score).await {
                        tracing::warn!(
                            "SutraLibrary: 图谱 apply_feedback_to_entities 落盘失败: {}",
                            e
                        );
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

                let neighbor_chunks = self
                    .graph
                    .get_chunks_of_entities(std::slice::from_ref(neighbor));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<EntityUuid> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// 2 字碎片不参与建边：它们是噪声边的主要来源
    /// （实测「探针」把两条语义无关的记忆连成了邻居，并顶掉精确命中）。
    #[test]
    fn two_char_fragments_are_excluded() {
        let picked = EntityGraph::select_cooccur_entities(&v(&["探针", "使用", "固定", "模块"]));
        assert!(
            picked.is_empty(),
            "纯 2 字碎片不应产生任何可建边实体: {picked:?}"
        );
    }

    /// 同批里被更长实体包含的碎片被剔除，只保留最长形式。
    #[test]
    fn substring_fragments_collapse_to_longest() {
        let picked =
            EntityGraph::select_cooccur_entities(&v(&["块职", "块职责", "模块职", "模块职责"]));
        assert_eq!(picked, v(&["模块职责"]));
    }

    /// 有区分度的实体保留（≥3 字且互不包含）。
    #[test]
    fn meaningful_entities_survive() {
        let picked = EntityGraph::select_cooccur_entities(&v(&[
            "渲染器",
            "Cycles",
            "ACES",
            "tokio",
            "色彩管理",
        ]));
        assert_eq!(picked.len(), 5);
        for e in ["渲染器", "Cycles", "ACES", "tokio", "色彩管理"] {
            assert!(picked.contains(&e.to_string()), "应保留有区分度的实体: {e}");
        }
    }

    /// 共现建边端到端：有区分度的实体共现 → 建边；纯碎片共现 → 不建边。
    #[test]
    fn cooccurrence_links_only_meaningful_entities() {
        let g = EntityGraph::new();
        g.link_cooccurrence(&v(&["渲染器", "Cycles"]));
        assert_eq!(g.edge_count(), 1, "两个有区分度的实体共现应产生 1 条边");

        let g2 = EntityGraph::new();
        g2.link_cooccurrence(&v(&["探针", "使用"]));
        assert_eq!(g2.edge_count(), 0, "纯 2 字碎片共现不应建边");
    }
}
