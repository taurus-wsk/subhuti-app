//! # Tantivy 全文检索引擎
//!
//! 替代旧版内存 BM25 + 关键词检索，替换为进程内嵌入式搜索引擎。
//!
//! ## 架构
//!
//! - 使用 tantivy（Rust 生态的 Lucene）作为全文检索引擎
//! - 进程内运行，无需单独启动服务
//! - 支持中文分词（jieba-rs）
//! - 支持布尔过滤（空间抽屉、知识节点、实体）
//! - 原生 BM25 打分，可自定义 k1/b 参数
//!
//! ## Schema 字段
//!
//! | 字段 | 类型 | 用途 |
//! |------|------|------|
//! | node_id | STRING + STORED | 唯一标识 |
//! | title | TEXT + STORED | 标题（可搜索） |
//! | content | TEXT | 正文（可搜索，主字段） |
//! | summary | TEXT + STORED | 摘要（可搜索） |
//! | collection_id | STRING | 集合过滤 |
//! | domain | STRING | 领域过滤 |
//! | node_type | STRING | 类型过滤 |
//! | space_path_id | STRING | 空间树抽屉 ID 过滤 |
//! | knowledge_node_ids | STRING | 知识树节点 ID 数组（逗号分隔） |
//! | entity_ids | STRING | 实体 ID 数组（逗号分隔） |

use crate::sutra_library::models::MemoryNode;
use crate::sutra_library::recall::BaseHit;
use jieba_rs::Jieba;
use std::collections::HashSet;
use std::sync::RwLock;
use tantivy::collector::TopDocs;
use tantivy::query::BooleanQuery;
use tantivy::query::Occur;
use tantivy::query::TermQuery;
use tantivy::schema::{Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::tokenizer::*;
use tantivy::{doc, Index, IndexWriter, ReloadPolicy, TantivyDocument};

// ─── 自定义中文分词器 ───────────────────────────────────────

/// 基于 jieba-rs 的中文分词器
#[derive(Clone)]
pub struct JiebaTokenizer {
    jieba: Jieba,
}

impl Default for JiebaTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

impl JiebaTokenizer {
    pub fn new() -> Self {
        Self {
            jieba: Jieba::new(),
        }
    }
}

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = self
            .jieba
            .tokenize(text, jieba_rs::TokenizeMode::Search, true);
        let mut words = Vec::new();
        let mut offsets = Vec::new();
        for token in &tokens {
            // jieba-rs 0.10 的 Token 有 byte_start/byte_end（字节偏移）
            // 与 start/end（Unicode 字符索引）不同
            let word = &text[token.byte_start..token.byte_end];
            words.push(word.to_string());
            offsets.push((token.byte_start, token.byte_end));
        }
        JiebaTokenStream {
            words,
            offsets,
            pos: 0,
            token: Token::default(),
            _marker: std::marker::PhantomData,
        }
    }
}

pub struct JiebaTokenStream<'a> {
    words: Vec<String>,
    offsets: Vec<(usize, usize)>,
    pos: usize,
    token: Token,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> TokenStream for JiebaTokenStream<'a> {
    fn advance(&mut self) -> bool {
        if self.pos < self.words.len() {
            self.token.position = self.pos;
            self.token.offset_from = self.offsets[self.pos].0;
            self.token.offset_to = self.offsets[self.pos].1;
            self.token.text.clear();
            self.token.text.push_str(&self.words[self.pos]);
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.token
    }
}

// ─── 检索过滤器 ─────────────────────────────────────────────

/// 检索过滤器（布尔过滤条件）
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// 按集合过滤
    pub collection_id: Option<String>,
    /// 按领域过滤
    pub domain: Option<String>,
    /// 按节点类型过滤
    pub node_type: Option<String>,
    /// 按空间树抽屉 ID 过滤
    pub space_path_id: Option<String>,
    /// 按知识节点 ID 过滤（必须属于至少一个）
    pub knowledge_node_ids: Option<Vec<String>>,
    /// 按实体 ID 过滤（必须包含至少一个）
    pub entity_ids: Option<Vec<String>>,
}

