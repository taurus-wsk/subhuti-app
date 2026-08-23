//! # library_retrieve 五阶段召回流水线
//!
//! 阶段1: 基础检索（含查询扩展），拿到原始种子
//! 阶段2: 并行一阶扩展通路（空间 + 图谱）
//! 阶段3: 可选二阶图谱扩展（空间切片投喂图谱）
//! 阶段4: 合并所有扩展结果到候选池
//! 阶段5: 全局打分、归一化、排序

use crate::sutra_library::recall::base_search::BaseSearch;
use crate::sutra_library::recall::config::LibraryRetrieveConfig;
use crate::sutra_library::recall::graph::{GraphExpandConfig, GraphPassageStrategy};
use crate::sutra_library::recall::query::QueryAnalyzer;
use crate::sutra_library::recall::scoring::{self, NormalizeStrategy};
use crate::sutra_library::recall::space::SpaceDepthStrategy;
use crate::sutra_library::recall::{
    Candidate, ChunkUuid, EntityUuid, GraphHit, RetrieveSource, SpaceHit,
};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};

/// 默认查询分析器（全局懒加载，支持领域同义词注册）
///
/// 返回 `&'static RwLock<QueryAnalyzer>`，外部可通过 `analyzer.write().unwrap().dict_mut()`
/// 注册领域特定同义词，对后续所有检索请求生效。
pub fn default_query_analyzer() -> &'static RwLock<QueryAnalyzer> {
    static ANALYZER: OnceLock<RwLock<QueryAnalyzer>> = OnceLock::new();
    ANALYZER.get_or_init(|| RwLock::new(QueryAnalyzer::new()))
}

/// 召回结果（包含候选项和种子信息，供后续学习任务使用）
pub struct RetrieveResult {
    pub candidates: Vec<Candidate>,
    pub seed_chunks: Vec<ChunkUuid>,
    pub seed_entities: HashSet<EntityUuid>,
}

