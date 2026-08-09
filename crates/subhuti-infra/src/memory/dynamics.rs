//! # 记忆连接动力学 - Memory Connection Dynamics
//!
//! 纯数学模块：基于神经科学研究的记忆连接强度计算。
//! 无 I/O、无数据库、无网络，仅操作普通结构体。
//!
//! ## 三大核心机制
//!
//! 1. **赫布式增强 (Hebbian Potentiation)**: 两个记忆被共同访问时，连接强度增长。
//!    神经科学基础：Hebb (1949) "一起激发的神经元会连接在一起" (Cells that fire together,
//!    wire together)。重复共激活会强化突触连接，对应 LTP (长时程增强) 现象。
//!
//! 2. **艾宾浩斯指数衰减 (Ebbinghaus Exponential Decay)**: 自上次激活以来随时间衰减。
//!    神经科学基础：Ebbinghaus (1885) 遗忘曲线，记忆强度随时间呈指数下降。
//!    稳定性 (stability) 越高，衰减越慢。
//!
//! 3. **Cepeda 间隔效应 (Spacing Effect)**: 间隔式强化比集中式强化更能提升稳定性。
//!    神经科学基础：Cepeda et al. (2006) 间隔学习比集中学习更有效，间隔重复使
//!    记忆更稳固，对应突触巩固的间隔依赖性。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ── 常量 ──────────────────────────────────────────────

/// 强度下限：连接永远不会衰减到这个值以下
pub const STRENGTH_FLOOR: f32 = 0.05;

/// 强度上限：连接强度的最大值
pub const MAX_STRENGTH: f32 = 5.0;

/// 默认稳定性
pub const DEFAULT_STABILITY: f32 = 1.0;

/// 默认强度
pub const DEFAULT_STRENGTH: f32 = 1.0;

/// 赫布式增强增量：每次共激活时强度增加量
pub const POTENTIATION_INCREMENT: f32 = 0.05;

/// 间隔式强化的最小间隔（小时）：超过此间隔视为间隔式强化
pub const SPACED_INTERVAL_HOURS: f32 = 1.0;

/// 稳定性增量：间隔式强化时稳定性的增加量
pub const STABILITY_INCREMENT: f32 = 0.1;

/// 衰减速率（每天）：调节艾宾浩斯遗忘曲线的衰减速度
pub const DECAY_RATE: f32 = 0.01;

// ── 自由函数 ──────────────────────────────────────────

/// 计算艾宾浩斯遗忘曲线
///
/// 公式：`strength * exp(-days / stability)`
///
/// - `strength`: 当前强度
/// - `days_since_activation`: 距上次激活的天数
/// - `stability`: 稳定性，越大则衰减越慢
///
/// 返回衰减后的强度。stability 越大，记忆越稳固，遗忘越慢。
pub fn compute_forgetting_curve(strength: f32, days_since_activation: f32, stability: f32) -> f32 {
    debug!(
        "Dynamics: compute_forgetting_curve strength={}, days={}, stability={}",
        strength, days_since_activation, stability
    );
    let effective_stability = if stability > 0.0 {
        stability
    } else {
        warn!("Dynamics: stability 为 0，直接返回 0");
        return 0.0;
    };
    let days = days_since_activation.max(0.0);
    if days < 0.0 {
        warn!("Dynamics: days_since_activation 为负，已 clamp 到 0");
    }
    let result = strength * (-days / effective_stability).exp();
    debug!("Dynamics: compute_forgetting_curve 结果={}", result);
    result
}

/// 判断是否为间隔式强化 (Spaced Reinforcement)
///
/// 当两次激活之间的时间间隔大于等于 `SPACED_INTERVAL_HOURS` 时，视为间隔式强化。
/// 间隔式强化能提升连接稳定性（Cepeda 间隔效应）。
///
/// - `last`: 上次激活时间
/// - `now`: 当前激活时间
///
/// 返回 true 表示间隔足够大，属于间隔式强化。
pub fn is_spaced_reinforcement(last: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let elapsed = now.signed_duration_since(last);
    let seconds = elapsed.num_seconds().max(0) as f32;
    let hours = seconds / 3600.0;
    let result = hours >= SPACED_INTERVAL_HOURS;
    debug!(
        "Dynamics: is_spaced_reinforcement last={}, now={}, elapsed_hours={:.2}, result={}",
        last, now, hours, result
    );
    if elapsed.num_seconds() < 0 {
        warn!("Dynamics: 时钟回拨，last={}, now={}", last, now);
    }
    result
}

// ── 连接动力学结构体 ─────────────────────────────────

