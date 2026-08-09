//! # 回放工具函数
//!
//! 封装常用的回放策略，提供开箱即用的工具函数。
//!
//! ## 使用方式
//!
//! ### 基本回放（带重试）
//!
//! ```rust,ignore
//! use subhuti::event::{replay_with_retry, ReplayConfig};
//!
//! let stats = replay_with_retry(&recording, |event| {
//!     // 处理事件
//!     Ok(())
//! }, ReplayConfig::default());
//! ```
//!
//! ### 自定义配置
//!
//! ```rust,ignore
//! let stats = replay_with_retry(&recording, |event| {
//!     if event.data.event_type() == "llm_calling" {
//!         return Err("LLM unavailable".into());
//!     }
//!     Ok(())
//! }, ReplayConfig {
//!     max_retries: 5,
//!     delay_ms: 200,
//!     stop_on_error: false,
//!     speed: 0.0,
//! });
//! ```

use super::recorder::{EventPlayer, Recording, ReplayResult, ReplayStats, ReplayStrategy};
use super::types::Event;

/// 回放配置
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// 最大重试次数（0 = 不重试）
    pub max_retries: usize,
    /// 每次重试间隔（毫秒）
    pub delay_ms: u64,
    /// 重试后仍失败时是否停止（true=停止，false=跳过）
    pub stop_on_error: bool,
    /// 回放速度（0=瞬间，1.0=实时，2.0=两倍速）
    pub speed: f64,
}

impl Default for ReplayConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            delay_ms: 100,
            stop_on_error: false,
            speed: 0.0,
        }
    }
}

impl ReplayConfig {
    /// 不重试，遇到错误直接跳过
    pub fn no_retry() -> Self {
        Self {
            max_retries: 0,
            delay_ms: 0,
            stop_on_error: false,
            speed: 0.0,
        }
    }

    /// 不重试，遇到错误直接停止
    pub fn stop_immediately() -> Self {
        Self {
            max_retries: 0,
            delay_ms: 0,
            stop_on_error: true,
            speed: 0.0,
        }
    }

    /// 快速回放（无延迟、无重试）
    pub fn fast() -> Self {
        Self {
            max_retries: 0,
            delay_ms: 0,
            stop_on_error: false,
            speed: 0.0,
        }
    }

    /// 实时回放（按原始时间间隔）
    pub fn realtime() -> Self {
        Self {
            max_retries: 3,
            delay_ms: 100,
            stop_on_error: false,
            speed: 1.0,
        }
    }

    /// 构建对应的 ReplayStrategy
    fn to_strategy(&self) -> ReplayStrategy {
        if self.max_retries == 0 && self.stop_on_error {
            ReplayStrategy::StopOnError
        } else if self.max_retries == 0 {
            ReplayStrategy::SkipOnError
        } else if self.stop_on_error {
            ReplayStrategy::RetryThenStop {
                max_retries: self.max_retries,
                delay_ms: self.delay_ms,
            }
        } else {
            ReplayStrategy::RetryThenSkip {
                max_retries: self.max_retries,
                delay_ms: self.delay_ms,
            }
        }
    }
}

/// 同步回放（带重试机制）
///
/// 封装了 EventPlayer 的常用配置，一行调用即可回放。
///
/// # 参数
///
/// - `recording`: 事件记录
/// - `callback`: 事件处理回调，返回 `Ok(())` 表示成功，`Err` 表示失败
/// - `config`: 回放配置
///
/// # 返回
///
/// 回放统计信息
///
/// # 示例
///
/// ```rust,ignore
/// use subhuti::event::{replay_with_retry, ReplayConfig};
///
/// let stats = replay_with_retry(&recording, |event| {
///     println!("{}", event.data.event_type());
///     Ok(())
/// }, ReplayConfig::default());
///
/// println!("成功: {}, 失败: {}", stats.succeeded, stats.failed);
/// ```
pub fn replay_with_retry<F>(recording: &Recording, callback: F, config: ReplayConfig) -> ReplayStats
where
    F: Fn(&Event) -> ReplayResult<()> + Send + Sync + 'static,
{
    let player = EventPlayer::new(recording.clone())
        .with_callback(callback)
        .with_speed(config.speed)
        .with_strategy(config.to_strategy());

    // 同步回放
    player.replay_sync()
}

/// 异步回放（带重试机制，按原始时间间隔）
///
/// 与 `replay_with_retry` 类似，但按原始时间间隔异步回放。
/// 需要设置 `config.speed > 0` 才会按时间间隔回放。
///
/// # 示例
///
/// ```rust,ignore
/// use subhuti::event::{replay_with_retry_async, ReplayConfig};
///
/// let stats = replay_with_retry_async(&recording, |event| {
///     println!("[{}] {}", event.metadata.timestamp.0, event.data.event_type());
///     Ok(())
/// }, ReplayConfig::realtime()).await;
/// ```
pub async fn replay_with_retry_async<F>(
    recording: &Recording,
    callback: F,
    config: ReplayConfig,
) -> ReplayStats
where
    F: Fn(&Event) -> ReplayResult<()> + Send + Sync + 'static,
{
    let player = EventPlayer::new(recording.clone())
        .with_callback(callback)
        .with_speed(config.speed)
        .with_strategy(config.to_strategy());

    if config.speed > 0.0 {
        player.replay().await
    } else {
        player.replay_sync()
    }
}

