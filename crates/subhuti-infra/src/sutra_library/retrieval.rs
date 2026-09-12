//! # 三级检索调度器
//!
//! 临时记忆 → 热内存 → PG 冷库，逐级递进、去重合并、重排序。

use crate::sutra_library::domain::DomainRouter;
use crate::sutra_library::hotness::HotnessCalculator;
use crate::sutra_library::models::*;
use crate::sutra_library::persistence::PersistencePort;
use crate::sutra_library::storage::MemoryStorage;
use std::sync::Arc;

/// 检索调度器
pub struct RetrievalScheduler {
    memory: Arc<MemoryStorage>,
    persistence: Option<Arc<dyn PersistencePort>>,
    domain_router: Arc<DomainRouter>,
}

impl RetrievalScheduler {
    pub fn new(
        memory: Arc<MemoryStorage>,
        persistence: Option<Arc<dyn PersistencePort>>,
        domain_router: Arc<DomainRouter>,
    ) -> Self {
        Self {
            memory,
            persistence,
            domain_router,
        }
    }

    /// 三级检索入口
    pub async fn search(&self, query: &RetrievalQuery) -> RetrievalResult {
        let now_ts = chrono::Utc::now().timestamp();
        let mut all: Vec<ScoredNode> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // 1. 临时会话记忆（搜索所有会话）
        let all_sessions = self.memory.get_all_session_keys();
        for session_id in all_sessions {
            let session_nodes = self.memory.get_session_memory(&session_id);
            for node in session_nodes {
                let id = node.node_id.clone();
                if seen.insert(id) {
                    let scored = self.score_node(&node, query, now_ts);
                    all.push(scored);
                }
            }
        }

        // 2. 内存检索（BM25 + 关键词）
        // 所有节点都参与检索，热节点获得额外加分，冷节点也不排除
        let bm25_results = self.memory.bm25_search(&query.text, query.limit * 2);
        for (node_id, bm25_score) in &bm25_results {
            if let Some(node) = self.memory.read_node(node_id) {
                if seen.insert(node_id.clone()) {
                    let activation = HotnessCalculator::compute_activation(&node, now_ts);
                    let mut scored = self.score_node_with_bm25(&node, query, *bm25_score, now_ts);
                    if activation > 0.6 {
                        scored.score_detail.hotness_bonus = activation * 0.2;
                    }
                    scored.score = scored.score_detail.final_score;
                    all.push(scored);
                }
            }
        }

        // 补充关键词搜索（未在 BM25 中命中的）
        let kw_results = self.memory.keyword_search(&query.text, query.limit);
        for node in kw_results {
            if seen.insert(node.node_id.clone()) {
                let scored = self.score_node(&node, query, now_ts);
                all.push(scored);
            }
        }

        // 3. 持久化冷库（PG 或 SQLite，如果配置了）
        if let Some(pg) = &self.persistence {
            if let Ok(pg_nodes) = pg
                .search_fts(
                    &query.text,
                    query.collection_id.as_deref(),
                    query.domain.as_deref(),
                    query.limit,
                )
                .await
            {
                for node in pg_nodes {
                    if seen.insert(node.node_id.clone()) {
                        let activation = HotnessCalculator::compute_activation(&node, now_ts);
                        if activation <= 0.6 {
                            // 冷节点
                            let mut scored = self.score_node(&node, query, now_ts);
                            scored.score_detail.hotness_bonus = 0.0;
                            scored.score = scored.score_detail.final_score;
                            all.push(scored);
                        }
                    }
                }
            }
        }

        // 领域重排序
        if let Some(domain) = &query.domain {
            if let Some(parser) = self.domain_router.parser_for(domain) {
                parser.rerank_nodes(&query.text, &mut all);
            }
        }

        // 全局排序 + 截断
        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all.truncate(query.limit);

        let source = if all.is_empty() {
            RetrievalSource::ColdStorage
        } else if all.iter().any(|s| s.score_detail.hotness_bonus > 0.0) {
            RetrievalSource::HotMemory
        } else {
            RetrievalSource::ColdStorage
        };

        // 更新热度
        for scored in &all {
            if let Some(mut n) = self.memory.read_node(&scored.node.node_id) {
                n.access_count += 1;
                n.last_accessed_at = now_ts;
                n.base_activation = HotnessCalculator::compute_activation(&n, now_ts);
                self.memory.write_node(&n);
            }
        }

        RetrievalResult {
            total_candidates: all.len(),
            source,
            nodes: all,
        }
    }

    fn score_node(&self, node: &MemoryNode, query: &RetrievalQuery, now_ts: i64) -> ScoredNode {
        let bm25_score = 0.0;
        let activation = HotnessCalculator::compute_activation(node, now_ts);
        let tree_match = self.tree_match_score(node, &query.text);

        let detail = ScoreDetail {
            bm25_score,
            tree_match_score: tree_match,
            hotness_bonus: activation * 0.2,
            feedback_bonus: (1.0 + node.feedback_score * 0.5),
            final_score: 0.0,
        };
        let mut final_score = tree_match + detail.hotness_bonus + detail.feedback_bonus * 0.1;
        if final_score <= 0.0 {
            final_score = 0.01;
        }
        ScoredNode {
            node: node.clone(),
            score: final_score,
            score_detail: ScoreDetail {
                final_score,
                ..detail
            },
            matched_text: Vec::new(),
        }
    }

    fn score_node_with_bm25(
        &self,
        node: &MemoryNode,
        query: &RetrievalQuery,
        bm25_score: f32,
        now_ts: i64,
    ) -> ScoredNode {
        let activation = HotnessCalculator::compute_activation(node, now_ts);
        let tree_match = self.tree_match_score(node, &query.text);

        let detail = ScoreDetail {
            bm25_score,
            tree_match_score: tree_match,
            hotness_bonus: activation * 0.2,
            feedback_bonus: (1.0 + node.feedback_score * 0.5),
            final_score: 0.0,
        };
        let final_score = bm25_score * 0.5
            + tree_match * 0.3
            + detail.hotness_bonus
            + detail.feedback_bonus * 0.1;
        ScoredNode {
            node: node.clone(),
            score: final_score,
            score_detail: ScoreDetail {
                final_score,
                ..detail
            },
            matched_text: Vec::new(),
        }
    }

    /// 树结构匹配得分（路径、标题、父节点）
    fn tree_match_score(&self, node: &MemoryNode, query: &str) -> f32 {
        let q = query.to_lowercase();
        let mut score: f32 = 0.0;

        if node.path.to_lowercase().contains(&q) {
            score += 0.3;
        }
        if node.title.to_lowercase().contains(&q) {
            score += 0.4;
        }
        if node.summary.to_lowercase().contains(&q) {
            score += 0.2;
        }

        // 子节点匹配加分
        if let Some(_parser) = self.domain_router.parser_for(&node.domain) {
            let children = self.memory.find_children(&node.node_id);
            for child in &children {
                if child.title.to_lowercase().contains(&q) {
                    score += 0.1;
                }
            }
        }

        score.min(1.0)
    }
}
