//! # 藏经阁引擎核心
//!
//! 写入流水线 + 查询流水线。入站端口 MemoryEnginePort 供外部调用。

use crate::sutra_library::domain::DomainRouter;
use crate::sutra_library::feedback::{
    ExecutionLog, FeedbackAnalyzer, FeedbackConfig, RecallChunkInfo,
};
use crate::sutra_library::hotness::HotnessCalculator;
use crate::sutra_library::models::*;
use crate::sutra_library::persistence::PersistencePort;
use crate::sutra_library::recall::pipeline::default_query_analyzer;
use crate::sutra_library::recall::{
    self, collect_entities, BaseSearch, ChunkUuid, EntityGraph, EntityUuid, GraphPassageStrategy,
    GraphPersistence, LibraryRetrieveConfig, RetrieveSource, SpaceDepthStrategy, TantivyIndex,
};
use crate::sutra_library::retrieval::RetrievalScheduler;
use crate::sutra_library::storage::{MemoryStorage, PgStorage};
use crate::sutra_library::tree::{SlotTree, TreeValidator};
use anyhow::{bail, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use subhuti_core::sutra_library::SutraLibraryPort;
use uuid::Uuid;

/// 藏经阁引擎（入站端口）
pub struct MemoryEnginePort {
    memory: Arc<MemoryStorage>,
    /// 核心持久化后端（PG 或 SQLite 降级），可为空（纯内存）
    persistence: Option<Arc<dyn PersistencePort>>,
    domain_router: Arc<DomainRouter>,
    retrieval: RetrievalScheduler,
    /// 新版召回引擎子模块（参考《藏经阁召回引擎·高层整合终极方案》）
    base_search: BaseSearch,
    space_strategy: SpaceDepthStrategy,
    graph_strategy: GraphPassageStrategy,
    /// 实体图谱（与 graph_strategy 共享同一实例）
    entity_graph: Arc<EntityGraph>,
    /// Tantivy 全文检索引擎（支持中文分词、原生 BM25 打分、布尔过滤）
    tantivy: Option<Arc<TantivyIndex>>,
    /// SlotTree 领域知识树（基于 slotmap，O(1) 节点查找）
    tree: RwLock<SlotTree>,
    /// 反馈分析器（记忆命中率反馈 + 自动生成 Learned 边）
    feedback_analyzer: Arc<FeedbackAnalyzer>,
    /// 最近 N 次检索结果缓存（query_hash -> Vec<chunk_id>），用于 record_execution 填充召回信息
    last_retrieve_cache: RwLock<HashMap<String, Vec<String>>>,
    /// 最近一次召回（时间戳 ms, chunk_ids）。
    ///
    /// 兜底通路：编排层记录反馈时用的 query 与专家实际检索的 query 常常不完全一致
    /// （planner 会改写、专家可能加前缀），只靠 query_hash 精确匹配会永远命中不了，
    /// 实测"召回 0 条"导致命中率恒为 0。这里保留最近一次结果作为兜底。
    last_retrieve_recent: RwLock<(i64, Vec<String>)>,
}

impl MemoryEnginePort {
    pub fn new(
        memory: Arc<MemoryStorage>,
        persistence: Option<Arc<dyn PersistencePort>>,
        feedback_pg: Option<Arc<PgStorage>>,
        domain_router: Arc<DomainRouter>,
        // Tantivy 索引目录：`Some` 走磁盘（重启后可恢复），`None` 走内存
        tantivy_dir: Option<std::path::PathBuf>,
    ) -> Self {
        let retrieval =
            RetrievalScheduler::new(memory.clone(), persistence.clone(), domain_router.clone());
        let entity_graph = Arc::new(EntityGraph::new());
        let graph_strategy = GraphPassageStrategy::new(entity_graph.clone());

        // 创建 Tantivy 索引。
        //
        // 默认曾固定为内存索引（`new_in_ram`），导致进程一退出索引全丢、
        // 每次启动都要重新索引，且**已沉淀到 SQLite 的节点在新进程里检索不到**
        // （检索走 BaseSearch → Tantivy + 内存态）。改为落盘后，
        // 索引与 SQLite 双写，重启即恢复。
        let tantivy = Arc::new(match &tantivy_dir {
            Some(dir) => {
                let idx = TantivyIndex::open_or_create_in_dir(dir);
                tracing::info!("SutraLibrary: Tantivy 索引落盘于 {:?}", dir);
                idx
            }
            None => TantivyIndex::new_in_ram(),
        });
        let base_search = BaseSearch::new(memory.clone()).with_tantivy(tantivy.clone());

        Self {
            base_search,
            space_strategy: SpaceDepthStrategy::new(memory.clone())
                .with_entity_graph(entity_graph.clone()),
            graph_strategy,
            entity_graph: entity_graph.clone(),
            tantivy: Some(tantivy),
            tree: RwLock::new(SlotTree::new()),
            feedback_analyzer: Arc::new(FeedbackAnalyzer::new(
                entity_graph.clone(),
                feedback_pg,
                FeedbackConfig::default(),
            )),
            last_retrieve_cache: RwLock::new(HashMap::new()),
            last_retrieve_recent: RwLock::new((0, Vec::new())),
            memory,
            persistence,
            domain_router,
            retrieval,
        }
    }

    /// 设置实体图谱（替换默认空图谱，用于新版召回引擎）
    pub fn set_entity_graph(&mut self, graph: Arc<EntityGraph>) {
        self.entity_graph = graph;
        self.graph_strategy = GraphPassageStrategy::new(self.entity_graph.clone());
        self.space_strategy = SpaceDepthStrategy::new(self.memory.clone())
            .with_entity_graph(self.entity_graph.clone());
    }

    /// 为实体图谱启用持久化（异步加载已有数据到内存，不阻塞）
    ///
    /// 适配器由调用方按后端可用性注入：PG 模式给 `PgGraphStorage`，
    /// SQLite 降级模式给 `SqliteStorage`。两者都实现 `GraphPersistence`。
    /// 在启动时调用，通过 spawn 后台加载持久化数据到图谱内存中。
    pub fn init_graph_persistence(&self, store: Arc<dyn GraphPersistence>) {
        let entity_graph = self.entity_graph.clone();
        tokio::task::spawn(async move {
            entity_graph.load_persisted_data(store).await;
        });
    }

    /// 获取实体图谱引用
    pub fn entity_graph(&self) -> &Arc<EntityGraph> {
        &self.entity_graph
    }

    /// 获取持久化后端（PG 或 SQLite 降级），供知识库 CRUD 等需要直连存储的场景使用
    pub fn persistence(&self) -> Option<Arc<dyn PersistencePort>> {
        self.persistence.clone()
    }

    /// 获取 Tantivy 索引引用
    pub fn tantivy_index(&self) -> Option<&Arc<TantivyIndex>> {
        self.tantivy.as_ref()
    }

    /// 获取反馈分析器引用
    pub fn feedback_analyzer(&self) -> &Arc<FeedbackAnalyzer> {
        &self.feedback_analyzer
    }

    /// 记录一条执行日志到反馈分析器
    ///
    /// 由 Agent 执行完成后调用，用于反馈闭环。
    ///
    /// # 参数
    /// - `query`: 用户原始 Query
    /// - `recalled_chunks`: 召回的全部切片信息
    /// - `used_chunk_ids`: Agent 实际使用的切片 ID 列表
    /// - `task_success`: 任务执行是否成功
    /// - `graph`: 对话图谱 ID
    /// - `domain`: 领域
    /// - `session_id`: 会话 ID（可选）
    #[allow(clippy::too_many_arguments)]
    pub fn record_execution(
        &self,
        query: &str,
        recalled_chunks: Vec<RecallChunkInfo>,
        used_chunk_ids: Vec<String>,
        task_success: bool,
        graph: &str,
        domain: &str,
        session_id: Option<String>,
    ) {
        use sha2::Digest;
        let query_hash = format!("{:x}", sha2::Sha256::digest(query.as_bytes()));

        let log = ExecutionLog {
            query_hash,
            query: query.to_string(),
            recalled_chunks,
            used_chunk_ids,
            task_success,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            graph: graph.to_string(),
            domain: domain.to_string(),
            session_id,
        };

        self.feedback_analyzer.record_execution(log);
    }

    /// 启动反馈分析器后台任务
    ///
    /// 在引擎初始化完成后调用，启动后台定时分析。
    pub fn start_feedback_analysis(&self) {
        let analyzer = self.feedback_analyzer.clone();
        analyzer.start_background_analysis();
    }

    /// 将内存中所有已有节点同步到 Tantivy 索引
    ///
    /// 在启动时调用，用于初始化 Tantivy 索引与内存数据一致。
    pub fn sync_all_to_tantivy(&self) {
        if let Some(ref tantivy) = self.tantivy {
            let nodes = self.memory.get_all_nodes();
            for node in &nodes {
                tantivy.index_node(node);
            }
            tantivy.commit();
            tracing::info!("TantivyIndex: 已同步 {} 个节点", nodes.len());
        }
    }

    /// 获取图谱策略引用（供外部调用 expand 等）
    pub fn graph_strategy(&self) -> &GraphPassageStrategy {
        &self.graph_strategy
    }

    // ─── 集合管理 ───────────────────────────────────────────────

    pub fn create_collection(&self, name: &str, domain: &str, description: &str) -> Collection {
        // 幂等：同名同域已存在则直接复用，不再新建。
        //
        // 历史 bug：每次注册都 `Uuid::new_v4()` 生成新 ID，而落库是按 `collection_id`
        // 去重的 → 每次启动都新增一行，实测把 `blender_knowledge` 堆了 45 份。
        // 重复集合的危害不只是脏数据：它会让 `list_collections()` 返回 N 份同名集合，
        // 检索时同一份知识被反复召回。
        if let Some(existing) = self
            .memory
            .list_collections()
            .into_iter()
            .find(|c| c.name == name && c.domain == domain)
        {
            tracing::debug!(
                "SutraLibrary: 集合已存在，复用 (name={}, domain={}, id={})",
                name,
                domain,
                existing.collection_id
            );
            return existing;
        }

        let collection = Collection {
            collection_id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            domain: domain.to_string(),
            description: description.to_string(),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.memory.create_collection(collection.clone());

        // 异步 PG 落库
        if let Some(pg) = &self.persistence {
            let pg = pg.clone();
            let col = collection.clone();
            tokio::task::spawn(async move {
                if let Err(e) = pg.write_collection(&col).await {
                    tracing::warn!("SutraLibrary: PG write collection failed: {}", e);
                }
            });
        }

        collection
    }

    pub fn list_collections(&self) -> Vec<Collection> {
        self.memory.list_collections()
    }

    // ─── 节点 CRUD ──────────────────────────────────────────────

    /// 写入节点（幂等写入流水线）
    ///
    /// 输入校验 → 领域路由 → 幂等去重前置 → 领域语义切片 →
    /// 摘要元数据增强 → 标准化节点 → 树结构挂载+校验 →
    /// 图谱关联提取 → 内存写入 → 异步 PG 落库 + 增量 TS 索引 →
    /// 热度初始化
    pub fn write_node(
        &self,
        collection_id: &str,
        raw_content: &str,
        domain: &str,
        parent_id: Option<&str>,
    ) -> Result<MemoryNode> {
        let now = chrono::Utc::now().timestamp();

        // 1. 输入校验
        if raw_content.is_empty() {
            bail!("写入内容不能为空");
        }

        // 2. 领域路由
        let parser = self
            .domain_router
            .parser_for(domain)
            .ok_or_else(|| anyhow::anyhow!("未知领域: {}", domain))?;

        // 3. 幂等去重前置
        let content_hash = Self::hash_content(raw_content);
        let existing = self.memory.get_all_nodes();
        if existing
            .iter()
            .any(|n| n.content_hash == content_hash && n.collection_id == collection_id)
        {
            tracing::debug!("SutraLibrary: 幂等去重命中，跳过写入");
            return Ok(existing
                .into_iter()
                .find(|n| n.content_hash == content_hash)
                .unwrap());
        }

        // 4. 领域语义切片
        let ctx = ParseContext {
            collection_id: collection_id.to_string(),
            domain: domain.to_string(),
            source_path: None,
            extra: std::collections::HashMap::new(),
        };
        let chunks = parser.split_semantic_chunks(raw_content, &ctx);

        if chunks.is_empty() {
            bail!("领域切片后无有效节点");
        }

        // 5. 处理每个切片
        let mut root_node: Option<MemoryNode> = None;
        let mut all_nodes = Vec::new();

        for chunk in &chunks {
            let (summary, _metadata) = parser.enrich_chunk(chunk);

            let node_id = Uuid::new_v4().to_string();
            let parent = parent_id.or_else(|| root_node.as_ref().map(|r| r.node_id.as_str()));

            // 树结构校验
            let mut node = MemoryNode {
                node_id: node_id.clone(),
                collection_id: collection_id.to_string(),
                domain: domain.to_string(),
                node_type: chunk.node_type.clone(),
                content_hash: Self::hash_content(&chunk.content),
                parent_id: parent.map(|p| p.to_string()),
                path: String::new(),
                depth: 0,
                sort_order: chunk.sort_order,
                title: chunk.title.clone(),
                summary,
                content: chunk.content.clone(),
                metadata: chunk.metadata.clone(),
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
            };

            // 计算路径和深度（使用 SlotTree）
            {
                let tree = self.tree.read().unwrap();
                node.path =
                    TreeValidator::compute_path(&tree, node.parent_id.as_deref(), &node.title);
                node.depth = TreeValidator::compute_depth(&tree, node.parent_id.as_deref());
            }

            // 树结构合法性校验（使用 SlotTree）
            {
                let tree = self.tree.read().unwrap();
                TreeValidator::validate_mount(
                    &tree,
                    &node.node_id,
                    &node.collection_id,
                    node.parent_id.as_deref(),
                )?;
            }

            // 写入内存
            self.memory.write_node(&node);

            // 添加到 SlotTree（领域知识树）
            {
                let mut tree = self.tree.write().unwrap();
                tree.add_node(
                    &node.node_id,
                    node.parent_id.as_deref(),
                    &node.title,
                    &node.collection_id,
                );
            }

            // 索引到 Tantivy（全文检索）
            if let Some(ref tantivy) = self.tantivy {
                tantivy.index_node(&node);
            }

            if root_node.is_none() {
                root_node = Some(node.clone());
            }
            all_nodes.push(node);
        }

        // 6. 图谱关联提取
        if let Some(parser) = self.domain_router.parser_for(domain) {
            let edges = parser.extract_edges(&all_nodes);
            // 边写入内存
            for edge in &edges {
                if let Some(target) = self.memory.read_node(&edge.target_node_id) {
                    let mut target = target;
                    target.refs_in.push(edge.clone());
                    self.memory.write_node(&target);
                }
            }
        }

        // 6b. 注册到实体图谱（启用图谱召回通路）
        for node in &all_nodes {
            self.entity_graph.register_chunk(node);
        }

        // 6c. 领域实体关系对 → EntityGraph Manual 边
        // 如果当前领域有 extract_entity_relations 实现，将提取的关系对写入图谱 adjacency
        if let Some(parser) = self.domain_router.parser_for(domain) {
            for node in &all_nodes {
                let relations = parser.extract_entity_relations(&node.content);
                for (entity_a, entity_b, weight) in &relations {
                    self.entity_graph
                        .add_manual_edge(entity_a, entity_b, *weight);
                }
            }
        }

        // 7. 异步 PG 落库
        if let Some(pg) = &self.persistence {
            let pg = pg.clone();
            let nodes = all_nodes.clone();
            tokio::task::spawn(async move {
                for node in &nodes {
                    if let Err(e) = pg.write_node(node).await {
                        tracing::warn!("SutraLibrary: PG write failed: {}", e);
                    }
                }
            });
        }

        Ok(root_node.unwrap())
    }

    /// 读取节点
    pub fn read_node(&self, node_id: &str) -> Option<MemoryNode> {
        let mut node = self.memory.read_node(node_id)?;
        let now = chrono::Utc::now().timestamp();
        node.access_count += 1;
        node.last_accessed_at = now;
        node.base_activation = HotnessCalculator::compute_activation(&node, now);
        self.memory.write_node(&node);
        Some(node)
    }

    /// 按路径读取
    pub fn read_by_path(&self, path: &str) -> Option<MemoryNode> {
        let node = self.memory.find_by_path(path)?;
        self.read_node(&node.node_id)
    }

    /// 删除节点
    pub fn delete_node(&self, node_id: &str) {
        // 递归删除子树（使用 SlotTree）
        let subtree = {
            let tree = self.tree.read().unwrap();
            TreeValidator::collect_subtree_ids(&tree, node_id)
        };
        for id in &subtree {
            self.memory.delete_node(id);
            // 从 Tantivy 索引中删除
            if let Some(ref tantivy) = self.tantivy {
                tantivy.delete_node(id);
            }
        }

        // 从 SlotTree 中移除
        {
            let mut tree = self.tree.write().unwrap();
            tree.remove_node(node_id);
        }

        // 异步 PG 删除
        if let Some(pg) = &self.persistence {
            let pg = pg.clone();
            let ids = subtree.clone();
            tokio::task::spawn(async move {
                for id in &ids {
                    if let Err(e) = pg.delete_node(id).await {
                        tracing::warn!("SutraLibrary: PG delete failed: {}", e);
                    }
                }
            });
        }
    }

    /// 获取子节点
    pub fn get_children(&self, parent_id: &str) -> Vec<MemoryNode> {
        self.memory.find_children(parent_id)
    }

    // ─── 检索查询 ───────────────────────────────────────────────

    /// 三级检索入口
    pub async fn search(&self, query: &RetrievalQuery) -> RetrievalResult {
        self.retrieval.search(query).await
    }

    /// 简单搜索（快捷接口）
    pub async fn simple_search(
        &self,
        text: &str,
        collection_id: Option<&str>,
        limit: usize,
    ) -> Vec<MemoryNode> {
        let query = RetrievalQuery {
            text: text.to_string(),
            collection_id: collection_id.map(|s| s.to_string()),
            domain: None,
            node_type: None,
            limit,
            mode: ContextMode::Standard,
        };
        let result = self.retrieval.search(&query).await;
        result.nodes.into_iter().map(|s| s.node).collect()
    }

    // ─── 新版召回引擎（五阶段流水线） ───────────────────────────

    /// 执行新版召回流水线（五阶段：BaseSearch → Space → Graph → 合并 → 排序）
    ///
    /// 参考《藏经阁召回引擎·高层整合终极方案》
    pub async fn library_retrieve(
        &self,
        query: &str,
        config: &LibraryRetrieveConfig,
    ) -> Vec<recall::Candidate> {
        // 使用全局默认查询分析器（可通过 default_query_analyzer 注册领域同义词）
        let result = recall::pipeline::library_retrieve(
            &self.base_search,
            &self.space_strategy,
            &self.graph_strategy,
            query,
            config,
            default_query_analyzer(),
        )
        .await;

        result.candidates
    }

    /// 召回完成后，后台执行共现学习任务（不阻塞查询链路）
    ///
    /// 1. 从最终候选中提取全部实体
    /// 2. 实体两两共现，更新 Learned 熟练度边
    pub fn post_retrieve_learn(&self, candidates: &[recall::Candidate]) {
        let chunk_ids: Vec<ChunkUuid> = candidates.iter().map(|c| c.chunk_id.clone()).collect();
        if chunk_ids.is_empty() {
            return;
        }
        let graph = self.entity_graph.clone();
        let memory = self.memory.clone();
        tokio::task::spawn_blocking(move || {
            let entities = collect_entities_from_graph(&graph, &memory, &chunk_ids);
            graph.observe_entity_cooccurrence(&entities);
        });
    }

    // ─── 沉淀同步 ───────────────────────────────────────────────

    /// 临时会话记忆 → 热记忆沉淀
    pub fn precipitate_session(&self, session_id: &str, collection_id: &str, domain: &str) {
        let nodes = self.memory.get_session_memory(session_id);
        for node in nodes {
            let mut n = node;
            n.collection_id = collection_id.to_string();
            n.domain = domain.to_string();
            n.base_activation = 0.7; // 沉淀后设为热记忆
            n.updated_at = chrono::Utc::now().timestamp();
            self.memory.write_node(&n);

            // 索引到 Tantivy
            if let Some(ref tantivy) = self.tantivy {
                tantivy.index_node(&n);
            }

            if let Some(pg) = &self.persistence {
                let pg = pg.clone();
                tokio::task::spawn(async move {
                    if let Err(e) = pg.write_node(&n).await {
                        tracing::warn!("SutraLibrary: PG precipitate failed: {}", e);
                    }
                });
            }
        }
        self.memory.clear_session(session_id);
    }

    /// 启动回灌：把持久化层里已有的集合与节点加载回内存 + Tantivy 索引
    ///
    /// **这是"沉淀 → 下次召回"闭环缺失的第二块拼图**：
    /// 检索（BaseSearch）只查内存态 `MemoryStorage` + Tantivy，而这两个都是进程内结构。
    /// 之前虽然沉淀会写库，但新进程启动时从不回灌 → 换一次进程就"失忆"，
    /// 表现就是探针里"新会话完全想不起来"。
    ///
    /// 返回回灌的节点数。
    pub async fn hydrate(&self) -> usize {
        let Some(port) = &self.persistence else {
            return 0;
        };

        match port.list_all_collections().await {
            Ok(cols) => {
                for c in cols {
                    self.memory.create_collection(c);
                }
            }
            Err(e) => tracing::warn!("SutraLibrary: 回灌集合失败: {}", e),
        }

        let nodes = match port.list_all_nodes().await {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("SutraLibrary: 回灌节点失败: {}", e);
                return 0;
            }
        };
        tracing::debug!("SutraLibrary: 回灌读取到 {} 条节点", nodes.len());

        let count = nodes.len();

        // 全文索引与库对齐：文档数与节点数不一致，说明索引里残留了"幽灵文档"
        // （节点已删、索引未清）。它们会被检索命中却读不到内容，白占 top_k 名额
        // —— 实测 top_k=1 时表现为"库里明明有数据却返回未找到"。此时整体重建。
        if let Some(ref tantivy) = self.tantivy {
            let indexed = tantivy.num_docs();
            if indexed != count {
                tracing::info!(
                    "SutraLibrary: 全文索引文档数({}) 与节点数({}) 不一致，重建索引",
                    indexed,
                    count
                );
                tantivy.clear();
            }
        }

        for n in &nodes {
            self.memory.write_node(n);
            if let Some(ref tantivy) = self.tantivy {
                tantivy.index_node(n);
            }
            // 这里**不**调 `register_chunk`：领域实体抽取代价高，对整篇知识文档
            // 会把启动回灌卡住（实测十几秒不返回）。实体索引改为在**沉淀时**构建
            // ——那时文本是短事实，没有性能风险。
        }
        if let Some(ref tantivy) = self.tantivy {
            tantivy.commit();
        }
        // 注意：这里**不**调用 persist_graph()。
        // 回灌出来的共现边每条都要单条写库，实测让启动卡在回灌环节十几秒不返回。
        // 这些边本就是每次启动从节点重建的，无需再落盘。
        if count > 0 {
            tracing::info!("SutraLibrary: 已从持久化层回灌 {} 个记忆节点", count);
        }
        count
    }

    /// 异步沉淀：等持久化写库**真正完成**再返回
    ///
    /// 同步版 `precipitate_session` 用 `tokio::spawn` 落库，调用方无法等待；
    /// MCP stdio 这类"请求结束即退出进程"的场景下，写库任务可能在进程退出前
    /// 还没跑完 → 库里依旧是 0 行。编排层必须用这个版本。
    pub async fn precipitate_session_async(
        &self,
        session_id: &str,
        collection_id: &str,
        domain: &str,
    ) -> usize {
        let nodes = self.memory.get_session_memory(session_id);
        let known: HashSet<String> = self
            .memory
            .get_all_nodes()
            .iter()
            .map(|n| n.content_hash.clone())
            .collect();

        let mut written = 0usize;
        for node in nodes {
            let mut n = node;
            if known.contains(&n.content_hash) {
                continue; // 内容已存在，幂等跳过
            }
            n.collection_id = collection_id.to_string();
            n.domain = domain.to_string();
            n.base_activation = 0.7; // 沉淀后设为热记忆
            n.updated_at = chrono::Utc::now().timestamp();
            self.memory.write_node(&n);
            if let Some(ref tantivy) = self.tantivy {
                tantivy.index_node(&n);
            }
            if let Some(pg) = &self.persistence {
                if let Err(e) = pg.write_node(&n).await {
                    tracing::warn!("SutraLibrary: 沉淀落库失败: {}", e);
                }
            }
            written += 1;
            // 实体入图（P3）：建立 chunk↔entity 索引，图谱通路才有数据可走。
            // 领域切片是 Markdown 说明时 `extract_entity_relations` 通常为空，
            // 但实体索引（register_chunk）仍然有价值——它让"按实体扩召回"成为可能。
            self.entity_graph.register_chunk(&n);
            if let Some(parser) = self.domain_router.parser_for(domain) {
                for (a, b, w) in parser.extract_entity_relations(&n.content) {
                    if !a.is_empty() && !b.is_empty() {
                        self.entity_graph.add_manual_edge(&a, &b, w);
                    }
                }
            }
        }
        if written > 0 {
            if let Some(ref tantivy) = self.tantivy {
                tantivy.commit();
            }
            // 图谱边落库（P3）：让实体关系也能跨进程存活
            self.persist_graph().await;
            tracing::info!(
                "SutraLibrary: 会话 {} 沉淀 {} 条记忆到集合 {}",
                session_id,
                written,
                collection_id
            );
        }
        self.memory.clear_session(session_id);
        written
    }

    /// 一键沉淀：建/复用领域集合 → 写入会话记忆 → 落库
    ///
    /// 供编排层在每轮对话结束后调用（P0 主链路）。
    ///
    /// 命名与 trait 方法 `auto_precipitate` 区分开，避免固有/trait 同名时
    /// 方法解析落到 trait 实现上导致无限递归。
    pub async fn precipitate_auto(
        &self,
        session_id: &str,
        domain: &str,
        facts: &[String],
    ) -> usize {
        if facts.is_empty() || session_id.is_empty() {
            return 0;
        }
        let name = format!("{}_memory", domain);
        let col = self.create_collection(&name, domain, &format!("{} 领域会话沉淀记忆", domain));
        // 用独立的会话命名空间：专家（如 blender.rs）也会往同一个 session_id
        // 里 `add_session` 写入**用户当前问题**，若直接沉淀同一个 key，
        // 就会把"我刚才说的渲染器和采样值是多少？"这类提问也当事实入库。
        let key = format!("{}#consolidate", session_id);
        for f in facts {
            self.add_session_memory(&key, f, domain);
        }
        self.precipitate_session_async(&key, &col.collection_id, domain)
            .await
    }

    /// 从正文推导标题（取首个自然句或前 24 字，避免半截词）
    fn derive_title(content: &str) -> String {
        let first_line = content
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches(|c: char| c == '-' || c == '*' || c == '#' || c.is_whitespace());
        for sep in ['。', '！', '？', '!', '?', '；', ';'] {
            if let Some(idx) = first_line.find(sep) {
                let head = first_line[..idx].trim();
                if head.chars().count() >= 4 {
                    return head.chars().take(24).collect();
                }
            }
        }
        first_line.chars().take(24).collect()
    }

    /// 判定某条召回记忆是否体现在最终回答里
    ///
    /// 做法：对记忆正文做 jieba 分词，取有信息量的实词（非停用词、长度≥2），
    /// 只要回答里出现其中任意一个，就认为这条记忆被用上了。
    /// 这是"证据下限"而非精确归因——宁可略微高估，也不要恒为 0。
    fn chunk_used_in(&self, chunk_id: &str, answer: &str) -> bool {
        if answer.trim().is_empty() {
            return false;
        }
        let node = match self.memory.read_node(chunk_id) {
            Some(n) => n,
            None => return false,
        };
        let text = format!("{} {}", node.title, node.content);
        if text.trim().is_empty() {
            return false;
        }
        let jieba = jieba_rs::Jieba::new();
        let tokens = jieba.tokenize(&text, jieba_rs::TokenizeMode::Search, true);
        for tk in &tokens {
            let w = &text[tk.byte_start..tk.byte_end];
            if w.chars().count() < 2 {
                continue;
            }
            if TantivyIndex::is_stop_word(w) {
                continue;
            }
            if answer.contains(w) {
                return true;
            }
        }
        false
    }

    /// 回灌实体图谱（P3）
    ///
    /// 此前只有 PG 模式会从库里加载图谱，SQLite 降级时图谱永远从零开始，
    /// 图谱通路（GraphPassageStrategy）因此形同虚设。这里补齐非 PG 路径。
    pub async fn hydrate_graph(&self) -> usize {
        let Some(port) = &self.persistence else {
            return 0;
        };
        let empty_adj = HashMap::new();
        let empty_e2c: HashMap<String, Vec<String>> = HashMap::new();
        let empty_c2e: HashMap<String, Vec<String>> = HashMap::new();
        let empty_ef: HashMap<String, f32> = HashMap::new();
        let (_, _, c2e, _) = match port
            .load_all(&empty_adj, &empty_e2c, &empty_c2e, &empty_ef)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("SutraLibrary: 图谱回灌失败（可忽略）: {}", e);
                return 0;
            }
        };

        // 先注册「切片 → 实体」索引，再据此按**当前建边规则**重建边。
        //
        // 不再直接导入库里那份邻接表：它可能由旧规则生成（含大量 2 字碎片
        // 共现），导进来等于把历史噪声固化。实体索引才是唯一真相，
        // 建边规则收紧后图谱应当自动自愈。
        for (chunk, entities) in &c2e {
            if !entities.is_empty() {
                self.entity_graph
                    .register_chunk_entities(chunk, entities.clone());
            }
        }

        // 节点自身 UUID 不参与共现（只指向自己的碎片，浪费边数）
        let chunk_ids: HashSet<String> = c2e.keys().cloned().collect();
        for entities in c2e.values() {
            let semantic: Vec<String> = entities
                .iter()
                .filter(|e| !chunk_ids.contains(*e))
                .cloned()
                .collect();
            self.entity_graph.link_cooccurrence(&semantic);
        }

        let n = self.entity_graph.edge_count();
        if n > 0 {
            tracing::info!("SutraLibrary: 实体图谱回灌完成，按当前规则重建 {} 条边", n);
        }
        n
    }

    /// 落库实体图谱边（P3）
    pub async fn persist_graph(&self) -> usize {
        let Some(port) = &self.persistence else {
            return 0;
        };
        let edges = self.entity_graph.export_edges();
        let mut ok = 0usize;
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for (from, to, kind, w) in &edges {
            let key = if from <= to {
                (from.clone(), to.clone())
            } else {
                (to.clone(), from.clone())
            };
            if !seen.insert(key) {
                continue;
            }
            let kind_str = match kind {
                recall::graph::EdgeKind::Manual => "Manual",
                recall::graph::EdgeKind::Learned => "Learned",
            };
            if port.write_edge(from, to, kind_str, *w).await.is_ok() {
                ok += 1;
            }
        }
        ok
    }

    /// 批量落库（await 语义，供种子数据等需要确保写盘的场景）
    pub async fn persist_nodes(&self, nodes: &[MemoryNode]) -> usize {
        let Some(port) = &self.persistence else {
            return 0;
        };
        let mut ok = 0usize;
        for n in nodes {
            if port.write_node(n).await.is_ok() {
                ok += 1;
            }
        }
        ok
    }

    /// 冷启动灌入领域静态知识（P1）
    ///
    /// 已存在（同 content_hash）的条目自动跳过，重复调用安全。
    /// 返回实际新写入的节点数。
    pub async fn seed_domain_knowledge(&self, domain: &str, entries: &[(String, String)]) -> usize {
        let name = format!("{}_knowledge", domain);
        let col = self.create_collection(&name, domain, &format!("{} 领域静态知识", domain));
        let known: HashSet<String> = self
            .memory
            .get_all_nodes()
            .iter()
            .map(|n| n.content_hash.clone())
            .collect();

        let mut written = Vec::new();
        for (title, content) in entries {
            if known.contains(&Self::hash_content(content)) {
                continue;
            }
            // 种子数据一律**直存原文**，不走 `write_node`（领域切片）。
            //
            // 两个实测教训：
            // 1. 领域切片器是为代码/脚本设计的，整篇 Markdown 知识常被判为
            //    "无有效节点" → 冷启动静默灌入 0 条；
            // 2. 更糟的是长文本会触发领域正则的灾难性回溯，
            //    实测种子任务卡死 8 秒以上仍不返回。
            // 知识条目的粒度本就适合整体检索，切碎反而丢失语义完整性。
            written.push(self.make_plain_node(&col.collection_id, title, content, domain));
        }
        for n in &written {
            self.memory.write_node(n);
            if let Some(ref tantivy) = self.tantivy {
                tantivy.index_node(n);
            }
        }
        if !written.is_empty() {
            if let Some(ref tantivy) = self.tantivy {
                tantivy.commit();
            }
        }
        let n = written.len();
        if n > 0 {
            self.persist_nodes(&written).await;
        }
        n
    }

    /// 构造一个不切片的原文节点（冷启动兜底用）
    fn make_plain_node(
        &self,
        collection_id: &str,
        title: &str,
        content: &str,
        domain: &str,
    ) -> MemoryNode {
        let now = chrono::Utc::now().timestamp();
        MemoryNode {
            node_id: Uuid::new_v4().to_string(),
            collection_id: collection_id.to_string(),
            domain: domain.to_string(),
            node_type: "knowledge".to_string(),
            content_hash: Self::hash_content(content),
            parent_id: None,
            path: format!("/knowledge/{}", title),
            depth: 0,
            sort_order: 0,
            title: title.to_string(),
            summary: String::new(),
            content: content.to_string(),
            metadata: serde_json::json!({"seeded": true}),
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: "current".to_string(),
            snapshot_id: None,
            base_activation: 0.6,
            importance: 3,
            access_count: 0,
            feedback_score: 0.0,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    /// 添加会话临时记忆
    pub fn add_session_memory(&self, session_id: &str, content: &str, domain: &str) {
        let now = chrono::Utc::now().timestamp();
        let node = MemoryNode {
            node_id: Uuid::new_v4().to_string(),
            collection_id: "session".to_string(),
            domain: domain.to_string(),
            node_type: "session_message".to_string(),
            content_hash: Self::hash_content(content),
            parent_id: None,
            path: format!("/session/{}", session_id),
            depth: 0,
            sort_order: 0,
            // 标题此前恒为空，导致召回结果格式化出来是 `****`，
            // 人/LLM 都看不出这条记忆是什么。这里兜底取正文前 24 字。
            title: Self::derive_title(content),
            summary: String::new(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: "current".to_string(),
            snapshot_id: None,
            base_activation: 1.0,
            importance: 1,
            access_count: 0,
            feedback_score: 0.0,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
        };
        // 索引到 Tantivy（在 move 之前克隆）
        let node_for_tantivy = node.clone();
        self.memory.add_session_memory(session_id, node);
        if let Some(ref tantivy) = self.tantivy {
            tantivy.index_node(&node_for_tantivy);
        }
    }

    // ─── 热度管理 ───────────────────────────────────────────────

    /// 更新热度（定时任务调用）
    pub fn refresh_hotness(&self) {
        let now = chrono::Utc::now().timestamp();
        let nodes = self.memory.get_all_nodes();
        for node in &nodes {
            let activation = HotnessCalculator::compute_activation(node, now);
            if (activation - node.base_activation).abs() > 0.01 {
                let mut n = node.clone();
                n.base_activation = activation;
                n.updated_at = now;
                self.memory.write_node(&n);
            }
        }
    }

    /// 启动 Learned 边衰减定时任务（后台异步，不阻塞主线程）
    ///
    /// 默认每 24 小时执行一次衰减，衰减因子 0.9（保留 90% 权重）。
    /// 可通过 `interval_hours` 自定义间隔。
    pub fn start_decay_task(&self, interval_hours: u64) {
        let graph = self.entity_graph.clone();
        let hours = if interval_hours == 0 {
            24
        } else {
            interval_hours
        };
        tokio::task::spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(hours * 3600)).await;
                graph.decay_learned_edges(0.9);
                tracing::info!("SutraLibrary: Learned 边衰减完成");
            }
        });
    }

    // ─── 反馈校验 ───────────────────────────────────────────────

    /// 用户反馈调整（反哺图谱边权重和空间排序权重）
    pub fn apply_feedback(&self, node_id: &str, positive: bool) {
        if let Some(mut node) = self.memory.read_node(node_id) {
            node.feedback_score += if positive { 0.1 } else { -0.1 };
            node.feedback_score = node.feedback_score.clamp(-1.0, 1.0);
            node.updated_at = chrono::Utc::now().timestamp();
            self.memory.write_node(&node);

            if let Some(pg) = &self.persistence {
                let pg = pg.clone();
                tokio::task::spawn(async move {
                    if let Err(e) = pg.write_node(&node).await {
                        tracing::warn!("SutraLibrary: PG feedback update failed: {}", e);
                    }
                });
            }

            // 反馈反哺：将用户反馈传播到实体图谱的边权重
            let chunk_id: ChunkUuid = node_id.to_string();
            self.entity_graph
                .apply_feedback_to_entities(&chunk_id, positive);
        }
    }

    // ─── 统计 ───────────────────────────────────────────────────

    pub fn stats(&self) -> SutraStats {
        let mut s = self.memory.stats();
        // 边只存在于 EntityGraph 里，MemoryStorage 不知道图谱的存在，
        // 直接用它的值会永远显示 0——可观测性上等于"图谱没启用"的假象。
        s.edges = self.entity_graph.edge_count();
        s
    }

    // ─── 工具方法 ───────────────────────────────────────────────

    fn hash_content(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

#[async_trait]
impl SutraLibraryPort for MemoryEnginePort {
    fn create_collection(&self, name: &str, domain: &str, description: &str) -> String {
        let collection = self.create_collection(name, domain, description);
        format!(
            "✅ 已创建集合\n集合ID: {}\n名称: {}\n领域: {}\n描述: {}",
            collection.collection_id, collection.name, collection.domain, collection.description
        )
    }

    fn list_collections(&self) -> String {
        let collections = self.list_collections();
        if collections.is_empty() {
            return "暂无记忆集合".to_string();
        }
        let mut output = "📚 记忆集合列表\n".to_string();
        output.push_str(&format!("{:<40} {:<20} {:<15}\n", "集合ID", "名称", "领域"));
        output.push_str(&"-".repeat(80));
        output.push('\n');
        for c in &collections {
            output.push_str(&format!(
                "{:<40} {:<20} {:<15}\n",
                c.collection_id, c.name, c.domain
            ));
        }
        output
    }

    fn write(
        &self,
        collection_id: &str,
        content: &str,
        domain: &str,
        parent_id: Option<&str>,
    ) -> String {
        match self.write_node(collection_id, content, domain, parent_id) {
            Ok(node) => {
                format!(
                    "✅ 已写入记忆\n节点ID: {}\n标题: {}\n路径: {}\n类型: {}\n摘要: {}",
                    node.node_id, node.title, node.path, node.node_type, node.summary
                )
            }
            Err(e) => format!("❌ 写入失败: {}", e),
        }
    }

    fn read(&self, node_id: &str) -> String {
        match self.read_node(node_id) {
            Some(node) => format!(
                "📄 记忆节点\n节点ID: {}\n标题: {}\n路径: {}\n类型: {}\n领域: {}\n摘要: {}\n激活分: {:.2}\n版本: {}",
                node.node_id, node.title, node.path, node.node_type, node.domain,
                node.summary, node.base_activation, node.version_tag
            ),
            None => format!("❌ 未找到节点: {}", node_id),
        }
    }

    async fn search(&self, text: &str, collection_id: Option<&str>, limit: usize) -> String {
        let query = RetrievalQuery {
            text: text.to_string(),
            collection_id: collection_id.map(|s| s.to_string()),
            domain: None,
            node_type: None,
            limit,
            mode: ContextMode::Standard,
        };
        let result = self.search(&query).await;
        if result.nodes.is_empty() {
            return "未找到匹配的记忆".to_string();
        }
        let mut output = format!(
            "🔍 搜索结果 (共 {} 条, 来源: {:?})\n\n",
            result.nodes.len(),
            result.source
        );
        for (i, scored) in result.nodes.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}** (得分: {:.2})\n   路径: {} | 类型: {} | 领域: {}\n   摘要: {}\n\n",
                i + 1,
                scored.node.title,
                scored.score,
                scored.node.path,
                scored.node.node_type,
                scored.node.domain,
                scored.node.summary.chars().take(100).collect::<String>(),
            ));
        }
        output
    }

    async fn library_retrieve(&self, query: &str, top_k: usize) -> String {
        use crate::sutra_library::recall::LibraryRetrieveConfig;
        let config = LibraryRetrieveConfig {
            base_top_k: top_k,
            graph_max_global_result: top_k * 2,
            ..Default::default()
        };
        let candidates = self.library_retrieve(query, &config).await;

        if candidates.is_empty() {
            return "未找到匹配的记忆".to_string();
        }

        // 组装时过滤两类脏项（09-14 实测，二者都会污染提示词）：
        // 1. **总分归零**的候选 —— rank 归一化后每条通路的末位都是 0，意味着
        //    没有任何证据支持它，此前却照样出现在结果里；
        // 2. **内容全空**的节点 —— 此前会输出成 `1. **(无标题)** (总分: 0.0000)`，
        //    对 LLM 而言是纯噪声。
        let mut body = String::new();
        let mut shown = 0usize;
        let mut shown_ids: Vec<String> = Vec::new();
        for c in candidates.iter() {
            // `top_k` 是调用方要的**最终条数**。base 与 graph 两路各自取数后
            // 此前从不截断，于是 top_k=1 也会返回 2 条（Base 1 + GraphFirstPass 1），
            // 多出来的通常是 0.075~0.3 分的共现邻居噪声（09-14 实测）。
            if shown >= top_k {
                break;
            }
            let total = c.base_score + c.bonus_score;
            if total <= 1e-6 {
                continue;
            }
            // ⚠️ 必须把正文带上：此前只输出 title，而会话沉淀的节点 title 为空
            // （`add_session_memory` 不设标题），于是专家拿到的"历史知识"是一串
            // `**** (总分: 1.0000)` —— 检索命中了，但 LLM 什么信息都没读到，
            // 表现就是"沉淀成功却依旧失忆"。正文才是记忆的实际载体。
            let (title, content, summary) = match self.read_node(&c.chunk_id) {
                Some(n) => (n.title, n.content, n.summary),
                None => continue,
            };
            if title.trim().is_empty() && content.trim().is_empty() && summary.trim().is_empty() {
                continue;
            }
            // 无标题的节点（会话沉淀）用正文开头当标题，避免一串「(无标题)」无法区分
            let display_title = if title.trim().is_empty() {
                let head: String = content.trim().chars().take(24).collect();
                if head.is_empty() {
                    "(无标题)".to_string()
                } else {
                    head
                }
            } else {
                title
            };

            shown += 1;
            shown_ids.push(c.chunk_id.clone());
            let sources: Vec<String> = c.source_flags.iter().map(|f| format!("{:?}", f)).collect();
            body.push_str(&format!(
                "{}. **{}** (总分: {:.4}, base: {:.4}, bonus: {:.4})\n   来源: {:?}\n",
                shown, display_title, total, c.base_score, c.bonus_score, sources,
            ));

            // 优先给 summary，没有再截断正文（防止超长污染提示词）
            let text = if summary.is_empty() {
                &content
            } else {
                &summary
            };
            if !text.trim().is_empty() {
                let chars: Vec<char> = text.chars().take(300).collect();
                let s: String = chars.into_iter().collect();
                if text.chars().count() > 300 {
                    body.push_str(&format!("   内容: {}…\n", s));
                } else {
                    body.push_str(&format!("   内容: {}\n", s));
                }
            }
            body.push('\n');
        }

        if shown == 0 {
            return "未找到匹配的记忆".to_string();
        }

        // 缓存**实际返回**的 chunk_ids（而非全部候选），供 record_execution 统计命中率。
        // 放在截断与去脏之后，保证"召回 N 条"与调用方看到的条数一致。
        {
            let query_hash = format!("{:x}", Sha256::digest(query.as_bytes()));
            {
                let mut recent = self.last_retrieve_recent.write().unwrap();
                *recent = (chrono::Utc::now().timestamp_millis(), shown_ids.clone());
            }
            let mut cache = self.last_retrieve_cache.write().unwrap();
            cache.insert(query_hash, shown_ids);
            // 限制缓存大小，避免内存泄漏
            if cache.len() > 100 {
                cache.clear();
            }
        }

        format!("🔍 新版召回检索结果 (共 {} 条)\n\n{}", shown, body)
    }

    fn delete(&self, node_id: &str) -> String {
        self.delete_node(node_id);
        format!("✅ 已删除节点及其子树: {}", node_id)
    }

    fn add_session(&self, session_id: &str, content: &str, domain: &str) -> String {
        self.add_session_memory(session_id, content, domain);
        format!("✅ 已添加会话临时记忆: {}", session_id)
    }

    fn precipitate(&self, session_id: &str, collection_id: &str, domain: &str) -> String {
        self.precipitate_session(session_id, collection_id, domain);
        format!("✅ 已沉淀会话 {} 到集合 {}", session_id, collection_id)
    }

    async fn auto_precipitate(&self, session_id: &str, domain: &str, facts: &[String]) -> usize {
        self.precipitate_auto(session_id, domain, facts).await
    }

    async fn seed_knowledge(&self, domain: &str, entries: &[(String, String)]) -> usize {
        self.seed_domain_knowledge(domain, entries).await
    }

    fn stats_json(&self) -> serde_json::Value {
        let s = self.stats();
        serde_json::json!({
            "total_nodes": s.total_nodes,
            "hot_nodes": s.hot_nodes,
            "cold_nodes": s.cold_nodes,
            "collections": s.collections,
            "edges": s.edges,
            "snapshots": s.snapshots,
            "entity_chunks": self.entity_graph.entity_chunk_pairs(),
        })
    }

    fn like(&self, node_id: &str) -> String {
        self.apply_feedback(node_id, true);
        format!("👍 已记录正反馈: {}", node_id)
    }

    fn dislike(&self, node_id: &str) -> String {
        self.apply_feedback(node_id, false);
        format!("👎 已记录负反馈: {}", node_id)
    }

    fn stats(&self) -> String {
        let s = self.stats();
        format!(
            "📊 藏经阁统计\n总节点数: {}\n热记忆: {}\n冷记忆: {}\n集合数: {}\n关联边: {}\n快照数: {}",
            s.total_nodes, s.hot_nodes, s.cold_nodes, s.collections, s.edges, s.snapshots
        )
    }

    fn record_execution(
        &self,
        query: &str,
        task_success: bool,
        graph: &str,
        domain: &str,
        session_id: Option<String>,
        final_answer: &str,
    ) -> String {
        let query_hash = format!("{:x}", Sha256::digest(query.as_bytes()));

        // 从缓存中查找对应的检索结果：先按 query_hash 精确匹配，
        // 匹配不到时退到"最近一次召回"（60 秒内），避免因 query 文本微差而永远统计为 0。
        let recalled_ids: Vec<String> = {
            let cache = self.last_retrieve_cache.read().unwrap();
            if let Some(ids) = cache.get(&query_hash) {
                ids.clone()
            } else {
                drop(cache);
                let recent = self.last_retrieve_recent.read().unwrap();
                let now = chrono::Utc::now().timestamp_millis();
                let age_ms = now - recent.0;
                // 放宽到 5 分钟：一次编排里 LLM 可能耗时很久，
                // 60 秒窗口会让慢请求统计成"召回 0 条"（实测命中率被低估）。
                if recent.0 > 0 && age_ms < 300_000 {
                    recent.1.clone()
                } else {
                    tracing::debug!(
                        "反馈记录: recent 不可用 (age={}ms, ids={})",
                        age_ms,
                        recent.1.len()
                    );
                    Vec::new()
                }
            }
        };
        let recalled_chunks: Vec<RecallChunkInfo> = recalled_ids
            .iter()
            .map(|id| RecallChunkInfo {
                chunk_id: id.clone(),
                source: RetrieveSource::Base,
                score: 0.0,
                llm_relevance: None,
                llm_reason: None,
            })
            .collect();

        // 判定"哪些召回切片真的被用上了"。
        //
        // 此前 `used_chunk_ids` 恒为空向量，命中率因此永远是 0.00%——
        // 反馈信号没有任何信息量，热度和排序也就无从调优。
        // 这里用"最终回答是否包含该记忆的关键内容"作为使用证据：
        // 不追求精确，但足以让命中率变成一个有区分度的指标。
        let used_chunk_ids: Vec<String> = recalled_chunks
            .iter()
            .filter(|c| self.chunk_used_in(&c.chunk_id, final_answer))
            .map(|c| c.chunk_id.clone())
            .collect();

        // 可观测：命中率此前恒为 0，先让"召回了多少 / 用上了多少"可见
        //
        // ⚠️ 截断必须按**字符**切（`chars().take`），不能按字节下标切——
        // `&query[..len.min(30)]` 在 30 字节落在多字节 UTF-8 字符中间会直接 panic，
        // 实测由「Arc 和 Mutex 有什么区别」触发的 `end byte index 30 is not a char
        // boundary; it is inside '有'` 把 subhuti_chat 的 HTTP 响应整段带崩。
        let query_preview: String = query.chars().take(30).collect();
        tracing::info!(
            "📈 反馈记录: 召回 {} 条 / 命中 {} 条（domain={}, query={}）",
            recalled_chunks.len(),
            used_chunk_ids.len(),
            domain,
            query_preview
        );

        let log = ExecutionLog {
            query_hash: query_hash.clone(),
            query: query.to_string(),
            recalled_chunks,
            used_chunk_ids,
            task_success,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            graph: graph.to_string(),
            domain: domain.to_string(),
            session_id,
        };

        self.feedback_analyzer.record_execution(log);
        format!("✅ 已记录执行日志: query_hash={}", query_hash)
    }

    // ─── 知识库 CRUD 实现 ──────────────────────────────────────

    async fn list_knowledge_bases(&self) -> String {
        let pg = match &self.persistence {
            Some(pg) => pg,
            None => return "⚠️ 无持久化后端，无法查询知识库".to_string(),
        };

        match pg.list_knowledge_bases().await {
            Ok(bases) => match serde_json::to_string_pretty(&bases) {
                Ok(json) => json,
                Err(e) => format!("⚠️ 序列化知识库列表失败: {}", e),
            },
            Err(e) => format!("⚠️ 查询知识库列表失败: {}", e),
        }
    }

    async fn list_chunks(&self, kb_id: &str) -> String {
        let pg = match &self.persistence {
            Some(pg) => pg,
            None => return "⚠️ 无持久化后端，无法查询知识库切片".to_string(),
        };

        match pg.list_chunks(kb_id).await {
            Ok(chunks) => match serde_json::to_string_pretty(&chunks) {
                Ok(json) => json,
                Err(e) => format!("⚠️ 序列化切片列表失败: {}", e),
            },
            Err(e) => format!("⚠️ 查询知识库切片失败: {}", e),
        }
    }

    async fn get_knowledge_base_by_expert(&self, expert_id: &str) -> String {
        let pg = match &self.persistence {
            Some(pg) => pg,
            None => return "⚠️ 无持久化后端，无法查询专家知识库".to_string(),
        };

        // 先获取所有知识库，然后过滤出 expert_id 匹配的
        match pg.list_knowledge_bases().await {
            Ok(bases) => {
                let filtered: Vec<_> = bases
                    .into_iter()
                    .filter(|b| b.expert_id == expert_id || b.expert_id.is_empty())
                    .collect();
                match serde_json::to_string_pretty(&filtered) {
                    Ok(json) => json,
                    Err(e) => format!("⚠️ 序列化知识库列表失败: {}", e),
                }
            }
            Err(e) => format!("⚠️ 查询知识库失败: {}", e),
        }
    }
}