/// 记忆连接动力学状态
///
/// 描述两个记忆之间连接的强度与稳定性，随共激活与时间演化。
/// 纯数据结构，所有更新通过显式方法触发。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConnectionDynamics {
    /// 连接强度 (0.05 ~ 5.0)：当前激活水平，越高越容易被回忆
    pub strength: f32,
    /// 稳定性：抗衰减能力，随间隔式强化增长，调节遗忘曲线斜率
    pub stability: f32,
    /// 最后一次激活时间：用于计算衰减与间隔
    pub last_activated: Option<DateTime<Utc>>,
    /// 累计访问次数：记录被共激活的总次数
    pub access_count: u32,
}

impl ConnectionDynamics {
    /// 创建新的连接动力学状态（使用默认值）
    pub fn new() -> Self {
        Self {
            strength: DEFAULT_STRENGTH,
            stability: DEFAULT_STABILITY,
            last_activated: None,
            access_count: 0,
        }
    }

    /// 赫布式增强：两个记忆被共同访问时调用
    ///
    /// 1. 增加强度（受 MAX_STRENGTH 上限约束）
    /// 2. 若为间隔式强化，增加稳定性（Cepeda 间隔效应）
    /// 3. 更新最后激活时间
    /// 4. 递增访问计数
    ///
    /// 注意：间隔判断使用更新前的 `last_activated`，确保间隔度量正确。
    pub fn potentiate(&mut self, now: DateTime<Utc>) {
        info!(
            "Dynamics: potentiate 开始 strength={:.4}, stability={:.4}, access_count={}",
            self.strength, self.stability, self.access_count
        );
        let was_spaced = if let Some(last) = self.last_activated {
            if is_spaced_reinforcement(last, now) {
                self.stability += STABILITY_INCREMENT;
                debug!(
                    "Dynamics: potentiate 间隔式强化，稳定性增加 {}，新值={:.4}",
                    STABILITY_INCREMENT, self.stability
                );
                true
            } else {
                false
            }
        } else {
            false
        };
        let old_strength = self.strength;
        self.strength = (self.strength + POTENTIATION_INCREMENT).min(MAX_STRENGTH);
        self.last_activated = Some(now);
        self.access_count += 1;
        info!(
            "Dynamics: potentiate 完成 strength={:.4}(+{}), stability={:.4}, access_count={}, spaced={}",
            self.strength,
            POTENTIATION_INCREMENT,
            self.stability,
            self.access_count,
            was_spaced
        );
        if old_strength == self.strength && self.strength >= MAX_STRENGTH {
            debug!(
                "Dynamics: potentiate 强度已达上限 MAX_STRENGTH={}",
                MAX_STRENGTH
            );
        }
    }

    /// 应用艾宾浩斯指数衰减
    ///
    /// 根据自上次激活以来的时间，按遗忘曲线衰减强度。
    /// 衰减速率由 `DECAY_RATE` 调节，稳定性越高衰减越慢。
    /// 强度永远不会低于 `STRENGTH_FLOOR`。
    ///
    /// 若从未激活 (`last_activated` 为 None)，则不衰减。
    pub fn apply_decay(&mut self, now: DateTime<Utc>) {
        let last = match self.last_activated {
            Some(t) => t,
            None => {
                debug!("Dynamics: apply_decay 从未激活，跳过衰减");
                return;
            }
        };
        info!(
            "Dynamics: apply_decay 开始 strength={:.4}, stability={:.4}, last_activated={}",
            self.strength, self.stability, last
        );
        let elapsed = now.signed_duration_since(last);
        let seconds = elapsed.num_seconds().max(0) as f32;
        let days = seconds / 86400.0;
        if elapsed.num_seconds() < 0 {
            warn!("Dynamics: apply_decay 时钟回拨，跳过衰减");
            return;
        }
        let old_strength = self.strength;
        let decayed = compute_forgetting_curve(self.strength, days * DECAY_RATE, self.stability);
        self.strength = decayed.max(STRENGTH_FLOOR);
        info!(
            "Dynamics: apply_decay 完成 elapsed_days={:.2}, strength={:.4}(前={:.4}, 衰减={:.2}%)",
            days,
            self.strength,
            old_strength,
            ((old_strength - self.strength) / old_strength * 100.0).max(0.0)
        );
        if self.strength <= STRENGTH_FLOOR {
            debug!("Dynamics: apply_decay 强度已降至地板值 {}", STRENGTH_FLOOR);
        }
    }

    /// 连接是否处于活跃状态
    ///
    /// 强度大于 `STRENGTH_FLOOR` 即视为活跃。
    pub fn is_active(&self) -> bool {
        let result = self.strength > STRENGTH_FLOOR;
        debug!(
            "Dynamics: is_active strength={:.4}, floor={}, result={}",
            self.strength, STRENGTH_FLOOR, result
        );
        result
    }

