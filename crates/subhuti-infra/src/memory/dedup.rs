//! # Dedup 模块 - 近重复记忆检测与去除
//!
//! 使用向量（embedding）余弦相似度检测近似重复的记忆项，
//! 并根据保留策略选出最具信息量者保留，其余标记删除。
//!
//! ## 主要类型
//!
//! - [`DedupConfig`] - 去重配置（阈值、最短长度、保留策略）
//! - [`KeepStrategy`] - 保留策略枚举
//! - [`DedupResult`] - 去重结果统计
//! - [`Deduplicator`] - 去重器，提供相似度计算、重复检测与删除选择

use super::MemoryItem;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// 保留策略：当检测到重复时，决定保留哪一条记忆
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepStrategy {
    /// 保留内容最长者
    LongestContent,
    /// 保留最近创建者
    MostRecent,
    /// 保留元数据字段最多者
    RichestMetadata,
}

/// 去重配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupConfig {
    /// 相似度阈值（余弦相似度），默认 0.92
    pub similarity_threshold: f32,
    /// 最短内容长度，低于此长度的内容跳过检测，默认 10
    pub min_content_length: usize,
    /// 保留策略
    pub keep_strategy: KeepStrategy,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.92,
            min_content_length: 10,
            keep_strategy: KeepStrategy::LongestContent,
        }
    }
}

/// 去重结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupResult {
    /// 检查的记忆总数
    pub total_checked: usize,
    /// 发现的重复对数量
    pub duplicates_found: usize,
    /// 被标记删除的 ID 列表
    pub removed_ids: Vec<String>,
    /// 保留的 ID 列表
    pub kept_ids: Vec<String>,
}

/// 去重器
///
/// 基于向量余弦相似度检测近重复记忆，并按保留策略选择保留项。
#[derive(Debug, Clone)]
pub struct Deduplicator {
    /// 去重配置
    pub config: DedupConfig,
}

impl Deduplicator {
    /// 创建新的去重器
    pub fn new(config: DedupConfig) -> Self {
        Self { config }
    }

    /// 使用默认配置创建去重器
    pub fn with_defaults() -> Self {
        Self::new(DedupConfig::default())
    }

    /// 计算两个向量的余弦相似度
    ///
    /// 余弦相似度 = (a·b) / (|a| * |b|)，取值范围 [-1, 1]。
    ///
    /// 边界情况处理：
    /// - 任一向量为空 → 返回 0.0
    /// - 两向量长度不一致 → 返回 0.0（避免错误匹配）
    /// - 任一向量范数为 0 → 返回 0.0（避免除零）
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        debug!(
            "Deduplicator: cosine_similarity 开始，len_a={}, len_b={}",
            a.len(),
            b.len()
        );
        if a.is_empty() || b.is_empty() {
            debug!("Deduplicator: cosine_similarity 空向量，返回 0");
            return 0.0;
        }
        if a.len() != b.len() {
            warn!(
                "Deduplicator: cosine_similarity 向量长度不一致 {} != {}",
                a.len(),
                b.len()
            );
            return 0.0;
        }

        let mut dot = 0.0f32;
        let mut norm_a = 0.0f32;
        let mut norm_b = 0.0f32;

        for i in 0..a.len() {
            dot += a[i] * b[i];
            norm_a += a[i] * a[i];
            norm_b += b[i] * b[i];
        }

        let denom = norm_a.sqrt() * norm_b.sqrt();
        if denom == 0.0 {
            debug!("Deduplicator: cosine_similarity 范数为 0，返回 0");
            return 0.0;
        }

