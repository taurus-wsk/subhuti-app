//! # 分数归一化
//!
//! BM25、实体重合数、图谱 BFS 权重量纲不同，通过归一化使各通路分数可比。
//! 使用 rank-based 归一化，鲁棒性好，不受极端值影响。

use crate::sutra_library::recall::{Candidate, RetrieveSource};

/// 归一化策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeStrategy {
    /// 基于排名的归一化：score = (max_rank - rank) / max_rank
    RankBased,
    /// 不归一化（直接相加）
    None,
}

impl Default for NormalizeStrategy {
    fn default() -> Self {
        Self::RankBased
    }
}

/// 对候选列表进行分数归一化
///
/// 将每个通路（Base、Space、GraphFirstPass、GraphSecondPass）的分数分别归一化到 [0, 1]，
/// 然后再求和得到最终总分。
pub fn normalize_candidates(candidates: &mut [Candidate], strategy: NormalizeStrategy) {
    match strategy {
        NormalizeStrategy::None => {}
        NormalizeStrategy::RankBased => {
            normalize_by_source(candidates, RetrieveSource::Base, |c| &mut c.base_score);
            normalize_by_source(candidates, RetrieveSource::Space, |c| &mut c.bonus_score);
            normalize_by_source(candidates, RetrieveSource::GraphFirstPass, |c| {
                &mut c.bonus_score
            });
            normalize_by_source(candidates, RetrieveSource::GraphSecondPass, |c| {
                &mut c.bonus_score
            });
        }
    }
}

/// 按来源分组做 rank-based 归一化
fn normalize_by_source(
    candidates: &mut [Candidate],
    source: RetrieveSource,
    score_field: impl Fn(&mut Candidate) -> &mut f32,
) {
    // 收集属于该来源的候选索引和分数
    let mut entries: Vec<(usize, f32)> = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if c.source_flags.contains(&source) {
            entries.push((i, *score_field(&mut candidates[i].clone())));
        }
    }

    if entries.is_empty() {
        return;
    }

    // 按分数降序排序
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let max_rank = entries.len() as f32;
    for (rank, (idx, _)) in entries.iter().enumerate() {
        let normalized = if max_rank > 1.0 {
            (max_rank - 1.0 - rank as f32) / (max_rank - 1.0)
        } else {
            1.0
        };
        *score_field(&mut candidates[*idx]) = normalized;
    }
}

/// 计算候选的最终总分（归一化后的 base_score + bonus_score）
pub fn final_score(candidate: &Candidate) -> f32 {
    candidate.base_score + candidate.bonus_score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sutra_library::recall::RetrieveSource;

    fn make_candidate(
        chunk_id: &str,
        base: f32,
        bonus: f32,
        sources: Vec<RetrieveSource>,
    ) -> Candidate {
        Candidate {
            chunk_id: chunk_id.to_string(),
            base_score: base,
            bonus_score: bonus,
            source_flags: sources,
        }
    }

    #[test]
    fn test_rank_based_normalization() {
        let mut candidates = vec![
            make_candidate("a", 10.0, 0.0, vec![RetrieveSource::Base]),
            make_candidate("b", 5.0, 0.0, vec![RetrieveSource::Base]),
            make_candidate("c", 1.0, 0.0, vec![RetrieveSource::Base]),
        ];

        normalize_by_source(&mut candidates, RetrieveSource::Base, |c| &mut c.base_score);

        // 归一化后: a=1.0, b=0.5, c=0.0
        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
        assert!((candidates[1].base_score - 0.5).abs() < 0.01);
        assert!((candidates[2].base_score - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_single_candidate_normalization() {
        let mut candidates = vec![make_candidate("a", 3.0, 0.0, vec![RetrieveSource::Base])];

        normalize_by_source(&mut candidates, RetrieveSource::Base, |c| &mut c.base_score);

        // 单个候选，归一化后为 1.0
        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_mixed_sources() {
        let mut candidates = vec![
            Candidate {
                chunk_id: "a".to_string(),
                base_score: 10.0,
                bonus_score: 5.0,
                source_flags: vec![RetrieveSource::Base, RetrieveSource::Space],
            },
            Candidate {
                chunk_id: "b".to_string(),
                base_score: 5.0,
                bonus_score: 0.0,
                source_flags: vec![RetrieveSource::Base],
            },
        ];

        normalize_candidates(&mut candidates, NormalizeStrategy::RankBased);

        // a: base=1.0, bonus=1.0
        // b: base=0.0, bonus=0.0
        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
        assert!((candidates[0].bonus_score - 1.0).abs() < 0.01);
        assert!((candidates[1].base_score - 0.0).abs() < 0.01);
    }
}
