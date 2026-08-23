//! # 领域适配器标准契约
//!
//! 所有领域必须实现 `DomainParser` 和 `DomainTokenizer`，内核永久不变。

pub mod blender;
pub mod general;
pub mod rust;

use crate::sutra_library::models::*;
use anyhow::Result;

/// 领域解析器：语义切片、摘要、边缘提取、增量解析、格式化、压缩、重排序
pub trait DomainParser: Send + Sync {
    fn domain_name(&self) -> &str;
    fn split_semantic_chunks(&self, raw: &str, ctx: &ParseContext) -> Vec<SemanticChunk>;
    fn enrich_chunk(&self, chunk: &SemanticChunk) -> (String, serde_json::Value);
    fn extract_edges(&self, nodes: &[MemoryNode]) -> Vec<RefEdge>;
    fn incremental_parse(&self, old: &MemoryNode, diff: &str) -> Result<MemoryNode>;
    fn format_context(&self, node: &MemoryNode, mode: ContextMode) -> String;
    fn compress_nodes(&self, nodes: &[MemoryNode]) -> MemoryNode;
    fn rerank_nodes(&self, query: &str, candidates: &mut Vec<ScoredNode>);

    /// 提取实体关系对，用于写入 EntityGraph Manual 边
    ///
    /// 返回 (实体A, 实体B, 权重) 三元组，引擎在 write_node 时会自动
    /// 将这些关系对注册到 EntityGraph 的 adjacency 中。
    /// 默认实现返回空 vec，领域可根据知识库覆盖此方法。
    fn extract_entity_relations(&self, _content: &str) -> Vec<(String, String, f32)> {
        Vec::new()
    }
}

/// 领域分词器：分词、停用词、查询扩展
pub trait DomainTokenizer: Send + Sync {
    fn tokenize(&self, text: &str) -> Vec<String>;
    fn stop_words(&self) -> &[&str];
    fn expand_query(&self, query: &str) -> Vec<String>;
}

/// 领域注册中心
pub struct DomainRouter {
    parsers: Vec<Box<dyn DomainParser>>,
    tokenizers: Vec<Box<dyn DomainTokenizer>>,
}

impl DomainRouter {
    pub fn new() -> Self {
        Self {
            parsers: Vec::new(),
            tokenizers: Vec::new(),
        }
    }

    pub fn register(&mut self, parser: Box<dyn DomainParser>, tokenizer: Box<dyn DomainTokenizer>) {
        self.parsers.push(parser);
        self.tokenizers.push(tokenizer);
    }

    pub fn parser_for(&self, domain: &str) -> Option<&dyn DomainParser> {
        self.parsers
            .iter()
            .find(|p| p.domain_name() == domain)
            .map(|p| p.as_ref())
    }

    pub fn tokenizer_for(&self, domain: &str) -> Option<&dyn DomainTokenizer> {
        self.tokenizers
            .iter()
            .find(|_t| {
                // 简单匹配：tokenizer 通常与 parser 同 domain
                self.parsers
                    .iter()
                    .enumerate()
                    .any(|(i, p)| p.domain_name() == domain && i < self.tokenizers.len())
            })
            .map(|t| t.as_ref())
    }

    pub fn all_parsers(&self) -> &[Box<dyn DomainParser>] {
        &self.parsers
    }
}

impl Default for DomainRouter {
    fn default() -> Self {
        Self::new()
    }
}
