//! # HybridSearch - 混合搜索引擎
//!
//! 将 BM25 关键词匹配与向量语义相似度融合，提升搜索质量。
//!
//! ## 核心思路
//!
//! - **BM25**：基于词频与逆文档频率的经典关键词打分算法，擅长精确词项命中
//! - **语义相似度**：基于向量余弦相似度，擅长捕捉语义近似但字面不同的结果
//! - **加权融合**：`final_score = w_semantic * semantic_score + w_keyword * keyword_score`
//!
//! ## BM25 公式
//!
//! 对查询中的每个词项 `t`：
//! ```text
//! score(t) = IDF(t) * (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * doc_len / avg_doc_len))
//! ```
//! 其中：
//! - `IDF(t) = ln(1 + (N - df + 0.5) / (df + 0.5))`
//! - `N` 为文档总数，`df` 为包含词项 `t` 的文档数
//! - `tf` 为词项 `t` 在文档中的出现频次
//! - `doc_len` 为当前文档长度，`avg_doc_len` 为平均文档长度
//! - `k1` 控制词频饱和速度（默认 1.5），`b` 控制文档长度归一化强度（默认 0.75）

use super::storage::MemoryRow;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// 混合搜索结果
///
/// 融合语义相似度与关键词 BM25 分数后的统一结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSearchResult {
    /// 文本内容
    pub content: String,
    /// 语义相似度分数（来自向量检索）
    pub semantic_score: f32,
    /// 关键词匹配分数（来自 BM25）
    pub keyword_score: f32,
    /// 融合后的最终分数（加权求和）
    pub final_score: f32,
    /// 记忆层（short_term / archive / knowledge）
    pub layer: String,
    /// 角色（user / assistant）
    pub role: String,
    /// 创建时间
    pub created_at: DateTime<Utc>,
}

/// 混合搜索器
///
/// 融合 BM25 关键词匹配与向量语义相似度，输出统一的排序结果。
#[derive(Debug, Clone)]
pub struct HybridSearcher {
    /// 语义权重（默认 0.6）
    pub semantic_weight: f32,
    /// 关键词权重（默认 0.4）
    pub keyword_weight: f32,
    /// BM25 参数 k1，控制词频饱和速度（默认 1.5）
    pub bm25_k1: f32,
    /// BM25 参数 b，控制文档长度归一化强度（默认 0.75）
    pub bm25_b: f32,
}

impl Default for HybridSearcher {
    fn default() -> Self {
        Self {
            semantic_weight: 0.6,
            keyword_weight: 0.4,
            bm25_k1: 1.5,
            bm25_b: 0.75,
        }
    }
}

impl HybridSearcher {
    /// 创建新的混合搜索器
    pub fn new() -> Self {
        Self::default()
    }

    /// 使用自定义权重和 BM25 参数构建搜索器
    pub fn with_params(semantic_weight: f32, keyword_weight: f32, k1: f32, b: f32) -> Self {
        Self {
            semantic_weight,
            keyword_weight,
            bm25_k1: k1,
            bm25_b: b,
        }
    }