/// 仅回放指定类型的事件
///
/// # 示例
///
/// ```rust,ignore
/// use subhuti::event::replay_filtered;
///
/// // 只回放 LLM 相关事件
/// let stats = replay_filtered(&recording, &["llm_calling", "llm_responded"], |event| {
///     println!("LLM event: {:?}", event.data);
///     Ok(())
/// }, ReplayConfig::default());
/// ```
pub fn replay_filtered<F>(
    recording: &Recording,
    event_types: &[&str],
    callback: F,
    config: ReplayConfig,
) -> ReplayStats
where
    F: Fn(&Event) -> ReplayResult<()> + Send + Sync + 'static,
{
    let types: std::collections::HashSet<String> =
        event_types.iter().map(|s| s.to_string()).collect();

    replay_with_retry(
        recording,
        move |event| {
            if types.contains(event.data.event_type()) {
                callback(event)
            } else {
                Ok(())
            }
        },
        config,
    )
}

/// 回放并打印时间线
///
/// 便捷函数：先打印时间线，再执行回放。
pub fn replay_with_timeline<F>(
    recording: &Recording,
    callback: F,
    config: ReplayConfig,
) -> ReplayStats
where
    F: Fn(&Event) -> ReplayResult<()> + Send + Sync + 'static,
{
    let player = EventPlayer::new(recording.clone());
    player.print_timeline();

    replay_with_retry(recording, callback, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AgentEventData, EventBus, EventRecorder};
    use std::sync::Arc;

    async fn create_test_recording() -> Recording {
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        bus.emit(AgentEventData::UserMessage {
            message: "test".to_string(),
        })
        .await;

        bus.emit(AgentEventData::LLMCalling {
            messages_count: 1,
            model: Some("gpt-4".to_string()),
        })
        .await;

        bus.emit(AgentEventData::LLMResponded {
            response: "ok".to_string(),
            tokens_used: 10,
            duration_ms: 50,
        })
        .await;

        bus.emit(AgentEventData::AgentCompleted {
            agent_id: "default".to_string(),
            output: "done".to_string(),
            duration_ms: 100,
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        recorder.get_recording().await
    }

    #[tokio::test]
    async fn test_replay_with_retry_default() {
        let recording = create_test_recording().await;
        assert_eq!(recording.events.len(), 4);

        let stats = replay_with_retry(&recording, |_event| Ok(()), ReplayConfig::default());

        assert_eq!(stats.succeeded, 4);
        assert_eq!(stats.failed, 0);
        assert!(stats.is_all_success());
    }

    #[tokio::test]
    async fn test_replay_with_retry_llm_failure() {
        let recording = create_test_recording().await;

        // LLM 调用失败
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let stats = replay_with_retry(
            &recording,
            move |event| {
                if event.data.event_type() == "llm_calling" {
                    attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Err("LLM unavailable".into());
                }
                Ok(())
            },
            ReplayConfig {
                max_retries: 2,
                delay_ms: 0,
                stop_on_error: false,
                speed: 0.0,
            },
        );

        // 3 成功 + 1 失败（LLMCalling）
        assert_eq!(stats.succeeded, 3);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.total_retries, 2);
        // 1 次初始 + 2 次重试 = 3 次
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_replay_no_retry() {
        let recording = create_test_recording().await;

        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let attempts_clone = attempts.clone();

        let stats = replay_with_retry(
            &recording,
            move |event| {
                if event.data.event_type() == "llm_calling" {
                    attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    return Err("fail".into());
                }
                Ok(())
            },
            ReplayConfig::no_retry(),
        );

        assert_eq!(stats.succeeded, 3);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.total_retries, 0); // 不重试
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_replay_stop_on_error() {
        let recording = create_test_recording().await;

        let stats = replay_with_retry(
            &recording,
            |event| {
                if event.data.event_type() == "llm_calling" {
                    return Err("stop".into());
                }
                Ok(())
            },
            ReplayConfig::stop_immediately(),
        );

        // 第1个成功，第2个失败即停止
        assert_eq!(stats.succeeded, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.skipped, 0);
    }

    #[tokio::test]
    async fn test_replay_filtered() {
        let recording = create_test_recording().await;

        let llm_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm_count_clone = llm_count.clone();

        let stats = replay_filtered(
            &recording,
            &["llm_calling", "llm_responded"],
            move |_event| {
                llm_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
            ReplayConfig::default(),
        );

        // 所有事件都"成功"了（非 LLM 事件被跳过回调）
        assert_eq!(stats.succeeded, 4);
        // 但只有 2 个 LLM 事件被实际处理
        assert_eq!(llm_count.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_replay_async_realtime() {
        let recording = create_test_recording().await;

        let stats = replay_with_retry_async(
            &recording,
            |_event| Ok(()),
            ReplayConfig {
                speed: 10.0, // 10 倍速
                ..Default::default()
            },
        )
        .await;

        assert_eq!(stats.succeeded, 4);
        assert!(stats.is_all_success());
    }
}