        let result = dot / denom;
        debug!("Deduplicator: cosine_similarity 完成，相似度={:.4}", result);
        result
    }

    /// 在记忆集合中查找重复对
    ///
    /// 遍历所有记忆两两组合，计算向量余弦相似度，
    /// 返回相似度达到阈值的 (id1, id2, similarity) 列表。
    ///
    /// - 跳过内容长度低于 `min_content_length` 的记忆
    /// - 跳过没有 embedding 的记忆
    pub fn find_duplicates(
        &self,
        memories: &[MemoryItem],
        embeddings: &HashMap<String, Vec<f32>>,
    ) -> Vec<(String, String, f32)> {
        info!(
            "Deduplicator: find_duplicates 开始，记忆数={}, embedding数={}, threshold={:.2}",
            memories.len(),
            embeddings.len(),
            self.config.similarity_threshold
        );
        let mut pairs: Vec<(String, String, f32)> = Vec::new();

        let candidates: Vec<&MemoryItem> = memories
            .iter()
            .filter(|m| m.content.chars().count() >= self.config.min_content_length)
            .filter(|m| embeddings.contains_key(&m.id))
            .collect();

        debug!(
            "Deduplicator: find_duplicates 预筛选后候选数={}, min_content_length={}",
            candidates.len(),
            self.config.min_content_length
        );

        let n = candidates.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let a = &candidates[i];
                let b = &candidates[j];

                let emb_a = embeddings.get(&a.id).unwrap();
                let emb_b = embeddings.get(&b.id).unwrap();

                let sim = Self::cosine_similarity(emb_a, emb_b);
                if sim >= self.config.similarity_threshold {
                    debug!(
                        "Deduplicator: find_duplicates 发现重复对 id1={}, id2={}, similarity={:.4}",
                        a.id, b.id, sim
                    );
                    pairs.push((a.id.clone(), b.id.clone(), sim));
                }
            }
        }

        info!(
            "Deduplicator: find_duplicates 完成，发现 {} 对重复",
            pairs.len()
        );
        pairs
    }

    /// 根据重复对和保留策略，选择需要删除的记忆 ID
    ///
    /// 使用连通分量算法处理传递性重复（A~B, B~C ⇒ A、B、C 同组），
    /// 每组按保留策略选出一个最优者保留，其余标记删除。
    /// 未涉及任何重复对的记忆直接保留。
    pub fn select_to_remove(
        &self,
        pairs: Vec<(String, String, f32)>,
        memories: &[MemoryItem],
    ) -> DedupResult {
        info!(
            "Deduplicator: select_to_remove 开始，重复对数={}, 记忆总数={}, 策略={:?}",
            pairs.len(),
            memories.len(),
            self.config.keep_strategy
        );
        let total_checked = memories.len();
        let duplicates_found = pairs.len();

        let lookup: HashMap<&String, &MemoryItem> = memories.iter().map(|m| (&m.id, m)).collect();

        let mut adj: HashMap<String, Vec<String>> = HashMap::new();
        for (id1, id2, _) in &pairs {
            adj.entry(id1.clone()).or_default().push(id2.clone());
            adj.entry(id2.clone()).or_default().push(id1.clone());
        }

        let mut kept_ids: Vec<String> = memories
            .iter()
            .filter(|m| !adj.contains_key(&m.id))
            .map(|m| m.id.clone())
            .collect();

        debug!(
            "Deduplicator: select_to_remove 非重复项直接保留 {} 条",
            kept_ids.len()
        );

        let mut removed_ids: Vec<String> = Vec::new();

        let nodes: Vec<String> = adj.keys().cloned().collect();
        let mut visited: HashMap<String, bool> = HashMap::new();

        for start in &nodes {
            if *visited.get(start).unwrap_or(&false) {
                continue;
            }

            let mut component: Vec<String> = Vec::new();
            let mut stack: Vec<String> = vec![start.clone()];
            visited.insert(start.clone(), true);

            while let Some(node) = stack.pop() {
                component.push(node.clone());
                if let Some(neighbors) = adj.get(&node) {
                    for nb in neighbors {
                        if !*visited.get(nb).unwrap_or(&false) {
                            visited.insert(nb.clone(), true);
                            stack.push(nb.clone());
                        }
                    }
                }
            }

            let keeper = self.select_keeper(&component, &lookup);
            debug!(
                "Deduplicator: select_to_remove 连通分量大小={}, 保留={}",
                component.len(),
                keeper
            );
            for id in &component {
                if id != &keeper {
                    removed_ids.push(id.clone());
                } else {
                    kept_ids.push(id.clone());
                }
            }
        }

        let result = DedupResult {
            total_checked,
            duplicates_found,
            removed_ids: removed_ids.clone(),
            kept_ids: kept_ids.clone(),
        };
        info!(
            "Deduplicator: select_to_remove 完成，检查={}, 发现重复={}, 删除={}, 保留={}",
            total_checked,
            duplicates_found,
            removed_ids.len(),
            kept_ids.len()
        );
        result
    }

    /// 从一组重复记忆中选出应保留者
    ///
    /// 根据保留策略计算每条记忆的得分，返回得分最高者的 ID。
    fn select_keeper<'a>(
        &self,
        component: &[String],
        lookup: &HashMap<&'a String, &'a MemoryItem>,
    ) -> String {
        debug!(
            "Deduplicator: select_keeper 开始，组大小={}, 策略={:?}",
            component.len(),
            self.config.keep_strategy
        );
        let mut best_id: Option<String> = None;
        let mut best_score: f64 = f64::MIN;

        for id in component {
            let item = match lookup.get(id) {
                Some(it) => *it,
                None => {
                    debug!("Deduplicator: select_keeper 未找到 id={}", id);
                    continue;
                }
            };

            let score = match self.config.keep_strategy {
                KeepStrategy::LongestContent => item.content.chars().count() as f64,
                KeepStrategy::MostRecent => item.created_at.timestamp_millis() as f64,
                KeepStrategy::RichestMetadata => item.metadata.len() as f64,
            };

            debug!("Deduplicator: select_keeper id={}, score={:.4}", id, score);

            if score > best_score {
                best_score = score;
                best_id = Some(id.clone());
            }
        }

        let result = best_id.unwrap_or_else(|| component.first().cloned().unwrap_or_default());
        debug!("Deduplicator: select_keeper 完成，保留={}", result);
        result
    }

    /// 检查给定内容是否与已有记忆重复
    ///
    /// 对内容进行分词后计算 Jaccard 相似度（文本相似度），
    /// 返回首个超过阈值的 (id, similarity)。
    ///
    /// 注意：此方法使用文本相似度作为快速预检，无需 embedding 服务；
    /// 若需基于向量的精确检测，请使用 [`Deduplicator::find_duplicates`]。
    pub fn check_duplicate(
        &self,
        content: &str,
        existing: &[(String, String, Vec<f32>)],
    ) -> Option<(String, f32)> {
        info!(
            "Deduplicator: check_duplicate 开始，内容长度={}, 已有记忆数={}",
            content.len(),
            existing.len()
        );
        if content.chars().count() < self.config.min_content_length {
            debug!(
                "Deduplicator: check_duplicate 内容过短，跳过，min_content_length={}",
                self.config.min_content_length
            );
            return None;
        }

        let tokens = Self::tokenize(content);
        debug!("Deduplicator: check_duplicate 分词结果数={}", tokens.len());

        for (id, existing_content, _embedding) in existing {
            let existing_tokens = Self::tokenize(existing_content);
            let sim = Self::jaccard_similarity(&tokens, &existing_tokens);
            if sim >= self.config.similarity_threshold {
                info!(
                    "Deduplicator: check_duplicate 发现重复 id={}, similarity={:.4}",
                    id, sim
                );
                return Some((id.clone(), sim));
            }
        }

        debug!("Deduplicator: check_duplicate 未发现重复");
        None
    }

    /// 简单分词：按非字母数字字符切分并转小写
    fn tokenize(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    }

    /// 计算 Jaccard 相似度 = |A∩B| / |A∪B|
    ///
    /// 两个空集的相似度定义为 1.0（完全相同）；
    /// 一空一非空的相似度为 0.0。
    fn jaccard_similarity(a: &[String], b: &[String]) -> f32 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }

        let set_a: std::collections::HashSet<&String> = a.iter().collect();
        let set_b: std::collections::HashSet<&String> = b.iter().collect();

        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();

        if union == 0 {
            return 0.0;
        }

        intersection as f32 / union as f32
    }
}