    /// 分词：小写化后按非字母数字字符切分（unicode 感知），保留长度 >= 2 的词项
    ///
    /// # 示例
    /// ```
    /// use subhuti::memory::hybrid_search::HybridSearcher;
    /// let tokens = HybridSearcher::tokenize("Hello, world! 你好世界");
    /// assert!(tokens.contains(&"hello".to_string()));
    /// assert!(tokens.contains(&"world".to_string()));
    /// ```
    pub fn tokenize(text: &str) -> Vec<String> {
        let result: Vec<String> = text
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.chars().count() >= 2)
            .map(|s| s.to_string())
            .collect();
        debug!(
            "HybridSearch: tokenize text_len={}, tokens={:?}",
            text.len(),
            result
        );
        result
    }

    /// 计算单个文档相对于查询的 BM25 分数
    ///
    /// # 参数
    /// - `query`: 查询文本
    /// - `document`: 文档文本
    /// - `k1`: 词频饱和参数
    /// - `b`: 文档长度归一化参数
    /// - `avg_doc_len`: 文档集合的平均文档长度（以 token 计）
    /// - `doc_len`: 当前文档长度（以 token 计）
    /// - `doc_freq`: 词项 -> 包含该词项的文档数 的映射
    /// - `total_docs`: 文档总数
    #[allow(clippy::too_many_arguments)]
    pub fn bm25_score(
        query: &str,
        document: &str,
        k1: f32,
        b: f32,
        avg_doc_len: f32,
        doc_len: f32,
        doc_freq: &HashMap<String, usize>,
        total_docs: usize,
    ) -> f32 {
        let query_terms = Self::tokenize(query);
        let doc_tokens = Self::tokenize(document);

        // 统计文档中每个词项的出现频次
        let mut doc_term_freq: HashMap<String, usize> = HashMap::new();
        for token in &doc_tokens {
            *doc_term_freq.entry(token.clone()).or_insert(0) += 1;
        }

        // 平均文档长度不能为 0，避免除零
        let avg_len = if avg_doc_len > 0.0 { avg_doc_len } else { 1.0 };
        // 当前文档长度不能为 0，避免后续除零
        let dl = if doc_len > 0.0 { doc_len } else { 1.0 };

        let mut score = 0.0_f32;
        for term in query_terms {
            let tf = *doc_term_freq.get(&term).unwrap_or(&0) as f32;
            if tf == 0.0 {
                continue;
            }

            let df = *doc_freq.get(&term).unwrap_or(&0) as f32;
            // IDF = ln(1 + (N - df + 0.5) / (df + 0.5))
            let idf = (1.0 + (total_docs as f32 - df + 0.5) / (df + 0.5)).ln();

            // BM25 词项得分：IDF * (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * doc_len / avg_doc_len))
            let denominator = tf + k1 * (1.0 - b + b * dl / avg_len);
            if denominator > 0.0 {
                score += idf * (tf * (k1 + 1.0)) / denominator;
            }
        }

        score
    }

    /// 计算一组文档相对于查询的 BM25 分数
    ///
    /// 自动统计文档频率、平均文档长度等统计量。
    ///
    /// # 参数
    /// - `query`: 查询文本
    /// - `documents`: 文档集合
    ///
    /// # 返回
    /// 与 `documents` 等长、按顺序对应的 BM25 分数列表
    pub fn compute_bm25_scores(&self, query: &str, documents: &[String]) -> Vec<f32> {
        info!(
            "HybridSearch: compute_bm25_scores query={}, documents={}",
            query,
            documents.len()
        );
        let n = documents.len();
        if n == 0 {
            debug!("HybridSearch: compute_bm25_scores 空文档集合");
            return Vec::new();
        }

        let docs_tokens: Vec<Vec<String>> = documents.iter().map(|d| Self::tokenize(d)).collect();

        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for tokens in &docs_tokens {
            let unique: std::collections::HashSet<&String> = tokens.iter().collect();
            for token in unique {
                *doc_freq.entry(token.clone()).or_insert(0) += 1;
            }
        }

        let total_len: usize = docs_tokens.iter().map(|t| t.len()).sum();
        let avg_doc_len = (total_len as f32) / (n as f32);

        debug!(
            "HybridSearch: compute_bm25_scores doc_freq_size={}, avg_doc_len={:.2}",
            doc_freq.len(),
            avg_doc_len
        );

        let scores: Vec<f32> = docs_tokens
            .iter()
            .enumerate()
            .map(|(i, tokens)| {
                let doc_len = tokens.len() as f32;
                Self::bm25_score(
                    query,
                    &documents[i],
                    self.bm25_k1,
                    self.bm25_b,
                    avg_doc_len,
                    doc_len,
                    &doc_freq,
                    n,
                )
            })
            .collect();

        info!(
            "HybridSearch: compute_bm25_scores 完成，最高分数={:.4}, 平均分数={:.4}",
            scores.iter().cloned().fold(0.0, f32::max),
            scores.iter().sum::<f32>() / scores.len() as f32
        );

        scores
    }

    /// 融合语义检索与关键词检索结果
    ///
    /// - 按 `MemoryRow.id` 匹配两路结果
    /// - 仅语义命中：`keyword_score = 0`
    /// - 仅关键词命中：`semantic_score = 0`
    /// - `final_score = semantic_weight * semantic_score + keyword_weight * keyword_score`
    /// - 结果按 `final_score` 降序排列
    ///
    /// # 参数
    /// - `semantic`: 语义检索结果（MemoryRow, 相似度分数）
    /// - `keyword`: 关键词检索结果（MemoryRow, BM25 分数）
    /// - `query`: 原始查询文本（保留以备扩展使用）
    pub fn fuse_results(
        &self,
        semantic: Vec<(MemoryRow, f32)>,
        keyword: Vec<(MemoryRow, f32)>,
        query: &str,
    ) -> Vec<HybridSearchResult> {
        info!(
            "HybridSearch: fuse_results semantic_count={}, keyword_count={}, query={}",
            semantic.len(),
            keyword.len(),
            query
        );

        let mut merged: HashMap<i32, (Option<f32>, Option<f32>, MemoryRow)> = HashMap::new();

        debug!("HybridSearch: fuse_results 开始合并语义结果");
        for (row, score) in semantic {
            let id = row.id;
            let entry = merged
                .entry(id)
                .or_insert_with(|| (None, None, row.clone()));
            entry.0 = Some(score);
            entry.2 = row;
            debug!(
                "HybridSearch: fuse_results 语义匹配 id={}, score={:.4}",
                id, score
            );
        }

        debug!("HybridSearch: fuse_results 开始合并关键词结果");
        for (row, score) in keyword {
            let id = row.id;
            let entry = merged
                .entry(id)
                .or_insert_with(|| (None, None, row.clone()));
            entry.1 = Some(score);
            if entry.0.is_none() {
                entry.2 = row;
            }
            debug!(
                "HybridSearch: fuse_results 关键词匹配 id={}, score={:.4}",
                id, score
            );
        }

        debug!(
            "HybridSearch: fuse_results 合并完成，去重后总数={}",
            merged.len()
        );

        let mut results: Vec<HybridSearchResult> = merged
            .into_iter()
            .map(|(_, (sem, kw, row))| {
                let semantic_score = sem.unwrap_or(0.0);
                let keyword_score = kw.unwrap_or(0.0);
                let final_score =
                    self.semantic_weight * semantic_score + self.keyword_weight * keyword_score;
                HybridSearchResult {
                    content: row.content,
                    semantic_score,
                    keyword_score,
                    final_score,
                    layer: row.layer,
                    role: row.role,
                    created_at: row.created_at,
                }
            })
            .collect();

        debug!("HybridSearch: fuse_results 开始按 final_score 降序排序");
        results.sort_by(|a, b| {
            b.final_score
                .partial_cmp(&a.final_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        debug!("HybridSearch: fuse_results 排序完成");

        info!(
            "HybridSearch: fuse_results 完成，融合结果数={}, semantic_weight={}, keyword_weight={}",
            results.len(),
            self.semantic_weight,
            self.keyword_weight
        );
        if !results.is_empty() {
            debug!(
                "HybridSearch: fuse_results top result final_score={:.4}, semantic={:.4}, keyword={:.4}",
                results[0].final_score, results[0].semantic_score, results[0].keyword_score
            );
        }

        results
    }
}

impl HybridSearcher {
    /// 便捷构造：仅指定权重
    pub fn with_weights(semantic_weight: f32, keyword_weight: f32) -> Self {
        Self {
            semantic_weight,
            keyword_weight,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// 构造测试用的 MemoryRow
    fn make_row(id: i32, content: &str, layer: &str, role: &str) -> MemoryRow {
        MemoryRow {
            id,
            user_id: "default".to_string(),
            session_id: Some("s1".to_string()),
            role: role.to_string(),
            content: content.to_string(),
            metadata: serde_json::json!({}),
            layer: layer.to_string(),
            embedding: None,
            created_at: Utc::now(),
        }
    }

    // ==================== 分词测试 ====================

    #[test]
    fn test_tokenize_basic_english() {
        let tokens = HybridSearcher::tokenize("Hello, world! This is a test.");
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
        assert!(tokens.contains(&"this".to_string()));
        assert!(tokens.contains(&"test".to_string()));
        // 单字符词应被过滤（min 2 chars）
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"is".to_string()) == false); // "is" 长度为 2，保留
    }

    #[test]
    fn test_tokenize_filters_single_char() {
        let tokens = HybridSearcher::tokenize("a b c de fg");
        // "a", "b", "c" 长度为 1，被过滤
        assert!(!tokens.contains(&"a".to_string()));
        assert!(!tokens.contains(&"b".to_string()));
        assert!(!tokens.contains(&"c".to_string()));
        // "de", "fg" 长度为 2，保留
        assert!(tokens.contains(&"de".to_string()));
        assert!(tokens.contains(&"fg".to_string()));
    }

    #[test]
    fn test_tokenize_unicode_chinese() {
        // 中文连续字符应作为整体 token 保留（unicode 感知）
        let tokens = HybridSearcher::tokenize("你好世界 Rust编程");
        assert!(tokens.contains(&"你好世界".to_string()));
        assert!(tokens.contains(&"rust编程".to_string()));
    }

    #[test]
    fn test_tokenize_lowercase() {
        let tokens = HybridSearcher::tokenize("Rust RUST rust");
        assert!(tokens.iter().all(|t| t == "rust"));
        assert_eq!(tokens.len(), 3);
    }

    #[test]
    fn test_tokenize_empty_and_punctuation() {
        assert!(HybridSearcher::tokenize("").is_empty());
        assert!(HybridSearcher::tokenize("!!! ,,, ...").is_empty());
    }

    // ==================== BM25 打分测试 ====================

    #[test]
    fn test_bm25_score_basic() {
        // 构造文档频率表：词项 "rust" 出现在 2 篇文档中
        let mut doc_freq = HashMap::new();
        doc_freq.insert("rust".to_string(), 2);
        doc_freq.insert("language".to_string(), 3);
        doc_freq.insert("fast".to_string(), 1);

        let score = HybridSearcher::bm25_score(
            "rust language",
            "rust is a fast language",
            1.5,
            0.75,
            5.0,
            5.0,
            &doc_freq,
            10,
        );

        // 分数应大于 0（有词项命中）
        assert!(score > 0.0, "BM25 score should be positive, got {}", score);
    }

    #[test]
    fn test_bm25_score_no_match() {
        let doc_freq = HashMap::new();
        let score = HybridSearcher::bm25_score(
            "python",
            "rust is a fast language",
            1.5,
            0.75,
            5.0,
            5.0,
            &doc_freq,
            10,
        );
        // 没有词项命中，分数应为 0
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_bm25_score_rare_term_higher_than_common() {
        // 罕见词（df 小）的 IDF 更高，得分应更高
        let mut doc_freq = HashMap::new();
        doc_freq.insert("rare".to_string(), 1); // 罕见词
        doc_freq.insert("common".to_string(), 100); // 常见词

        let total = 101;
        let score_rare = HybridSearcher::bm25_score(
            "rare",
            "rare term document",
            1.5,
            0.75,
            3.0,
            3.0,
            &doc_freq,
            total,
        );
        let score_common = HybridSearcher::bm25_score(
            "common",
            "common term document",
            1.5,
            0.75,
            3.0,
            3.0,
            &doc_freq,
            total,
        );

        assert!(
            score_rare > score_common,
            "rare term should score higher: rare={} common={}",
            score_rare,
            score_common
        );
    }

    #[test]
    fn test_compute_bm25_scores_length_matches_documents() {
        let searcher = HybridSearcher::new();
        let documents = vec![
            "rust is fast".to_string(),
            "python is slow".to_string(),
            "rust and python are languages".to_string(),
        ];
        let scores = searcher.compute_bm25_scores("rust", &documents);
        assert_eq!(scores.len(), documents.len());
    }

    #[test]
    fn test_compute_bm25_scores_relevant_doc_scores_higher() {
        let searcher = HybridSearcher::new();
        let documents = vec![
            "rust is a fast programming language".to_string(), // 包含 rust
            "the weather is nice today".to_string(),           // 无关
        ];
        let scores = searcher.compute_bm25_scores("rust", &documents);
        assert!(
            scores[0] > scores[1],
            "relevant doc should score higher: doc0={} doc1={}",
            scores[0],
            scores[1]
        );
        // 无关文档分数应为 0
        assert_eq!(scores[1], 0.0);
    }

    #[test]
    fn test_compute_bm25_scores_empty_documents() {
        let searcher = HybridSearcher::new();
        let scores = searcher.compute_bm25_scores("query", &[]);
        assert!(scores.is_empty());
    }

    // ==================== 融合测试 ====================

    #[test]
    fn test_fuse_results_semantic_only() {
        let searcher = HybridSearcher::new();
        let row = make_row(1, "hello world", "short_term", "user");

        let results = searcher.fuse_results(vec![(row, 0.9)], vec![], "hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].semantic_score, 0.9);
        assert_eq!(results[0].keyword_score, 0.0);
        // final_score = 0.6 * 0.9 + 0.4 * 0 = 0.54
        assert!((results[0].final_score - 0.54).abs() < 1e-5);
    }

    #[test]
    fn test_fuse_results_keyword_only() {
        let searcher = HybridSearcher::new();
        let row = make_row(1, "hello world", "short_term", "user");

        let results = searcher.fuse_results(vec![], vec![(row, 2.0)], "hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].semantic_score, 0.0);
        assert_eq!(results[0].keyword_score, 2.0);
        // final_score = 0.6 * 0 + 0.4 * 2.0 = 0.8
        assert!((results[0].final_score - 0.8).abs() < 1e-5);
    }

    #[test]
    fn test_fuse_results_both_match() {
        let searcher = HybridSearcher::new();
        let row_sem = make_row(1, "hello world", "short_term", "user");
        let row_kw = make_row(1, "hello world", "short_term", "user");

        let results = searcher.fuse_results(vec![(row_sem, 0.8)], vec![(row_kw, 1.5)], "hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].semantic_score, 0.8);
        assert_eq!(results[0].keyword_score, 1.5);
        // final_score = 0.6 * 0.8 + 0.4 * 1.5 = 0.48 + 0.6 = 1.08
        assert!((results[0].final_score - 1.08).abs() < 1e-5);
    }

    #[test]
    fn test_fuse_results_sorted_by_final_score_desc() {
        let searcher = HybridSearcher::new();
        // doc1: semantic=0.9, keyword=0 -> final=0.54
        // doc2: semantic=0.0, keyword=2.0 -> final=0.8
        // doc3: semantic=0.8, keyword=1.5 -> final=1.08
        let semantic = vec![
            (make_row(1, "doc1", "short_term", "user"), 0.9),
            (make_row(3, "doc3", "short_term", "user"), 0.8),
        ];
        let keyword = vec![
            (make_row(2, "doc2", "short_term", "user"), 2.0),
            (make_row(3, "doc3", "short_term", "user"), 1.5),
        ];

        let results = searcher.fuse_results(semantic, keyword, "doc");
        assert_eq!(results.len(), 3);
        // 降序：doc3(1.08) > doc2(0.8) > doc1(0.54)
        assert!(results[0].final_score >= results[1].final_score);
        assert!(results[1].final_score >= results[2].final_score);
        assert!((results[0].final_score - 1.08).abs() < 1e-5);
        assert!((results[1].final_score - 0.8).abs() < 1e-5);
        assert!((results[2].final_score - 0.54).abs() < 1e-5);
    }

    #[test]
    fn test_fuse_results_empty_inputs() {
        let searcher = HybridSearcher::new();
        let results = searcher.fuse_results(vec![], vec![], "query");
        assert!(results.is_empty());
    }

    #[test]
    fn test_fuse_results_preserves_row_fields() {
        let searcher = HybridSearcher::new();
        let row = make_row(42, "custom content", "archive", "assistant");

        let results = searcher.fuse_results(vec![(row, 0.5)], vec![], "content");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "custom content");
        assert_eq!(results[0].layer, "archive");
        assert_eq!(results[0].role, "assistant");
    }

    #[test]
    fn test_with_custom_weights() {
        let searcher = HybridSearcher::with_weights(0.8, 0.2);
        let row = make_row(1, "test", "short_term", "user");

        let results = searcher.fuse_results(vec![(row, 1.0)], vec![], "test");
        // final_score = 0.8 * 1.0 + 0.2 * 0 = 0.8
        assert!((results[0].final_score - 0.8).abs() < 1e-5);
    }
}