/// 藏经阁召回主入口
///
/// `query`: 用户检索输入
/// `config`: 召回配置
/// `query_analyzer`: 查询分析器（可使用 `default_query_analyzer()` 获取全局默认）
pub async fn library_retrieve(
    base_search: &BaseSearch,
    space_strategy: &SpaceDepthStrategy,
    graph_strategy: &GraphPassageStrategy,
    query: &str,
    config: &LibraryRetrieveConfig,
    query_analyzer: &RwLock<QueryAnalyzer>,
) -> RetrieveResult {
    // 查询分析（读锁，不影响并发注册同义词）
    let analyzed = query_analyzer.read().unwrap().analyze(query);
    let expansions = analyzed.expansions;

    // ==================== 阶段1：基础检索（含查询扩展） ====================
    // 对所有扩展查询做检索，合并结果
    let base_hits = {
        let mut all_hits = base_search.search(&expansions[0], config.base_top_k);
        for exp in &expansions[1..] {
            let extra = base_search.search(exp, config.base_top_k);
            for hit in extra {
                // 合并，取最大 base_score
                if let Some(existing) = all_hits.iter_mut().find(|h| h.chunk_id == hit.chunk_id) {
                    existing.base_score = existing.base_score.max(hit.base_score);
                } else {
                    all_hits.push(hit);
                }
            }
        }
        all_hits.sort_by(|a, b| {
            b.base_score
                .partial_cmp(&a.base_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_hits.truncate(config.base_top_k);
        all_hits
    };
    let seed0: Vec<ChunkUuid> = base_hits.iter().map(|h| h.chunk_id.clone()).collect();

    // 提取全局固定根实体集合，空间排序全程只使用这一份实体
    // 使用图谱索引提取（参考《藏经阁召回引擎·高层整合终极方案》）
    let root_entity_set: HashSet<EntityUuid> = graph_strategy.get_entities_of_chunks(&seed0);

    // 候选池 Map: chunk_id -> Candidate
    let mut candidate_pool: HashMap<ChunkUuid, Candidate> = HashMap::new();

    // 1.1 将基础检索结果写入候选池
    for hit in base_hits {
        candidate_pool.insert(
            hit.chunk_id.clone(),
            Candidate {
                chunk_id: hit.chunk_id,
                base_score: hit.base_score,
                bonus_score: 0.0,
                source_flags: vec![RetrieveSource::Base],
            },
        );
    }

    // ==================== 阶段2：并行一阶扩展通路 ====================

    // 通路A：静态空间召回
    let space_hits: Vec<SpaceHit> = if config.space_enable {
        space_strategy.expand(
            &seed0,
            &root_entity_set,
            config.space_max_tree_distance,
            config.space_per_level_limit,
        )
    } else {
        Vec::new()
    };

    // 通路B：图谱一阶 BFS（原始种子），分数倍率 = 1.0
    let graph_cfg = GraphExpandConfig {
        max_depth: config.graph_max_depth,
        max_result: config.graph_max_global_result,
        decay_factor: config.graph_decay_factor,
        enable_manual_edge: config.enable_manual_edge,
        enable_learned_edge: config.enable_learned_edge,
        learned_scale: config.learned_edge_scale,
    };
    let graph1_hits: Vec<GraphHit> = graph_strategy.expand(&seed0, &graph_cfg, 1.0);

    // ==================== 阶段3：可选二阶图谱扩展（空间切片投喂图谱） ====================
    let graph2_hits: Vec<GraphHit> =
        if config.graph_second_pass_from_space && !space_hits.is_empty() {
            let space_chunk_seeds: Vec<ChunkUuid> =
                space_hits.iter().map(|x| x.chunk_id.clone()).collect();
            graph_strategy.expand(
                &space_chunk_seeds,
                &graph_cfg,
                config.second_pass_score_decay,
            )
        } else {
            Vec::new()
        };

    // ==================== 阶段4：合并所有扩展结果到候选池 ====================

    // 4.1 合并空间通路
    for sh in space_hits {
        candidate_pool
            .entry(sh.chunk_id.clone())
            .and_modify(|c| {
                c.bonus_score = c.bonus_score.max(sh.bonus_score);
                if !c.source_flags.contains(&RetrieveSource::Space) {
                    c.source_flags.push(RetrieveSource::Space);
                }
            })
            .or_insert(Candidate {
                chunk_id: sh.chunk_id,
                base_score: 0.0,
                bonus_score: sh.bonus_score,
                source_flags: vec![RetrieveSource::Space],
            });
    }

    // 4.2 合并一阶图谱
    for gh in graph1_hits {
        candidate_pool
            .entry(gh.chunk_id.clone())
            .and_modify(|c| {
                c.bonus_score = c.bonus_score.max(gh.bonus_score);
                if !c.source_flags.contains(&RetrieveSource::GraphFirstPass) {
                    c.source_flags.push(RetrieveSource::GraphFirstPass);
                }
            })
            .or_insert(Candidate {
                chunk_id: gh.chunk_id,
                base_score: 0.0,
                bonus_score: gh.bonus_score,
                source_flags: vec![RetrieveSource::GraphFirstPass],
            });
    }

    // 4.3 合并二阶图谱
    for gh in graph2_hits {
        candidate_pool
            .entry(gh.chunk_id.clone())
            .and_modify(|c| {
                c.bonus_score = c.bonus_score.max(gh.bonus_score);
                if !c.source_flags.contains(&RetrieveSource::GraphSecondPass) {
                    c.source_flags.push(RetrieveSource::GraphSecondPass);
                }
            })
            .or_insert(Candidate {
                chunk_id: gh.chunk_id,
                base_score: 0.0,
                bonus_score: gh.bonus_score,
                source_flags: vec![RetrieveSource::GraphSecondPass],
            });
    }

    // ==================== 阶段5：全局打分、归一化、排序 ====================
    let mut list: Vec<Candidate> = candidate_pool.into_values().collect();

    // 5.1 分数归一化：按通路 rank-based 归一化，消除量纲差异
    scoring::normalize_candidates(&mut list, NormalizeStrategy::RankBased);

    // 5.2 排序（总分 = base_score + bonus_score）
    list.sort_by(|a, b| {
        let sa = scoring::final_score(a);
        let sb = scoring::final_score(b);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    RetrieveResult {
        candidates: list,
        seed_chunks: seed0,
        seed_entities: root_entity_set,
    }
}