    /// 判断连接是否应被修剪
    ///
    /// 修剪条件：不活跃 (强度 <= STRENGTH_FLOOR) 且 超过最大年龄。
    /// 仍活跃的连接无论多老都不会被修剪。
    ///
    /// - `now`: 当前时间
    /// - `max_age_days`: 自上次激活以来的最大允许天数
    pub fn should_prune(&self, now: DateTime<Utc>, max_age_days: u32) -> bool {
        debug!(
            "Dynamics: should_prune strength={:.4}, max_age_days={}",
            self.strength, max_age_days
        );
        if self.is_active() {
            debug!("Dynamics: should_prune 仍活跃，不修剪");
            return false;
        }
        match self.last_activated {
            Some(last) => {
                let elapsed = now.signed_duration_since(last);
                let days = elapsed.num_seconds().max(0) as f32 / 86400.0;
                let result = days > max_age_days as f32;
                debug!(
                    "Dynamics: should_prune elapsed_days={:.2}, max_age_days={}, result={}",
                    days, max_age_days, result
                );
                result
            }
            None => {
                debug!("Dynamics: should_prune 从未激活且不活跃，视为死连接，修剪");
                true
            }
        }
    }
}

impl Default for ConnectionDynamics {
    fn default() -> Self {
        Self::new()
    }
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    /// 浮点近似相等辅助
    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn test_new_default_values() {
        let d = ConnectionDynamics::new();
        assert!(approx_eq(d.strength, DEFAULT_STRENGTH));
        assert!(approx_eq(d.stability, DEFAULT_STABILITY));
        assert_eq!(d.last_activated, None);
        assert_eq!(d.access_count, 0);
    }

    #[test]
    fn test_potentiation_increases_strength() {
        // 赫布式增强：共激活应提升强度并更新状态
        let mut d = ConnectionDynamics::new();
        let now = Utc::now();
        let initial = d.strength;

        d.potentiate(now);

        assert!(approx_eq(d.strength, initial + POTENTIATION_INCREMENT));
        assert_eq!(d.access_count, 1);
        assert_eq!(d.last_activated, Some(now));
    }

    #[test]
    fn test_potentiation_caps_at_max_strength() {
        // 强度封顶于 MAX_STRENGTH
        let mut d = ConnectionDynamics::new();
        d.strength = MAX_STRENGTH - 0.01;

        d.potentiate(Utc::now());

        assert!(approx_eq(d.strength, MAX_STRENGTH));
    }

    #[test]
    fn test_decay_reduces_strength() {
        // 艾宾浩斯衰减：时间推移应降低强度
        let mut d = ConnectionDynamics::new();
        let now = Utc::now();
        d.potentiate(now);
        let before = d.strength;

        // 10 天后衰减
        d.apply_decay(now + Duration::days(10));

        assert!(d.strength < before, "strength should decay over time");
    }

    #[test]
    fn test_decay_floored_at_strength_floor() {
        // 强度永远不低于 STRENGTH_FLOOR
        let mut d = ConnectionDynamics::new();
        d.strength = STRENGTH_FLOOR;
        let now = Utc::now();
        d.last_activated = Some(now);

        // 即使过了很久，也不低于地板值
        d.apply_decay(now + Duration::days(365));

        assert!(approx_eq(d.strength, STRENGTH_FLOOR));
    }

    #[test]
    fn test_decay_no_op_without_activation() {
        // 从未激活则不衰减
        let mut d = ConnectionDynamics::new();
        let before = d.strength;
        d.apply_decay(Utc::now());
        assert!(approx_eq(d.strength, before));
    }

    #[test]
    fn test_spacing_effect_increases_stability() {
        // Cepeda 间隔效应：间隔式强化应提升稳定性
        let mut d = ConnectionDynamics::new();
        let t0 = Utc::now();
        d.potentiate(t0);
        let stability_before = d.stability;

        // 间隔 2 小时（超过 SPACED_INTERVAL_HOURS）再次激活
        d.potentiate(t0 + Duration::hours(2));

        assert!(
            d.stability > stability_before,
            "spaced reinforcement should increase stability"
        );
        assert!(approx_eq(
            d.stability,
            stability_before + STABILITY_INCREMENT
        ));
    }

    #[test]
    fn test_massed_reinforcement_no_stability_gain() {
        // 集中式强化（间隔不足）不应提升稳定性
        let mut d = ConnectionDynamics::new();
        let t0 = Utc::now();
        d.potentiate(t0);
        let stability_before = d.stability;

        // 仅隔 10 分钟（小于 SPACED_INTERVAL_HOURS）
        d.potentiate(t0 + Duration::minutes(10));

        assert!(
            approx_eq(d.stability, stability_before),
            "massed reinforcement should not increase stability"
        );
        // 但强度仍应增加
        assert!(d.strength > DEFAULT_STRENGTH);
    }