impl Default for Deduplicator {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    /// 辅助函数：创建测试用记忆项
    fn make_item(id: &str, content: &str, created_at: chrono::DateTime<Utc>) -> MemoryItem {
        let mut item = MemoryItem::default();
        item.id = id.to_string();
        item.content = content.to_string();
        item.created_at = created_at;
        item
    }

    // ── cosine_similarity 测试 ──────────────────────────

    #[test]
    fn test_cosine_similarity_identical() {
        // 相同向量相似度应为 1.0
        let a = vec![1.0, 2.0, 3.0];
        let sim = Deduplicator::cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        // 正交向量相似度应为 0
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = Deduplicator::cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-5);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        // 空向量返回 0
        let sim = Deduplicator::cosine_similarity(&[], &[]);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_zero_norm() {
        // 零向量范数为 0，返回 0 避免除零
        let a = vec![0.0, 0.0, 0.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = Deduplicator::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_length() {
        // 长度不一致返回 0
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = Deduplicator::cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_known_value() {
        // a = [1,0], b = [1,1] => cos = 1/√2 ≈ 0.7071
        let a = vec![1.0, 0.0];
        let b = vec![1.0, 1.0];
        let sim = Deduplicator::cosine_similarity(&a, &b);
        assert!((sim - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
    }

    // ── find_duplicates 测试 ────────────────────────────

    #[test]
    fn test_find_duplicates_basic() {
        // m1 与 m2 embedding 相同，应被检测为重复
        let now = Utc::now();
        let m1 = make_item("m1", "这是一段测试内容用于去重检测", now);
        let m2 = make_item("m2", "这是一段测试内容用于去重检测", now);
        let m3 = make_item("m3", "完全不同的内容在这里出现", now);
        let memories = vec![m1, m2, m3];

        let mut embeddings = HashMap::new();
        embeddings.insert("m1".to_string(), vec![1.0, 0.0, 0.0]);
        embeddings.insert("m2".to_string(), vec![1.0, 0.0, 0.0]); // 与 m1 相同
        embeddings.insert("m3".to_string(), vec![0.0, 1.0, 0.0]); // 与 m1 正交

        let dedup = Deduplicator::with_defaults();
        let pairs = dedup.find_duplicates(&memories, &embeddings);

        assert_eq!(pairs.len(), 1);
        let (id1, id2, sim) = &pairs[0];
        assert!(id1 == "m1" && id2 == "m2" || id1 == "m2" && id2 == "m1");
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_find_duplicates_short_content_skipped() {
        // 内容过短应被跳过
        let now = Utc::now();
        let m1 = make_item("m1", "短", now);
        let m2 = make_item("m2", "短", now);
        let memories = vec![m1, m2];

        let mut embeddings = HashMap::new();
        embeddings.insert("m1".to_string(), vec![1.0, 0.0]);
        embeddings.insert("m2".to_string(), vec![1.0, 0.0]);

        let dedup = Deduplicator::with_defaults();
        let pairs = dedup.find_duplicates(&memories, &embeddings);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_find_duplicates_missing_embedding_skipped() {
        // 缺少 embedding 的记忆应被跳过
        let now = Utc::now();
        let m1 = make_item("m1", "这是一段测试内容用于去重检测", now);
        let m2 = make_item("m2", "这是一段测试内容用于去重检测", now);
        let memories = vec![m1, m2];

        let mut embeddings = HashMap::new();
        embeddings.insert("m1".to_string(), vec![1.0, 0.0]);
        // m2 没有 embedding

        let dedup = Deduplicator::with_defaults();
        let pairs = dedup.find_duplicates(&memories, &embeddings);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_find_duplicates_below_threshold() {
        // 相似度低于阈值不应被报告
        let now = Utc::now();
        let m1 = make_item("m1", "这是一段测试内容用于去重检测", now);
        let m2 = make_item("m2", "这是一段测试内容用于去重检测", now);
        let memories = vec![m1, m2];

        let mut embeddings = HashMap::new();
        // cos = 1/√2 ≈ 0.707 < 0.92
        embeddings.insert("m1".to_string(), vec![1.0, 0.0]);
        embeddings.insert("m2".to_string(), vec![1.0, 1.0]);

        let dedup = Deduplicator::with_defaults();
        let pairs = dedup.find_duplicates(&memories, &embeddings);
        assert!(pairs.is_empty());
    }

    // ── select_to_remove / KeepStrategy 测试 ───────────

    #[test]
    fn test_select_to_remove_longest_content() {
        // 策略：保留最长内容 → 保留 m2，删除 m1
        let now = Utc::now();
        let m1 = make_item("m1", "短内容", now);
        let m2 = make_item("m2", "这是一段更长的内容用于测试", now);
        let memories = vec![m1, m2];

        let pairs = vec![("m1".to_string(), "m2".to_string(), 0.95)];

        let config = DedupConfig {
            keep_strategy: KeepStrategy::LongestContent,
            ..DedupConfig::default()
        };
        let dedup = Deduplicator::new(config);
        let result = dedup.select_to_remove(pairs, &memories);

        assert_eq!(result.total_checked, 2);
        assert_eq!(result.duplicates_found, 1);
        assert!(result.removed_ids.contains(&"m1".to_string()));
        assert!(result.kept_ids.contains(&"m2".to_string()));
    }

    #[test]
    fn test_select_to_remove_most_recent() {
        // 策略：保留最近 → 保留 m2（更晚创建），删除 m1
        let old = Utc::now();
        let new = old + chrono::Duration::seconds(100);

        let m1 = make_item("m1", "内容相同长度", old);
        let m2 = make_item("m2", "内容相同长度", new);
        let memories = vec![m1, m2];

        let pairs = vec![("m1".to_string(), "m2".to_string(), 0.95)];

        let config = DedupConfig {
            keep_strategy: KeepStrategy::MostRecent,
            ..DedupConfig::default()
        };
        let dedup = Deduplicator::new(config);
        let result = dedup.select_to_remove(pairs, &memories);

        assert!(result.removed_ids.contains(&"m1".to_string()));
        assert!(result.kept_ids.contains(&"m2".to_string()));
    }

    #[test]
    fn test_select_to_remove_richest_metadata() {
        // 策略：保留元数据最多 → 保留 m1，删除 m2
        let now = Utc::now();
        let mut m1 = make_item("m1", "内容相同长度", now);
        m1.metadata.insert("key1".to_string(), "v1".to_string());
        let m2 = make_item("m2", "内容相同长度", now); // 无 metadata

        let memories = vec![m1, m2];
        let pairs = vec![("m1".to_string(), "m2".to_string(), 0.95)];

        let config = DedupConfig {
            keep_strategy: KeepStrategy::RichestMetadata,
            ..DedupConfig::default()
        };
        let dedup = Deduplicator::new(config);
        let result = dedup.select_to_remove(pairs, &memories);

        assert!(result.removed_ids.contains(&"m2".to_string()));
        assert!(result.kept_ids.contains(&"m1".to_string()));
    }

    #[test]
    fn test_select_to_remove_transitive_duplicates() {
        // 传递性重复：A~B, B~C ⇒ A、B、C 同组，只保留一个
        let now = Utc::now();
        let m_a = make_item("a", "短", now);
        let m_b = make_item("b", "中等长度内容", now);
        let m_c = make_item("c", "最长的一段内容在这里出现", now);
        let memories = vec![m_a, m_b, m_c];

        let pairs = vec![
            ("a".to_string(), "b".to_string(), 0.95),
            ("b".to_string(), "c".to_string(), 0.93),
        ];

        let dedup = Deduplicator::with_defaults(); // LongestContent
        let result = dedup.select_to_remove(pairs, &memories);

        // 应保留内容最长的 c，删除 a 和 b
        assert_eq!(result.removed_ids.len(), 2);
        assert!(result.kept_ids.contains(&"c".to_string()));
        assert!(result.removed_ids.contains(&"a".to_string()));
        assert!(result.removed_ids.contains(&"b".to_string()));
    }

    #[test]
    fn test_select_to_remove_no_duplicates() {
        // 无重复对时全部保留
        let now = Utc::now();
        let m1 = make_item("m1", "内容一", now);
        let m2 = make_item("m2", "内容二", now);
        let memories = vec![m1, m2];

        let dedup = Deduplicator::with_defaults();
        let result = dedup.select_to_remove(vec![], &memories);

        assert_eq!(result.duplicates_found, 0);
        assert!(result.removed_ids.is_empty());
        assert_eq!(result.kept_ids.len(), 2);
    }

    // ── check_duplicate 测试 ───────────────────────────

    #[test]
    fn test_check_duplicate_found() {
        // 完全相同文本应被检测为重复
        let dedup = Deduplicator::with_defaults();
        let existing: Vec<(String, String, Vec<f32>)> = vec![(
            "e1".to_string(),
            "这是一段测试内容用于去重检测".to_string(),
            vec![1.0, 0.0],
        )];

        let result = dedup.check_duplicate("这是一段测试内容用于去重检测", &existing);
        assert!(result.is_some());
        let (id, sim) = result.unwrap();
        assert_eq!(id, "e1");
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_check_duplicate_not_found() {
        // 完全不同文本不应被判定为重复
        let dedup = Deduplicator::with_defaults();
        let existing: Vec<(String, String, Vec<f32>)> = vec![(
            "e1".to_string(),
            "完全不同的内容在这里出现".to_string(),
            vec![1.0, 0.0],
        )];

        let result = dedup.check_duplicate("这是一段测试内容用于去重检测", &existing);
        assert!(result.is_none());
    }

    #[test]
    fn test_check_duplicate_short_content() {
        // 内容过短直接返回 None
        let dedup = Deduplicator::with_defaults();
        let existing: Vec<(String, String, Vec<f32>)> = vec![];

        let result = dedup.check_duplicate("短", &existing);
        assert!(result.is_none());
    }

    #[test]
    fn test_check_duplicate_english_tokens() {
        // 英文分词场景：相同单词集合应被判定为重复
        let dedup = Deduplicator::with_defaults();
        let existing: Vec<(String, String, Vec<f32>)> = vec![(
            "e1".to_string(),
            "the quick brown fox jumps".to_string(),
            vec![1.0, 0.0],
        )];

        // 相同单词，顺序不同 → Jaccard = 1.0
        let result = dedup.check_duplicate("fox jumps the quick brown", &existing);
        assert!(result.is_some());
        let (id, sim) = result.unwrap();
        assert_eq!(id, "e1");
        assert!((sim - 1.0).abs() < 1e-5);
    }

    // ── 配置与序列化测试 ───────────────────────────────

    #[test]
    fn test_dedup_config_default() {
        let config = DedupConfig::default();
        assert!((config.similarity_threshold - 0.92).abs() < 1e-5);
        assert_eq!(config.min_content_length, 10);
        assert_eq!(config.keep_strategy, KeepStrategy::LongestContent);
    }

    #[test]
    fn test_keep_strategy_serde() {
        // 验证 snake_case 序列化
        let strategy = KeepStrategy::MostRecent;
        let json = serde_json::to_string(&strategy).unwrap();
        assert_eq!(json, "\"most_recent\"");
        let de: KeepStrategy = serde_json::from_str(&json).unwrap();
        assert_eq!(de, KeepStrategy::MostRecent);
    }

    #[test]
    fn test_dedup_config_serde() {
        let config = DedupConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let de: DedupConfig = serde_json::from_str(&json).unwrap();
        assert!((de.similarity_threshold - 0.92).abs() < 1e-5);
        assert_eq!(de.min_content_length, 10);
        assert_eq!(de.keep_strategy, KeepStrategy::LongestContent);
    }

    #[test]
    fn test_dedup_result_serde() {
        let result = DedupResult {
            total_checked: 10,
            duplicates_found: 3,
            removed_ids: vec!["a".to_string(), "b".to_string()],
            kept_ids: vec!["c".to_string()],
        };
        let json = serde_json::to_string(&result).unwrap();
        let de: DedupResult = serde_json::from_str(&json).unwrap();
        assert_eq!(de.total_checked, 10);
        assert_eq!(de.duplicates_found, 3);
        assert_eq!(de.removed_ids.len(), 2);
        assert_eq!(de.kept_ids.len(), 1);
    }
}
