//! # LLMScorer LLM 自省打分器
//!
//! 抽样评估召回切片对当前任务的相关度（软反馈）。
//!
//! 原理：每次拿到召回的 Top‑N 切片，交给 LLM 判断每条切片对当前任务的相关度，
//! 输出结构化 JSON。不阻塞主查询链路，作为后台异步任务运行。
//!
//! 优点：粒度细，可以评估没有被显式引用但是有帮助的切片
//! 缺点：消耗 token、有延迟，不能每条请求立刻跑，抽样/批量后台处理

use crate::sutra_library::feedback::data_models::RecallChunkInfo;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

// ─── LLM 自省打分结果 ───────────────────────────────────────

/// LLM 自省打分结果（单条切片）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRelevanceScore {
    /// 切片 ID
    pub chunk_id: String,
    /// 相关度得分 0.0~1.0
    pub relevant_score: f64,
    /// 是否被实际使用
    pub is_used: bool,
    /// 理由
    pub reason: String,
}

/// LLM 自省打分请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmScoringRequest {
    /// 原始 Query
    pub query: String,
    /// 需要评估的切片列表
    pub chunks: Vec<ScoringChunkInfo>,
}

/// 待评估的切片信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringChunkInfo {
    pub chunk_id: String,
    pub title: String,
    pub content: String,
    pub summary: String,
}

/// LLM 自省打分器
pub struct LlmScorer {
    /// 是否启用
    pub enabled: bool,
    /// 采样率（0.0 ~ 1.0）
    pub sample_rate: f64,
    /// 计数器（用于采样判断）
    counter: AtomicU64,
}

impl LlmScorer {
    /// 创建新的 LLM 打分器
    ///
    /// `sample_rate`: 采样率，0.0 表示关闭，1.0 表示每条都评估
    pub fn new(enabled: bool, sample_rate: f64) -> Self {
        Self {
            enabled,
            sample_rate: sample_rate.clamp(0.0, 1.0),
            counter: AtomicU64::new(0),
        }
    }

    /// 判断当前请求是否应该被采样
    pub fn should_sample(&self) -> bool {
        if !self.enabled || self.sample_rate <= 0.0 {
            return false;
        }
        if self.sample_rate >= 1.0 {
            return true;
        }

        // 基于计数器的采样：每 1/sample_rate 次采样一次
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let threshold = (1.0 / self.sample_rate) as u64;
        count % threshold == 0
    }

    /// 构建 LLM 评分 Prompt
    ///
    /// 返回格式化的 prompt 字符串，供外部 LLM 调用使用
    pub fn build_scoring_prompt(&self, chunks: &[RecallChunkInfo]) -> String {
        let mut prompt = String::new();
        prompt.push_str("请评估以下召回切片与用户查询的相关度。\n\n");
        prompt.push_str("请对每个切片输出 JSON 格式的评估结果：\n");
        prompt.push_str("{\n");
        prompt.push_str("  \"chunk_id\": \"切片ID\",\n");
        prompt.push_str("  \"relevant_score\": 0.0~1.0,\n");
        prompt.push_str("  \"is_used\": true/false,\n");
        prompt.push_str("  \"reason\": \"评估理由\"\n");
        prompt.push_str("}\n\n");
        prompt.push_str("待评估切片：\n");

        for (i, chunk) in chunks.iter().enumerate() {
            prompt.push_str(&format!(
                "{}. [{}] 得分={}\n",
                i + 1,
                chunk.chunk_id,
                chunk.score
            ));
        }

        prompt
    }

    /// 解析 LLM 返回的 JSON 评分结果
    pub fn parse_scoring_result(
        &self,
        _request: &LlmScoringRequest,
        json_str: &str,
    ) -> Vec<LlmRelevanceScore> {
        // 尝试解析为数组
        if let Ok(scores) = serde_json::from_str::<Vec<LlmRelevanceScore>>(json_str) {
            return scores;
        }

        // 尝试解析为单个对象
        if let Ok(score) = serde_json::from_str::<LlmRelevanceScore>(json_str) {
            return vec![score];
        }

        // 解析失败，返回空
        tracing::warn!("LLMScorer: 解析评分结果失败: {}", json_str);
        Vec::new()
    }

    /// 将 LLM 评分结果合并到 RecallChunkInfo
    pub fn apply_scores(
        &self,
        chunks: &[RecallChunkInfo],
        scores: &[LlmRelevanceScore],
    ) -> Vec<RecallChunkInfo> {
        let score_map: std::collections::HashMap<&str, &LlmRelevanceScore> =
            scores.iter().map(|s| (s.chunk_id.as_str(), s)).collect();

        chunks
            .iter()
            .map(|chunk| {
                let mut updated = chunk.clone();
                if let Some(score) = score_map.get(chunk.chunk_id.as_str()) {
                    updated.llm_relevance = Some(score.relevant_score as f32);
                    updated.llm_reason = Some(score.reason.clone());
                }
                updated
            })
            .collect()
    }
}

impl Default for LlmScorer {
    fn default() -> Self {
        Self::new(false, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sutra_library::recall::RetrieveSource;

    #[test]
    fn test_should_sample() {
        // 采样率 1.0 → 总是采样
        let scorer = LlmScorer::new(true, 1.0);
        assert!(scorer.should_sample());

        // 采样率 0.0 → 从不采样
        let scorer = LlmScorer::new(false, 0.0);
        assert!(!scorer.should_sample());

        // 采样率 0.5 → 大约一半概率采样
        let scorer = LlmScorer::new(true, 0.5);
        let mut sampled = 0;
        for _ in 0..100 {
            if scorer.should_sample() {
                sampled += 1;
            }
        }
        // 理论上每 2 次采样 1 次，100 次大约 50 次
        assert!(sampled > 0 && sampled < 100);
    }

    #[test]
    fn test_build_scoring_prompt() {
        let scorer = LlmScorer::new(true, 1.0);
        let chunks = vec![RecallChunkInfo {
            chunk_id: "chunk_1".to_string(),
            source: RetrieveSource::Base,
            score: 0.9,
            llm_relevance: None,
            llm_reason: None,
        }];
        let prompt = scorer.build_scoring_prompt(&chunks);
        assert!(prompt.contains("chunk_1"));
        assert!(prompt.contains("0.9"));
    }

    #[test]
    fn test_parse_scoring_result() {
        let scorer = LlmScorer::new(true, 1.0);
        let request = LlmScoringRequest {
            query: "test".to_string(),
            chunks: vec![],
        };

        let json = r#"[
            {"chunk_id": "chunk_1", "relevant_score": 0.8, "is_used": true, "reason": "相关"}
        ]"#;

        let scores = scorer.parse_scoring_result(&request, json);
        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].chunk_id, "chunk_1");
        assert!((scores[0].relevant_score - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_apply_scores() {
        let scorer = LlmScorer::new(true, 1.0);
        let chunks = vec![RecallChunkInfo {
            chunk_id: "chunk_1".to_string(),
            source: RetrieveSource::Base,
            score: 0.9,
            llm_relevance: None,
            llm_reason: None,
        }];

        let scores = vec![LlmRelevanceScore {
            chunk_id: "chunk_1".to_string(),
            relevant_score: 0.85,
            is_used: true,
            reason: "高度相关".to_string(),
        }];

        let updated = scorer.apply_scores(&chunks, &scores);
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].llm_relevance, Some(0.85));
        assert_eq!(updated[0].llm_reason, Some("高度相关".to_string()));
    }
}
