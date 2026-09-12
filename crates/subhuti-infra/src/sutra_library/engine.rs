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
    LibraryRetrieveConfig, RetrieveSource, SpaceDepthStrategy, TantivyIndex,
};
use crate::sutra_library::retrieval::RetrievalScheduler;
use crate::sutra_library::storage::{MemoryStorage, PgGraphStorage, PgStorage};
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
}

impl MemoryEnginePort {
    pub fn new(
        memory: Arc<MemoryStorage>,
        persistence: Option<Arc<dyn PersistencePort>>,
        feedback_pg: Option<Arc<PgStorage>>,
        domain_router: Arc<DomainRouter>,
    ) -> Self {
        let retrieval =
            RetrievalScheduler::new(memory.clone(), persistence.clone(), domain_router.clone());
        let entity_graph = Arc::new(EntityGraph::new());
        let graph_strategy = GraphPassageStrategy::new(entity_graph.clone());

        // 创建 Tantivy 索引（内存模式，进程内运行）
        let tantivy = Arc::new(TantivyIndex::new_in_ram());
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

    /// 为实体图谱启用 PG 持久化（异步加载已有数据到内存，不阻塞）
    ///
    /// 在启动时调用，通过 spawn 后台加载 PG 数据到图谱内存中。
    pub fn init_graph_pg_async(&self, pg: Arc<PgGraphStorage>) {
        let entity_graph = self.entity_graph.clone();
        tokio::task::spawn(async move {
            entity_graph.load_pg_data(pg).await;
        });
    }

    /// 获取实体图谱引用
    pub fn entity_graph(&self) -> &Arc<EntityGraph> {
        &self.entity_graph
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
            title: String::new(),
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
        self.memory.stats()
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

        // 缓存本次检索结果（chunk_ids），供 record_execution 使用
        {
            let query_hash = format!("{:x}", Sha256::digest(query.as_bytes()));
            let chunk_ids: Vec<String> = candidates.iter().map(|c| c.chunk_id.clone()).collect();
            let mut cache = self.last_retrieve_cache.write().unwrap();
            cache.insert(query_hash, chunk_ids);
            // 限制缓存大小，避免内存泄漏
            if cache.len() > 100 {
                cache.clear();
            }
        }

        if candidates.is_empty() {
            return "未找到匹配的记忆".to_string();
        }
        let mut output = format!("🔍 新版召回检索结果 (共 {} 条)\n\n", candidates.len());
        for (i, c) in candidates.iter().enumerate() {
            let title = self
                .read_node(&c.chunk_id)
                .map(|n| n.title)
                .unwrap_or_else(|| "未知".to_string());
            let total = c.base_score + c.bonus_score;
            let sources: Vec<String> = c.source_flags.iter().map(|f| format!("{:?}", f)).collect();
            output.push_str(&format!(
                "{}. **{}** (总分: {:.4}, base: {:.4}, bonus: {:.4})\n   来源: {:?}\n\n",
                i + 1,
                title,
                total,
                c.base_score,
                c.bonus_score,
                sources,
            ));
        }
        output
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
    ) -> String {
        let query_hash = format!("{:x}", Sha256::digest(query.as_bytes()));

        // 从缓存中查找对应的检索结果
        let recalled_chunks: Vec<RecallChunkInfo> = {
            let cache = self.last_retrieve_cache.read().unwrap();
            cache
                .get(&query_hash)
                .map(|chunk_ids| {
                    chunk_ids
                        .iter()
                        .map(|id| RecallChunkInfo {
                            chunk_id: id.clone(),
                            source: RetrieveSource::Base, // 简化：标记为 Base 通路
                            score: 0.0,
                            llm_relevance: None,
                            llm_reason: None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let log = ExecutionLog {
            query_hash: query_hash.clone(),
            query: query.to_string(),
            recalled_chunks,
            used_chunk_ids: Vec::new(), // 专家层面无法精确知道，留空让反馈分析器仅统计基础指标
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
