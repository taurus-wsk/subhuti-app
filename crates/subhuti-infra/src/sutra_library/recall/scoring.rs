//! # 分数归一化
//!
//! BM25、实体重合数、图谱 BFS 权重量纲不同，通过归一化使各通路分数可比。
//! 使用 rank-based 归一化，鲁棒性好，不受极端值影响。

use crate::sutra_library::recall::{Candidate, RetrieveSource};

/// 归一化策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NormalizeStrategy {
    /// 基于排名的归一化：score = (max_rank - rank) / max_rank
    #[default]
    RankBased,
    /// 不归一化（直接相加）
    None,
}

/// 通路可信度权重
///
/// 为什么需要：四个通路各自做 rank 归一化后，**每条通路的第一名都恒等于 1.0**。
/// 直接相加就等于把「全文检索的精确命中」和「图谱扩召回蹭到的邻居」当成同等
/// 证据。09-14 用真实 MCP 实测复现了后果：查询「色彩变换怎么处理」时，排第一的
/// 是一条仅因共享 2 字碎片「探针」而被 BFS 拉进来的无关记忆。
///
/// 权重体现的是「这条通路的分数说明了多少相关性」：
/// - Base（全文检索）：命中即文本相关，强证据 → 1.0
/// - Space（静态空间）：同域同层级，弱证据 → 0.5
/// - GraphFirstPass（与种子共享实体的邻居）：只证明"沾亲带故" → 0.3
/// - GraphSecondPass（把空间切片再投喂图谱）：连沾亲带故都隔了一层 → 0.15
const WEIGHT_BASE: f32 = 1.0;
const WEIGHT_SPACE: f32 = 0.5;
const WEIGHT_GRAPH1: f32 = 0.3;
const WEIGHT_GRAPH2: f32 = 0.15;

/// 对候选列表进行分数归一化
///
/// 将每个通路（Base、Space、GraphFirstPass、GraphSecondPass）的分数**分别**归一化，
/// 再乘各自通路权重后合并；最终总分仍为 `base_score + bonus_score`。
pub fn normalize_candidates(candidates: &mut [Candidate], strategy: NormalizeStrategy) {
    match strategy {
        NormalizeStrategy::None => {}
        NormalizeStrategy::RankBased => fuse_by_source(candidates),
    }
}

/// 各通路独立 rank 归一化 → 乘通路权重 → 取最大值合并
///
/// 与旧实现的两点差异：
/// 1. 引入通路权重，避免弱通路第一名（恒为 1.0）压过强通路的精确命中；
/// 2. 三条扩展通路共用 `bonus_score` 字段，旧实现顺序覆盖会丢掉先处理的通路
///    贡献；这里按通路各算一份再取 max，保证"多通路命中"不会丢证据。
fn fuse_by_source(candidates: &mut [Candidate]) {
    if candidates.is_empty() {
        return;
    }

    const PLAN: [(RetrieveSource, f32, bool); 4] = [
        (RetrieveSource::Base, WEIGHT_BASE, true),
        (RetrieveSource::Space, WEIGHT_SPACE, false),
        (RetrieveSource::GraphFirstPass, WEIGHT_GRAPH1, false),
        (RetrieveSource::GraphSecondPass, WEIGHT_GRAPH2, false),
    ];

    let mut base_out = vec![0f32; candidates.len()];
    let mut bonus_out = vec![0f32; candidates.len()];

    for (source, weight, is_base) in PLAN {
        // 收集属于该通路的候选原始分
        let mut entries: Vec<(usize, f32)> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.source_flags.contains(&source))
            .map(|(i, c)| (i, if is_base { c.base_score } else { c.bonus_score }))
            .collect();
        if entries.is_empty() {
            continue;
        }

        // rank-based：第 1 名 1.0，最后一名 0.0，再整体缩放到 [0, weight]
        entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let n = entries.len() as f32;
        for (rank, (idx, _)) in entries.iter().enumerate() {
            let norm = if n > 1.0 {
                (n - 1.0 - rank as f32) / (n - 1.0)
            } else {
                1.0
            };
            let v = norm * weight;
            let slot = if is_base {
                &mut base_out[*idx]
            } else {
                &mut bonus_out[*idx]
            };
            if v > *slot {
                *slot = v;
            }
        }
    }

    for (i, c) in candidates.iter_mut().enumerate() {
        c.base_score = base_out[i];
        c.bonus_score = bonus_out[i];
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

        normalize_candidates(&mut candidates, NormalizeStrategy::RankBased);

        // 归一化后: a=1.0, b=0.5, c=0.0
        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
        assert!((candidates[1].base_score - 0.5).abs() < 0.01);
        assert!((candidates[2].base_score - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_single_candidate_normalization() {
        let mut candidates = vec![make_candidate("a", 3.0, 0.0, vec![RetrieveSource::Base])];

        normalize_candidates(&mut candidates, NormalizeStrategy::RankBased);

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

        // a: base 第 1 名 → 1.0；space 第 1 名 → 0.5（通路权重），bonus 取 0.5
        // b: base 第 2 名 → 0.0
        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
        assert!((candidates[0].bonus_score - WEIGHT_SPACE).abs() < 0.01);
        assert!((candidates[1].base_score - 0.0).abs() < 0.01);
        assert!((candidates[1].bonus_score - 0.0).abs() < 0.01);
    }

    /// 核心回归：图谱扩召回不得压过 Base 精确命中。
    ///
    /// 复现 09-14 的排序倒挂——图谱通路第一名在旧实现里恒为 1.0，
    /// 与 Base 精确命中同分，甚至靠排序稳定性排到前面。
    #[test]
    fn graph_expansion_never_outranks_base_match() {
        let mut candidates = vec![
            make_candidate("exact", 9.0, 0.0, vec![RetrieveSource::Base]),
            make_candidate("noise", 0.0, 0.3, vec![RetrieveSource::GraphFirstPass]),
        ];

        normalize_candidates(&mut candidates, NormalizeStrategy::RankBased);

        assert!(
            final_score(&candidates[0]) > final_score(&candidates[1]),
            "Base 精确命中({}) 必须高于图谱扩召回({})",
            final_score(&candidates[0]),
            final_score(&candidates[1])
        );
    }

    /// 多通路命中不丢证据：同时被 Base 与图谱命中时，两份分数都要保留。
    #[test]
    fn multi_source_hit_keeps_both_signals() {
        let mut candidates = vec![
            make_candidate(
                "both",
                9.0,
                0.3,
                vec![RetrieveSource::Base, RetrieveSource::GraphFirstPass],
            ),
            make_candidate("base_only", 5.0, 0.0, vec![RetrieveSource::Base]),
        ];

        normalize_candidates(&mut candidates, NormalizeStrategy::RankBased);

        assert!((candidates[0].base_score - 1.0).abs() < 0.01);
        assert!((candidates[0].bonus_score - WEIGHT_GRAPH1).abs() < 0.01);
        assert!(final_score(&candidates[0]) > final_score(&candidates[1]));
    }

    /// 二阶图谱权重最低，单独命中时不该盖过一阶。
    #[test]
    fn second_pass_weight_is_lowest() {
        // 用数组滑窗而非直接两两比较常量：后者会被 clippy 判为
        // "assertion has a constant value"（编译期常量断言）。
        let weights = [WEIGHT_GRAPH2, WEIGHT_GRAPH1, WEIGHT_SPACE, WEIGHT_BASE];
        assert!(
            weights.windows(2).all(|w| w[0] < w[1]),
            "通路权重必须严格递增（二阶 < 一阶 < 空间 < 全文）: {weights:?}"
        );
    }
}
