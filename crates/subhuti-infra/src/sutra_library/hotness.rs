//! # 冷热记忆算法
//!
//! 综合激活分 = base_activation × 时间衰减 × 频率加成 × 重要性加成 × 反馈加成

use crate::sutra_library::models::MemoryNode;

/// 冷热记忆计算器
pub struct HotnessCalculator;

impl HotnessCalculator {
    /// 计算综合激活分
    ///
    /// - 时间衰减：指数衰减，每日自然遗忘
    /// - 频率加成：对数增长，避免过热
    /// - 重要性：永久固定权重
    /// - 反馈分：用户/Agent 训练自进化
    pub fn compute_activation(node: &MemoryNode, now_ts: i64) -> f32 {
        let base = node.base_activation;
        let time_decay = Self::time_decay(node.last_accessed_at, now_ts);
        let freq_bonus = Self::frequency_bonus(node.access_count);
        let importance_bonus = Self::importance_bonus(node.importance);
        let feedback_bonus = Self::feedback_bonus(node.feedback_score);

        let activation = base * time_decay * freq_bonus * importance_bonus * feedback_bonus;
        activation.clamp(0.0, 1.0)
    }

    /// 时间衰减：指数衰减，半衰期 7 天
    fn time_decay(last_accessed: i64, now: i64) -> f32 {
        let days = (now - last_accessed).max(0) as f32 / 86400.0;
        (-0.099 * days).exp() // 7 天半衰期: ln(2)/7 ≈ 0.099
    }

    /// 频率加成：对数增长，避免过热
    fn frequency_bonus(access_count: u32) -> f32 {
        if access_count == 0 {
            0.5
        } else {
            (1.0 + (access_count as f32).ln()).min(3.0)
        }
    }

    /// 重要性加成
    fn importance_bonus(importance: u8) -> f32 {
        match importance {
            0..=2 => 0.5,
            3..=5 => 1.0,
            6..=8 => 1.5,
            _ => 2.0,
        }
    }

    /// 反馈加成
    fn feedback_bonus(feedback_score: f32) -> f32 {
        // feedback_score 范围 [-1.0, 1.0]，映射到 [0.5, 1.5]
        1.0 + feedback_score * 0.5
    }

    /// 判断节点是否应视为"热"
    pub fn is_hot(node: &MemoryNode, now_ts: i64) -> bool {
        Self::compute_activation(node, now_ts) > 0.6
    }

    /// 判断节点是否应视为"冷"
    pub fn is_cold(node: &MemoryNode, now_ts: i64) -> bool {
        Self::compute_activation(node, now_ts) < 0.2
    }
}