// ─── Tantivy 索引引擎 ───────────────────────────────────────

/// Tantivy 全文检索引擎
pub struct TantivyIndex {
    index: Index,
    // 字段引用
    node_id: Field,
    title: Field,
    content: Field,
    summary: Field,
    collection_id: Field,
    domain: Field,
    node_type: Field,
    space_path_id: Field,
    knowledge_node_ids: Field,
    entity_ids: Field,
    // 写入器（RwLock 包装以支持并发）
    writer: RwLock<IndexWriter>,
}

impl TantivyIndex {
    /// 创建内存索引（默认，适合测试和轻量使用）
    pub fn new_in_ram() -> Self {
        let (schema, fields) = Self::build_schema();
        let index = Index::create_in_ram(schema.clone());
        Self::init_index(index, fields)
    }

    /// 创建磁盘持久化索引
    ///
    /// `dir_path`: 索引目录路径，不存在会自动创建
    pub fn new_in_dir<P: AsRef<std::path::Path>>(
        dir_path: P,
    ) -> Result<Self, tantivy::TantivyError> {
        let (schema, fields) = Self::build_schema();
        let index = Index::create_in_dir(dir_path, schema.clone())?;
        Ok(Self::init_index(index, fields))
    }

    /// 打开（或首次创建）磁盘索引 —— 落盘持久化的推荐入口
    ///
    /// 与 `new_in_dir` 的区别：目录已有索引时**复用**而不是报错重建，
    /// 因此进程重启后已沉淀的记忆仍可被检索到。
    ///
    /// 任何异常（权限不足、schema 不兼容、索引损坏）都会降级为内存索引并告警，
    /// 保证记忆引擎不会因为索引问题整体不可用。
    pub fn open_or_create_in_dir<P: AsRef<std::path::Path>>(dir_path: P) -> Self {
        let dir = dir_path.as_ref();
        if let Ok(index) = Index::open_in_dir(dir) {
            if let Some(fields) = Self::fields_from_schema(&index.schema()) {
                return Self::init_index(index, fields);
            }
            tracing::warn!(
                "TantivyIndex: 磁盘索引 schema 不兼容，重建索引目录 {:?}",
                dir
            );
            let _ = std::fs::remove_dir_all(dir);
        }
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::warn!("TantivyIndex: 无法创建索引目录 {:?}: {}", dir, e);
            return Self::new_in_ram();
        }
        match Self::new_in_dir(dir) {
            Ok(idx) => idx,
            Err(e) => {
                tracing::warn!("TantivyIndex: 磁盘索引创建失败，降级内存: {}", e);
                Self::new_in_ram()
            }
        }
    }

    /// 从已打开的 schema 中取回字段句柄；任一字段缺失返回 None（表示 schema 不兼容）
    fn fields_from_schema(schema: &Schema) -> Option<TantivyFields> {
        let get = |name: &str| schema.get_field(name).ok();
        Some(TantivyFields {
            node_id: get("node_id")?,
            title: get("title")?,
            content: get("content")?,
            summary: get("summary")?,
            collection_id: get("collection_id")?,
            domain: get("domain")?,
            node_type: get("node_type")?,
            space_path_id: get("space_path_id")?,
            knowledge_node_ids: get("knowledge_node_ids")?,
            entity_ids: get("entity_ids")?,
        })
    }

    /// 构建 Schema
    fn build_schema() -> (Schema, TantivyFields) {
        let mut builder = Schema::builder();

        // node_id: 精确匹配（不分词）+ 存储
        let node_id = builder.add_text_field(
            "node_id",
            TextOptions::default().set_stored().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // title: 全文检索（jieba 分词）+ 存储
        let title = builder.add_text_field(
            "title",
            TextOptions::default().set_stored().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("jieba")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            ),
        );

        // content: 全文检索（jieba 分词）
        let content = builder.add_text_field(
            "content",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("jieba")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            ),
        );

        // summary: 全文检索（jieba 分词）+ 存储
        let summary = builder.add_text_field(
            "summary",
            TextOptions::default().set_stored().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("jieba")
                    .set_index_option(IndexRecordOption::WithFreqsAndPositions),
            ),
        );

        // collection_id: 精确过滤（不分词）
        let collection_id = builder.add_text_field(
            "collection_id",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // domain: 精确过滤（不分词）
        let domain = builder.add_text_field(
            "domain",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // node_type: 精确过滤（不分词）
        let node_type = builder.add_text_field(
            "node_type",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // space_path_id: 精确过滤（不分词）
        let space_path_id = builder.add_text_field(
            "space_path_id",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // knowledge_node_ids: 精确匹配（逗号分隔）
        let knowledge_node_ids = builder.add_text_field(
            "knowledge_node_ids",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        // entity_ids: 精确匹配（逗号分隔）
        let entity_ids = builder.add_text_field(
            "entity_ids",
            TextOptions::default().set_indexing_options(
                TextFieldIndexing::default()
                    .set_tokenizer("raw")
                    .set_index_option(IndexRecordOption::Basic),
            ),
        );

        let schema = builder.build();
        (
            schema,
            TantivyFields {
                node_id,
                title,
                content,
                summary,
                collection_id,
                domain,
                node_type,
                space_path_id,
                knowledge_node_ids,
                entity_ids,
            },
        )
    }

    /// 初始化索引（注册分词器、创建写入器）
    fn init_index(index: Index, fields: TantivyFields) -> Self {
        // 注册 jieba 中文分词器（用于 TEXT 字段）
        let tokenizer = TextAnalyzer::from(JiebaTokenizer::new());
        index.tokenizers().register("jieba", tokenizer);

        // 注册 raw 分词器（不分词，用于 STRING 字段的精确匹配）
        let raw_tokenizer = TextAnalyzer::from(RawTokenizer::default());
        index.tokenizers().register("raw", raw_tokenizer);

        let writer = index
            .writer_with_num_threads(1, 32_000_000)
            .expect("TantivyIndex: 创建写入器失败");

        Self {
            index,
            node_id: fields.node_id,
            title: fields.title,
            content: fields.content,
            summary: fields.summary,
            collection_id: fields.collection_id,
            domain: fields.domain,
            node_type: fields.node_type,
            space_path_id: fields.space_path_id,
            knowledge_node_ids: fields.knowledge_node_ids,
            entity_ids: fields.entity_ids,
            writer: RwLock::new(writer),
        }
    }

    // ─── 索引写入 ───────────────────────────────────────────────

    /// 索引一个 MemoryNode
    ///
    /// 如果 node_id 已存在，会先删除旧文档再添加新文档（覆盖更新）。
    pub fn index_node(&self, node: &MemoryNode) {
        let knowledge_ids = Self::collect_knowledge_node_ids(node);
        let entity_ids_str = Self::collect_entity_ids(node);

        let tantivy_doc = doc!(
            self.node_id => node.node_id.as_str(),
            self.title => node.title.as_str(),
            self.content => node.content.as_str(),
            self.summary => node.summary.as_str(),
            self.collection_id => node.collection_id.as_str(),
            self.domain => node.domain.as_str(),
            self.node_type => node.node_type.as_str(),
            self.space_path_id => node.path.as_str(),
            self.knowledge_node_ids => knowledge_ids.as_str(),
            self.entity_ids => entity_ids_str.as_str(),
        );

        // 取读锁即可：tantivy 的 `IndexWriter::delete_term` / `add_document` 都是 `&self`
        // 且内部自带同步，本就支持多线程并发写入。此前用写锁会把所有索引写入
        // 串行化（clippy: this write lock is used only for reading）。
        // 真正需要独占的只有 `commit()`（需要 `&mut self`），见下方。
        let writer = self.writer.read().unwrap();
        // 先删除旧文档（幂等更新），再添加新文档
        let term = tantivy::Term::from_field_text(self.node_id, &node.node_id);
        let _ = writer.delete_term(term);
        let _ = writer.add_document(tantivy_doc);
    }

    /// 批量索引多个节点
    pub fn index_nodes(&self, nodes: &[MemoryNode]) {
        for node in nodes {
            self.index_node(node);
        }
    }

    /// 从索引中删除节点
    pub fn delete_node(&self, node_id: &str) {
        // 同 `index_node`：`delete_term` 只需 `&self`，读锁即可
        let writer = self.writer.read().unwrap();
        let term = tantivy::Term::from_field_text(self.node_id, node_id);
        let _ = writer.delete_term(term);
    }

    /// 提交索引（将内存缓冲区写入 segment）
    ///
    /// 这里必须用写锁：`IndexWriter::commit` 需要 `&mut self`，
    /// 且语义上应与并发的 `index_node` / `delete_node` 互斥。
    pub fn commit(&self) {
        let mut writer = self.writer.write().unwrap();
        let _ = writer.commit();
    }

    /// 清空全部文档（用于与持久化层全量对齐后的索引重建）
    ///
    /// 为什么需要：节点被删除后（清理脏数据、或数据目录被手工改动），磁盘索引
    /// 里仍留着已删节点的文档。检索会命中这些"幽灵文档"——`read_node` 读不到，
    /// 却照样占掉 top_k 名额，表现为"库里明明有数据却检索不到"
    /// （实测 top_k=1 时直接返回"未找到匹配的记忆"）。
    pub fn clear(&self) {
        let mut writer = self.writer.write().unwrap();
        if let Err(e) = writer.delete_all_documents() {
            tracing::warn!("TantivyIndex: 清空索引失败: {}", e);
            return;
        }
        if let Err(e) = writer.commit() {
            tracing::warn!("TantivyIndex: 清空索引后提交失败: {}", e);
        }
    }

    // ─── 检索查询 ───────────────────────────────────────────────

    /// 全文检索，返回 BM25 得分降序的 BaseHit 列表
    pub fn search(&self, query: &str, top_k: usize) -> Vec<BaseHit> {
        self.search_with_filters(query, &SearchFilters::default(), top_k)
    }

    /// 带布尔过滤的全文检索
    ///
    /// 支持同时按 collection_id / domain / node_type / space_path_id /
    /// knowledge_node_ids / entity_ids 进行 AND 过滤。
    pub fn search_with_filters(
        &self,
        query: &str,
        filters: &SearchFilters,
        top_k: usize,
    ) -> Vec<BaseHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }

        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .expect("TantivyIndex: 创建读取器失败");

        let searcher = reader.searcher();

        // 构建全文查询
        //
        // ⚠️ 不能直接用 `QueryParser::parse_query(整个句子)`：实测它会把整句当成一个
        // 近似精确匹配的查询，长问句（"我刚才说的渲染器和采样值是多少？"）
        // 几乎只能匹配到"用户当前这句话本身"，历史记忆一条都召回不到——
        // 表现就是"沉淀成功却依旧失忆"。
        //
        // 改为：jieba 分词 → 去停用词 → 各词在 title/content/summary 上做 OR（Should）。
        // 分词器与索引侧一致，保证 token 对得上；OR 语义让长句里的关键实体也能命中。
        let parsed_query = self.build_fulltext_query(query);

        // 构建过滤条件
        let mut subqueries: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
        subqueries.push((Occur::Must, parsed_query));

        // 添加过滤条件
        if let Some(ref cid) = filters.collection_id {
            let term = tantivy::Term::from_field_text(self.collection_id, cid);
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if let Some(ref d) = filters.domain {
            let term = tantivy::Term::from_field_text(self.domain, d);
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if let Some(ref nt) = filters.node_type {
            let term = tantivy::Term::from_field_text(self.node_type, nt);
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        if let Some(ref sp) = filters.space_path_id {
            let term = tantivy::Term::from_field_text(self.space_path_id, sp);
            subqueries.push((
                Occur::Must,
                Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
            ));
        }
        // knowledge_node_ids 过滤：必须包含至少一个指定 ID
        if let Some(ref kn_ids) = filters.knowledge_node_ids {
            let mut kn_subqueries: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
            for kn_id in kn_ids {
                let term = tantivy::Term::from_field_text(self.knowledge_node_ids, kn_id);
                kn_subqueries.push((
                    Occur::Should,
                    Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
                ));
            }
            if !kn_subqueries.is_empty() {
                subqueries.push((Occur::Must, Box::new(BooleanQuery::new(kn_subqueries))));
            }
        }
        // entity_ids 过滤：必须包含至少一个指定实体
        if let Some(ref e_ids) = filters.entity_ids {
            let mut e_subqueries: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
            for e_id in e_ids {
                let term = tantivy::Term::from_field_text(self.entity_ids, e_id);
                e_subqueries.push((
                    Occur::Should,
                    Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
                ));
            }
            if !e_subqueries.is_empty() {
                subqueries.push((Occur::Must, Box::new(BooleanQuery::new(e_subqueries))));
            }
        }

        let combined_query = BooleanQuery::new(subqueries);
        let top_docs = searcher
            .search(
                &combined_query,
                &TopDocs::with_limit(top_k).order_by_score(),
            )
            .unwrap_or_default();

        let mut hits: Vec<BaseHit> = top_docs
            .into_iter()
            .filter_map(|(score, doc_addr)| {
                let doc_ref = searcher.doc::<TantivyDocument>(doc_addr).ok()?;
                let chunk_id = doc_ref.get_first(self.node_id)?.as_str()?.to_string();
                Some(BaseHit {
                    chunk_id,
                    base_score: score,
                })
            })
            .collect();

        // 去重（按 chunk_id）
        let mut seen = HashSet::new();
        hits.retain(|h| seen.insert(h.chunk_id.clone()));

        hits
    }

    // ─── 工具方法 ───────────────────────────────────────────────

    /// 用 jieba 分词构造 OR 全文查询（与索引侧分词器一致）
    fn build_fulltext_query(&self, query: &str) -> Box<dyn tantivy::query::Query> {
        let jieba = Jieba::new();
        let mut clauses: Vec<(Occur, Box<dyn tantivy::query::Query>)> = Vec::new();

        // 与索引侧 `JiebaTokenizer` 保持一致：Search 模式 + hmm。
        // 分词模式不一致会导致 token 对不上（实测查 "采样值" 召回不到
        // 含 "采样值设为 128" 的节点）。
        let tokens = jieba.tokenize(query, jieba_rs::TokenizeMode::Search, true);
        let mut words: Vec<String> = Vec::new();
        for tk in &tokens {
            let w = &query[tk.byte_start..tk.byte_end];
            words.push(w.to_string());
            // 索引侧不做大小写归一，这里补一个小写变体，
            // 让 "Cycles" / "cycles" 两种写法都能命中（纯 ASCII 词才有意义）
            let lower = w.to_lowercase();
            if lower != w && w.chars().all(|c| c.is_ascii_alphanumeric()) {
                words.push(lower);
            }
        }

        for raw in words {
            let t = raw.trim();
            if Self::is_stop_word(t) {
                continue;
            }
            // 单字（"的""了"）噪音太大；但纯 ASCII 数字/英文单词保留（如 128、PNG）
            let is_ascii = t.chars().all(|c| c.is_ascii_alphanumeric());
            if t.chars().count() < 2 && !is_ascii {
                continue;
            }
            for field in [self.title, self.content, self.summary] {
                let term = tantivy::Term::from_field_text(field, &t);
                clauses.push((
                    Occur::Should,
                    Box::new(TermQuery::new(term, IndexRecordOption::Basic)),
                ));
            }
        }

        if clauses.is_empty() {
            // 全是停用词（如"你好吗"）：退化为全匹配，交由上层排序
            return Box::new(tantivy::query::AllQuery);
        }
        Box::new(BooleanQuery::new(clauses))
    }

    /// 查询侧停用词：疑问词、代词、助词等在记忆检索里没有区分度
    pub fn is_stop_word(t: &str) -> bool {
        const STOP: &[&str] = &[
            "我",
            "我们",
            "你",
            "你们",
            "他",
            "她",
            "它",
            "的",
            "了",
            "是",
            "在",
            "有",
            "和",
            "就",
            "不",
            "也",
            "都",
            "要",
            "会",
            "着",
            "过",
            "吗",
            "呢",
            "吧",
            "啊",
            "什么",
            "怎么",
            "怎样",
            "如何",
            "多少",
            "哪个",
            "哪些",
            "为什么",
            "请问",
            "刚",
            "刚才",
            "说",
            "问",
            "告诉",
            "一下",
            "这个",
            "那个",
            "可以",
            "能否",
            "能不能",
            "之前",
            "现在",
            "然后",
            "就是",
            "没有",
            "还是",
            "一个",
            "一下",
            "知道",
            "需要",
        ];
        STOP.contains(&t)
    }

    /// 从 MemoryNode 收集知识节点 ID（目前从 metadata 中提取）
    fn collect_knowledge_node_ids(node: &MemoryNode) -> String {
        node.metadata
            .get("knowledge_node_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default()
    }

    /// 从 MemoryNode 收集实体 ID
    fn collect_entity_ids(node: &MemoryNode) -> String {
        // 从 metadata 中提取 entity_ids
        let from_meta = node
            .metadata
            .get("entity_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        if !from_meta.is_empty() {
            return from_meta;
        }
        // 回退：使用 node_id
        format!("node:{}", node.node_id)
    }

    /// 获取索引中的文档总数
    pub fn num_docs(&self) -> usize {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .expect("TantivyIndex: 创建读取器失败");
        reader.searcher().num_docs() as usize
    }
}

// ─── 内部辅助结构 ───────────────────────────────────────────

struct TantivyFields {
    node_id: Field,
    title: Field,
    content: Field,
    summary: Field,
    collection_id: Field,
    domain: Field,
    node_type: Field,
    space_path_id: Field,
    knowledge_node_ids: Field,
    entity_ids: Field,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sutra_library::models::MemoryNode;

    fn make_node(id: &str, title: &str, content: &str, domain: &str) -> MemoryNode {
        MemoryNode {
            node_id: id.to_string(),
            collection_id: "test".to_string(),
            domain: domain.to_string(),
            node_type: "document".to_string(),
            content_hash: format!("hash_{}", id),
            parent_id: None,
            path: format!("/test/{}", title),
            depth: 0,
            sort_order: 0,
            title: title.to_string(),
            summary: format!("Summary of {}", title),
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
            last_accessed_at: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn test_index_and_search() {
        let index = TantivyIndex::new_in_ram();

        let node1 = make_node(
            "1",
            "阵列修改器",
            "阵列修改器 Count 参数控制重复数量",
            "blender",
        );
        let node2 = make_node(
            "2",
            "布尔修改器",
            "布尔修改器支持差集交集并集操作",
            "blender",
        );
        let node3 = make_node("3", "材质节点", "PBR 材质需要金属度和粗糙度贴图", "blender");

        index.index_node(&node1);
        index.index_node(&node2);
        index.index_node(&node3);
        index.commit();

        let hits = index.search("阵列修改器", 10);
        assert!(!hits.is_empty(), "应该找到阵列修改器");
        assert!(hits.iter().any(|h| h.chunk_id == "1"), "应该找到节点1");

        let hits = index.search("PBR 材质", 10);
        assert!(!hits.is_empty(), "应该找到 PBR 材质");
        assert!(hits.iter().any(|h| h.chunk_id == "3"), "应该找到节点3");
    }

    #[test]
    fn test_delete_and_reindex() {
        let index = TantivyIndex::new_in_ram();

        let node = make_node("1", "测试", "这是测试内容", "test");
        index.index_node(&node);
        index.commit();

        assert_eq!(index.num_docs(), 1, "应该有 1 个文档");

        // 删除
        index.delete_node("1");
        index.commit();

        // 重新索引同 ID 的文档
        let node2 = make_node("1", "新测试", "这是新内容", "test");
        index.index_node(&node2);
        index.commit();

        let hits = index.search("新内容", 10);
        assert!(!hits.is_empty(), "应该找到新内容");
    }

    #[test]
    fn test_filter_search() {
        let index = TantivyIndex::new_in_ram();

        let node1 = make_node("1", "Blender教程", "阵列修改器用法", "blender");
        let node2 = make_node("2", "Rust教程", "Rust 所有权系统", "rust");

        index.index_node(&node1);
        index.index_node(&node2);
        index.commit();

        let filters = SearchFilters {
            domain: Some("blender".to_string()),
            ..SearchFilters::default()
        };

        let hits = index.search_with_filters("教程", &filters, 10);
        assert_eq!(hits.len(), 1, "应该只找到 Blender 文档");
        assert_eq!(hits[0].chunk_id, "1");
    }

    #[test]
    fn test_empty_query() {
        let index = TantivyIndex::new_in_ram();
        let hits = index.search("", 10);
        assert!(hits.is_empty(), "空查询应该返回空结果");
    }
}

/// 磁盘索引回归测试
///
/// 锁两件事：
/// 1. 落盘索引重启后仍在（记忆不会随进程退出而蒸发）
/// 2. **长问句也能召回历史记忆** —— 曾经因为整句精确匹配，
///    长问句只命中"用户当前这句话本身"，历史记忆一条都召不回。
#[cfg(test)]
mod disk_roundtrip {
    use super::*;
    use crate::sutra_library::models::MemoryNode;

    fn node(id: &str, content: &str) -> MemoryNode {
        MemoryNode {
            node_id: id.to_string(),
            collection_id: "c1".to_string(),
            domain: "blender".to_string(),
            node_type: "session_message".to_string(),
            content_hash: format!("h{}", id),
            parent_id: None,
            path: "/p".to_string(),
            depth: 0,
            sort_order: 0,
            title: content.chars().take(20).collect(),
            summary: String::new(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            refs_out: Vec::new(),
            refs_in: Vec::new(),
            version_tag: "current".to_string(),
            snapshot_id: None,
            base_activation: 0.7,
            importance: 1,
            access_count: 0,
            feedback_score: 0.0,
            last_accessed_at: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn disk_index_survives_restart_and_recalls_by_long_question() {
        let dir = std::env::temp_dir().join(format!("tantivy_diag_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        {
            let idx = TantivyIndex::open_or_create_in_dir(&dir);
            idx.index_node(&node("n1", "用户项目固定使用 Cycles 渲染器"));
            idx.index_node(&node("n2", "采样值设为 128"));
            idx.commit();
            assert_eq!(idx.num_docs(), 2, "提交后应能读到 2 篇文档");
            let hits = idx.search("渲染器", 5);
            assert!(!hits.is_empty(), "磁盘索引应能检索到");
        }
        // 重新打开（模拟进程重启）
        {
            let idx = TantivyIndex::open_or_create_in_dir(&dir);
            assert_eq!(idx.num_docs(), 2, "重开后文档应仍在");
            for q in ["渲染器", "采样值", "Cycles", "128"] {
                assert!(!idx.search(q, 5).is_empty(), "重开后应能检索到 {:?}", q);
            }
            // 关键回归：完整问句（含代词/疑问词）必须能召回历史记忆
            let long_q = "我刚才说的渲染器和采样值是多少？";
            let hits = idx.search(long_q, 5);
            let ids: Vec<&str> = hits.iter().map(|h| h.chunk_id.as_str()).collect();
            assert!(
                ids.contains(&"n1") && ids.contains(&"n2"),
                "长问句必须召回两条历史记忆，实际: {:?}",
                ids
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