    #[test]
    fn test_is_active() {
        let mut d = ConnectionDynamics::new();
        assert!(d.is_active());

        d.strength = STRENGTH_FLOOR;
        assert!(!d.is_active(), "at floor should be inactive");

        d.strength = STRENGTH_FLOOR + 0.01;
        assert!(d.is_active());
    }

    #[test]
    fn test_should_prune_inactive_and_old() {
        // 不活跃且超龄应修剪
        let mut d = ConnectionDynamics::new();
        let now = Utc::now();
        d.last_activated = Some(now);
        d.strength = STRENGTH_FLOOR;

        // 未超龄：不修剪
        assert!(!d.should_prune(now, 30));

        // 超过最大年龄：修剪
        assert!(d.should_prune(now + Duration::days(31), 30));
    }

    #[test]
    fn test_should_not_prune_active() {
        // 仍活跃则无论多老都不修剪
        let mut d = ConnectionDynamics::new();
        let now = Utc::now();
        d.last_activated = Some(now);
        // strength 仍为默认 1.0 > STRENGTH_FLOOR

        assert!(!d.should_prune(now + Duration::days(365), 30));
    }

    #[test]
    fn test_should_prune_never_activated_inactive() {
        // 从未激活且不活跃：视为死连接
        let mut d = ConnectionDynamics::new();
        d.strength = STRENGTH_FLOOR;
        d.last_activated = None;
        assert!(d.should_prune(Utc::now(), 30));
    }

    #[test]
    fn test_compute_forgetting_curve() {
        // 0 天不衰减
        let r = compute_forgetting_curve(1.0, 0.0, 1.0);
        assert!(approx_eq(r, 1.0));

        // 稳定性越大衰减越慢
        let fast = compute_forgetting_curve(1.0, 1.0, 1.0);
        let slow = compute_forgetting_curve(1.0, 1.0, 10.0);
        assert!(fast < slow, "higher stability should decay slower");

        // 强度越高结果越大（线性缩放）
        let a = compute_forgetting_curve(1.0, 1.0, 1.0);
        let b = compute_forgetting_curve(2.0, 1.0, 1.0);
        assert!(approx_eq(b, 2.0 * a));
    }

    #[test]
    fn test_compute_forgetting_curve_zero_stability() {
        // 稳定性为 0 时不产生 NaN，直接归零
        let r = compute_forgetting_curve(1.0, 1.0, 0.0);
        assert!(approx_eq(r, 0.0));
    }

    #[test]
    fn test_compute_forgetting_curve_negative_days() {
        // 负天数（时钟回拨）应 clamp 到 0，不增加强度
        let r = compute_forgetting_curve(1.0, -5.0, 1.0);
        assert!(approx_eq(r, 1.0));
    }

    #[test]
    fn test_is_spaced_reinforcement_spaced() {
        let t0 = Utc::now();
        // 间隔 2 小时：超过 1 小时阈值
        assert!(is_spaced_reinforcement(t0, t0 + Duration::hours(2)));
    }

    #[test]
    fn test_is_spaced_reinforcement_massed() {
        let t0 = Utc::now();
        // 间隔 30 分钟：不足阈值
        assert!(!is_spaced_reinforcement(t0, t0 + Duration::minutes(30)));
    }

    #[test]
    fn test_is_spaced_reinforcement_exact_boundary() {
        let t0 = Utc::now();
        // 恰好等于阈值：>= 视为间隔式
        assert!(is_spaced_reinforcement(t0, t0 + Duration::hours(1)));
    }

    #[test]
    fn test_full_lifecycle() {
        // 完整生命周期：创建 -> 多次间隔强化 -> 衰减 -> 修剪
        let mut d = ConnectionDynamics::new();
        let t0 = Utc::now();

        // 间隔式强化多次（每次间隔 2 小时）
        d.potentiate(t0);
        d.potentiate(t0 + Duration::hours(2));
        d.potentiate(t0 + Duration::hours(4));
        d.potentiate(t0 + Duration::hours(6));

        // 经过 3 次间隔式强化，稳定性应增加 3 次
        assert!(approx_eq(
            d.stability,
            DEFAULT_STABILITY + 3.0 * STABILITY_INCREMENT
        ));
        assert_eq!(d.access_count, 4);

        // 长时间后衰减但仍活跃
        d.apply_decay(t0 + Duration::hours(6) + Duration::days(5));
        assert!(d.is_active(), "should still be active after moderate decay");

        // 更长时间后降至地板值并超龄
        d.apply_decay(t0 + Duration::hours(6) + Duration::days(500));
        assert!(!d.is_active());
        assert!(d.should_prune(t0 + Duration::hours(6) + Duration::days(500), 30));
    }
}