// ─── 工具函数 ───────────────────────────────────────────────

/// 从图谱索引或 MemoryNode 中提取实体集合（用于异步学习任务）
fn collect_entities_from_graph(
    graph: &EntityGraph,
    memory: &MemoryStorage,
    chunk_ids: &[ChunkUuid],
) -> HashSet<EntityUuid> {
    // 优先从图谱索引中提取
    let graph_entities = graph.get_entities_of_chunks(chunk_ids);
    if !graph_entities.is_empty() {
        return graph_entities;
    }
    // 回退：从 MemoryNode 中提取
    let nodes: Vec<MemoryNode> = chunk_ids
        .iter()
        .filter_map(|id| memory.read_node(id))
        .collect();
    collect_entities(&nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 纯内存引擎（无持久化），用于验证集合注册语义。
    fn engine() -> MemoryEnginePort {
        MemoryEnginePort::new(
            Arc::new(MemoryStorage::new()),
            None,
            None,
            Arc::new(DomainRouter::new()),
            None, // 测试用内存索引，避免污染真实索引目录
        )
    }

    /// 关键回归：沉淀下去的事实必须能被召回。
    ///
    /// 这条测试锁的是"记忆闭环的最小可证伪单元"：如果沉淀后检索不出来，
    /// 上层再怎么接都是空转（曾长期处于这种状态：沉淀写库成功但检索恒空）。
    #[tokio::test]
    async fn precipitated_facts_are_retrievable() {
        let e = engine();
        let col = e.create_collection("t", "blender", "test");

        e.add_session_memory("s1", "用户渲染器用 Cycles", "blender");
        e.add_session_memory("s1", "采样值固定 128", "blender");
        let n = e
            .precipitate_session_async("s1", &col.collection_id, "blender")
            .await;
        assert_eq!(n, 2, "两条事实都应沉淀成功");

        let cfg = LibraryRetrieveConfig::default();
        let hits = MemoryEnginePort::library_retrieve(&e, "渲染器 采样值", &cfg).await;
        assert!(!hits.is_empty(), "沉淀后必须能召回，否则闭环是断的");
    }

    /// `top_k` 必须约束**最终返回条数**。
    ///
    /// 历史 bug：`base_top_k = top_k` 而 `graph_max_global_result = top_k * 2`，
    /// 两路各自取数、合并后**从不截断** → `top_k=1` 也返回 2 条
    /// （Base 1 条 + GraphFirstPass 1 条）。对只想拿最相关 1 条的调用方
    /// （MCP `recall(top_k=1)`）是明确的契约违约；多出的那条通常是
    /// 0.075~0.3 分的共现邻居噪声（09-14 实测）。
    #[tokio::test]
    async fn top_k_caps_final_result_count() {
        let e = engine();
        let col = e.create_collection("t", "blender", "test");
        for f in [
            "渲染输出目录固定 /tmp/render_out",
            "采样值固定 128",
            "输出格式统一 PNG",
        ] {
            e.add_session_memory("s1", f, "blender");
        }
        e.precipitate_session_async("s1", &col.collection_id, "blender")
            .await;

        // 注意：`MemoryEnginePort` 上的 inherent `library_retrieve(&self, q, &config)`
        // 会**遮蔽** trait 里的同名方法，这里必须显式走 `SutraLibraryPort`，
        // 才能测到面向调用方的 String 层——截断正是加在那一层。
        let one = SutraLibraryPort::library_retrieve(&e, "渲染 输出目录", 1).await;
        assert_eq!(
            shown_count(&one),
            1,
            "top_k=1 必须只返回 1 条，实际输出：\n{}",
            one
        );

        let two = SutraLibraryPort::library_retrieve(&e, "渲染 输出目录", 2).await;
        assert!(
            shown_count(&two) <= 2,
            "top_k=2 不应超过 2 条，实际输出：\n{}",
            two
        );
    }

    /// 从召回文本的统计行里抠出「共 N 条」的 N
    fn shown_count(text: &str) -> usize {
        text.lines()
            .find(|l| l.contains("召回检索结果") && l.contains("共 "))
            .and_then(|l| l.split_once("共 "))
            .and_then(|(_, rest)| rest.split_once(" 条"))
            .and_then(|(n, _)| n.trim().parse().ok())
            .unwrap_or(0)
    }

    /// 关键回归：同名同域的集合必须复用，不能每次注册都新建。
    ///
    /// 历史 bug 是落库只按 `collection_id` 去重、而 ID 每次都是新 UUID，
    /// 于是每次启动都新增一行，实测把 `blender_knowledge` 堆了 45 份。
    /// 重复集合不只是脏数据——`list_collections()` 会返回多份同名集合，
    /// 同一份知识在检索时被反复召回。
    #[test]
    fn create_collection_is_idempotent_by_name_and_domain() {
        let e = engine();

        let a = e.create_collection("blender_knowledge", "blender", "Blender 知识");
        let b = e.create_collection("blender_knowledge", "blender", "Blender 知识");

        assert_eq!(
            a.collection_id, b.collection_id,
            "同名同域必须复用同一个集合 ID"
        );
        assert_eq!(e.list_collections().len(), 1, "不应堆出重复集合");

        // 同名但不同域 → 视为不同集合，必须各占一条
        let c = e.create_collection("blender_knowledge", "rust", "另一个域");
        assert_ne!(a.collection_id, c.collection_id, "跨域不应被误判为同一集合");
        assert_eq!(e.list_collections().len(), 2);
    }
}
