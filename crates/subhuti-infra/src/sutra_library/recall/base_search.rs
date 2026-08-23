//! # BaseSearch 基础检索
//!
//! 关键词匹配，产出原始种子切片 `Seed0`。
//! 使用 Tantivy 全文检索引擎替代旧版内存 BM25 + 关键词搜索。

use crate::sutra_library::models::MemoryNode;
use crate::sutra_library::recall::tantivy_index::{SearchFilters, TantivyIndex};
use crate::sutra_library::recall::BaseHit;
use crate::sutra_library::storage::MemoryStorage;
use std::sync::Arc;

/// 基础检索器
pub struct BaseSearch {
    memory: Arc<MemoryStorage>,
    tantivy: Option<Arc<TantivyIndex>>,
}

impl BaseSearch {
    pub fn new(memory: Arc<MemoryStorage>) -> Self {
        Self {
            memory,
            tantivy: None,
        }
    }

    /// 设置 Tantivy 索引（可选，不设置则回退到内存检索）
    pub fn with_tantivy(mut self, tantivy: Arc<TantivyIndex>) -> Self {
        self.tantivy = Some(tantivy);
        self
    }

    /// 执行基础检索，返回按得分降序的 BaseHit 列表
    ///
    /// 优先使用 Tantivy 全文检索（支持中文分词、原生 BM25），
    /// 回退到旧版内存 BM25 + 关键词搜索。
    pub fn search(&self, query: &str, top_k: usize) -> Vec<BaseHit> {
        if let Some(ref tantivy) = self.tantivy {
            tantivy.search(query, top_k)
        } else {
            self.search_memory(query, top_k)
        }
    }

    /// 带过滤条件的基础检索
    ///
    /// 仅 Tantivy 支持过滤条件，内存回退不支持过滤。
    pub fn search_with_filters(
        &self,
        query: &str,
        filters: &SearchFilters,
        top_k: usize,
    ) -> Vec<BaseHit> {
        if let Some(ref tantivy) = self.tantivy {
            tantivy.search_with_filters(query, filters, top_k)
        } else {
            self.search_memory(query, top_k)
        }
    }

    /// 回退到内存 BM25 + 关键词检索
    fn search_memory(&self, query: &str, top_k: usize) -> Vec<BaseHit> {
        let mut seen = std::collections::HashSet::new();
        let mut hits: Vec<BaseHit> = Vec::new();

        // 1. BM25 检索（主要结果）
        let bm25_results = self.memory.bm25_search(query, top_k * 2);
        for (node_id, bm25_score) in &bm25_results {
            if seen.insert(node_id.clone()) {
                hits.push(BaseHit {
                    chunk_id: node_id.clone(),
                    base_score: *bm25_score,
                });
            }
        }

        // 2. 关键词补充检索（未在 BM25 中命中的）
        let kw_results = self.memory.keyword_search(query, top_k);
        for node in kw_results {
            if seen.insert(node.node_id.clone()) {
                hits.push(BaseHit {
                    chunk_id: node.node_id,
                    base_score: 0.1, // 关键词命中给基础分
                });
            }
        }

        // 按得分降序排序
        hits.sort_by(|a, b| {
            b.base_score
                .partial_cmp(&a.base_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        hits.truncate(top_k);
        hits
    }

    /// 通过 PG tsvector 补充检索（如果配置了 PG）
    pub async fn search_pg(
        &self,
        query: &str,
        collection_id: Option<&str>,
        top_k: usize,
        pg: &crate::sutra_library::storage::PgStorage,
    ) -> Vec<BaseHit> {
        if let Ok(nodes) = pg.search_fts(query, collection_id, None, top_k).await {
            return nodes
                .into_iter()
                .map(|n| BaseHit {
                    chunk_id: n.node_id,
                    base_score: 0.5, // PG 命中给中等基础分
                })
                .collect();
        }
        Vec::new()
    }

    /// 将 BaseHit 解析为 MemoryNode 列表
    pub fn resolve_hits(&self, hits: &[BaseHit]) -> Vec<MemoryNode> {
        hits.iter()
            .filter_map(|h| self.memory.read_node(&h.chunk_id))
            .collect()
    }
}
