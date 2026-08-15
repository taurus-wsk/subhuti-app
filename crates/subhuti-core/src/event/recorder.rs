//! # 事件记录与回放
//!
//! 记录 Agent 执行过程中所有事件，支持回放用于调试和分析。
//!
//! ## 使用方式
//!
//! ```rust,ignore
//! use subhuti::event::{EventBus, EventRecorder};
//!
//! let bus = EventBus::new(1024);
//!
//! // 1. 开始记录
//! let recorder = EventRecorder::new();
//! bus.subscribe(Arc::new(recorder.clone())).await;
//!
//! // 2. 执行 Agent 任务...
//!
//! // 3. 回放事件
//! let recording = recorder.get_recording().await;
//! for event in recording.events() {
//!     println!("[{}] {:?}", event.metadata.timestamp.0, event.data);
//! }
//! ```

use super::handler::{EventFilter, EventHandler};
use super::types::{AgentEventData, Event};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// 单次记录会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    /// 记录 ID
    pub id: String,
    /// 记录开始时间
    pub started_at: DateTime<Utc>,
    /// 记录结束时间
    pub ended_at: Option<DateTime<Utc>>,
    /// 关联的 trace_id
    pub trace_id: Option<String>,
    /// 关联的 session_id
    pub session_id: Option<String>,
    /// 事件列表
    pub events: Vec<Event>,
}

impl Recording {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            started_at: Utc::now(),
            ended_at: None,
            trace_id: None,
            session_id: None,
            events: Vec::new(),
        }
    }

    /// 事件数量
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 获取所有事件
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// 按类型过滤事件
    pub fn filter_by_type(&self, event_type: &str) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.data.event_type() == event_type)
            .collect()
    }

    /// 按 trace_id 过滤事件
    pub fn filter_by_trace(&self, trace_id: &str) -> Vec<&Event> {
        self.events
            .iter()
            .filter(|e| e.metadata.trace_id.as_deref() == Some(trace_id))
            .collect()
    }

    /// 获取时间线摘要
    pub fn timeline_summary(&self) -> Vec<TimelineEntry> {
        self.events
            .iter()
            .map(|e| TimelineEntry {
                timestamp: e.metadata.timestamp.0,
                event_type: e.data.event_type().to_string(),
                trace_id: e.metadata.trace_id.clone(),
                summary: e.data.summary(),
            })
            .collect()
    }

    /// 统计各类型事件数量
    pub fn event_counts(&self) -> std::collections::HashMap<String, usize> {
        let mut counts = std::collections::HashMap::new();
        for e in &self.events {
            *counts.entry(e.data.event_type().to_string()).or_insert(0) += 1;
        }
        counts
    }

    /// 总耗时（毫秒）
    pub fn total_duration_ms(&self) -> u64 {
        if let (Some(first), Some(last)) = (self.events.first(), self.events.last()) {
            (last.metadata.timestamp.0 - first.metadata.timestamp.0).num_milliseconds() as u64
        } else {
            0
        }
    }
}

impl Default for Recording {
    fn default() -> Self {
        Self::new()
    }
}

/// 时间线条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub trace_id: Option<String>,
    pub summary: String,
}

/// 事件记录器
///
/// 作为 EventHandler 订阅到 EventBus，记录所有事件。
/// 支持 clone（内部共享状态），可以在多个地方引用。
#[derive(Clone)]
pub struct EventRecorder {
    inner: Arc<RwLock<Recording>>,
    /// 最大记录事件数（防止内存溢出）
    max_events: usize,
}

impl EventRecorder {
    pub fn new() -> Self {
        Self::with_capacity(10000)
    }

    pub fn with_capacity(max_events: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Recording::new())),
            max_events,
        }
    }

    /// 开始新的记录（清空之前的记录）
    pub async fn start(&self) {
        let mut rec = self.inner.write().await;
        *rec = Recording::new();
    }

    /// 结束记录
    pub async fn stop(&self) {
        let mut rec = self.inner.write().await;
        rec.ended_at = Some(Utc::now());
    }

    /// 获取当前记录的快照
    pub async fn get_recording(&self) -> Recording {
        self.inner.read().await.clone()
    }

    /// 获取已记录的事件数量
    pub async fn event_count(&self) -> usize {
        self.inner.read().await.events.len()
    }

    /// 导出为 JSON 字符串
    pub async fn to_json(&self) -> serde_json::Result<String> {
        let rec = self.inner.read().await;
        serde_json::to_string_pretty(&*rec)
    }

    /// 从 JSON 导入
    pub async fn from_json(&self, json: &str) -> serde_json::Result<()> {
        let recording: Recording = serde_json::from_str(json)?;
        let mut rec = self.inner.write().await;
        *rec = recording;
        Ok(())
    }
}

impl Default for EventRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EventRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventRecorder")
            .field("max_events", &self.max_events)
            .finish()
    }
}

#[async_trait]
impl EventHandler for EventRecorder {
    async fn handle(&self, event: &Event) {
        let mut rec = self.inner.write().await;
        if rec.events.len() < self.max_events {
            // 记录 trace_id 和 session_id
            if rec.trace_id.is_none() {
                rec.trace_id = event.metadata.trace_id.clone();
            }
            if rec.session_id.is_none() {
                rec.session_id = event.metadata.session_id.clone();
            }
            rec.events.push(event.clone());
        }
    }

    fn filter(&self) -> EventFilter {
        EventFilter::All
    }

    fn name(&self) -> &str {
        "recorder"
    }
}

/// 回放回调可能失败
pub type ReplayResult<T> = std::result::Result<T, ReplayError>;

/// 回放错误
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayError {
    /// 事件索引
    pub event_index: usize,
    /// 事件类型
    pub event_type: String,
    /// 错误信息
    pub message: String,
    /// 重试次数
    pub retries: usize,
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[ReplayError] event #{} ({}) failed after {} retries: {}",
            self.event_index, self.event_type, self.retries, self.message
        )
    }
}

impl std::error::Error for ReplayError {}

impl From<&str> for ReplayError {
    fn from(s: &str) -> Self {
        Self {
            event_index: 0,
            event_type: String::new(),
            message: s.to_string(),
            retries: 0,
        }
    }
}

impl From<String> for ReplayError {
    fn from(s: String) -> Self {
        Self {
            event_index: 0,
            event_type: String::new(),
            message: s,
            retries: 0,
        }
    }
}

/// 回放策略
#[derive(Debug, Clone)]
pub enum ReplayStrategy {
    /// 遇到错误立即停止
    StopOnError,
    /// 跳过失败事件，继续回放
    SkipOnError,
    /// 重试指定次数后跳过
    RetryThenSkip { max_retries: usize, delay_ms: u64 },
    /// 重试指定次数后停止
    RetryThenStop { max_retries: usize, delay_ms: u64 },
}

impl Default for ReplayStrategy {
    fn default() -> Self {
        Self::SkipOnError
    }
}

/// 回放统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayStats {
    /// 总事件数
    pub total_events: usize,
    /// 成功回放数
    pub succeeded: usize,
    /// 失败数
    pub failed: usize,
    /// 跳过数
    pub skipped: usize,
    /// 总重试次数
    pub total_retries: usize,
    /// 错误列表
    pub errors: Vec<ReplayError>,
}

impl ReplayStats {
    pub fn is_all_success(&self) -> bool {
        self.failed == 0 && self.errors.is_empty()
    }
}

/// 事件回放器
///
/// 从 Recording 回放事件，用于调试和分析。
/// 支持重试机制和异常处理策略。
pub struct EventPlayer {
    recording: Recording,
    /// 回放速度倍率（1.0 = 实时，0 = 瞬间完成）
    speed: f64,
    /// 回放回调（返回 Result）
    callback: Option<Box<dyn Fn(&Event) -> ReplayResult<()> + Send + Sync>>,
    /// 错误处理策略
    strategy: ReplayStrategy,
}

impl EventPlayer {
    pub fn new(recording: Recording) -> Self {
        Self {
            recording,
            speed: 0.0,
            callback: None,
            strategy: ReplayStrategy::default(),
        }
    }

    /// 设置回放速度（1.0 = 实时，0 = 瞬间，2.0 = 两倍速）
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// 设置回放回调（可失败版本）
    ///
    /// 回调返回 Err 时，根据 strategy 处理。
    pub fn with_callback<F>(mut self, f: F) -> Self
    where
        F: Fn(&Event) -> ReplayResult<()> + Send + Sync + 'static,
    {
        self.callback = Some(Box::new(f));
        self
    }

    /// 设置错误处理策略
    pub fn with_strategy(mut self, strategy: ReplayStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// 同步回放（瞬间完成）
    ///
    /// 返回回放统计信息。
    pub fn replay_sync(&self) -> ReplayStats {
        let mut stats = ReplayStats {
            total_events: self.recording.events.len(),
            ..Default::default()
        };

        for (index, event) in self.recording.events.iter().enumerate() {
            if let Some(ref cb) = self.callback {
                match self.execute_with_retry(cb, event, index, &mut stats) {
                    Ok(()) => stats.succeeded += 1,
                    Err(err) => {
                        stats.failed += 1;
                        stats.errors.push(err.clone());

                        match &self.strategy {
                            ReplayStrategy::StopOnError => {
                                tracing::warn!("Replay stopped at event #{}: {}", index, err);
                                return stats;
                            }
                            ReplayStrategy::SkipOnError => {
                                stats.skipped += 1;
                                tracing::warn!("Replay skipped event #{}: {}", index, err);
                            }
                            ReplayStrategy::RetryThenSkip { .. } => {
                                stats.skipped += 1;
                                tracing::warn!(
                                    "Replay skipped event #{} after retries: {}",
                                    index,
                                    err
                                );
                            }
                            ReplayStrategy::RetryThenStop { .. } => {
                                tracing::warn!(
                                    "Replay stopped at event #{} after retries: {}",
                                    index,
                                    err
                                );
                                return stats;
                            }
                        }
                    }
                }
            } else {
                stats.succeeded += 1;
            }
        }

        stats
    }

    /// 异步回放（按原始时间间隔）
    ///
    /// 返回回放统计信息。
    pub async fn replay(&self) -> ReplayStats {
        if self.speed == 0.0 || self.recording.events.len() < 2 {
            return self.replay_sync();
        }

        let mut stats = ReplayStats {
            total_events: self.recording.events.len(),
            ..Default::default()
        };

        let events = &self.recording.events;

        // 第一个事件
        if let Some(first) = events.first() {
            if let Some(ref cb) = self.callback {
                match self.execute_with_retry(cb, first, 0, &mut stats) {
                    Ok(()) => stats.succeeded += 1,
                    Err(err) => {
                        stats.failed += 1;
                        stats.errors.push(err);
                        if matches!(self.strategy, ReplayStrategy::StopOnError) {
                            return stats;
                        }
                        stats.skipped += 1;
                    }
                }
            } else {
                stats.succeeded += 1;
            }
        }

        for window in events.windows(2) {
            let prev = &window[0];
            let curr = &window[1];
            let index = stats.succeeded + stats.failed;

            // 计算原始间隔
            let original_delay = (curr.metadata.timestamp.0 - prev.metadata.timestamp.0)
                .to_std()
                .unwrap_or(std::time::Duration::ZERO);

            // 应用速度倍率
            let delay = if self.speed >= 1.0 {
                original_delay / self.speed as u32
            } else {
                original_delay.mul_f64(1.0 / self.speed)
            };

            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            if let Some(ref cb) = self.callback {
                match self.execute_with_retry(cb, curr, index, &mut stats) {
                    Ok(()) => stats.succeeded += 1,
                    Err(err) => {
                        stats.failed += 1;
                        stats.errors.push(err.clone());

                        match &self.strategy {
                            ReplayStrategy::StopOnError => {
                                tracing::warn!("Replay stopped at #{}: {}", index, err);
                                return stats;
                            }
                            ReplayStrategy::SkipOnError => {
                                stats.skipped += 1;
                            }
                            ReplayStrategy::RetryThenSkip { .. } => {
                                stats.skipped += 1;
                            }
                            ReplayStrategy::RetryThenStop { .. } => {
                                tracing::warn!("Replay stopped at #{}: {}", index, err);
                                return stats;
                            }
                        }
                    }
                }
            } else {
                stats.succeeded += 1;
            }
        }

        stats
    }

    /// 带重试的执行单个事件回调
    fn execute_with_retry(
        &self,
        cb: &Box<dyn Fn(&Event) -> ReplayResult<()> + Send + Sync>,
        event: &Event,
        index: usize,
        stats: &mut ReplayStats,
    ) -> ReplayResult<()> {
        let max_retries = match &self.strategy {
            ReplayStrategy::RetryThenSkip { max_retries, .. }
            | ReplayStrategy::RetryThenStop { max_retries, .. } => *max_retries,
            _ => 0,
        };

        let retry_delay = match &self.strategy {
            ReplayStrategy::RetryThenSkip { delay_ms, .. }
            | ReplayStrategy::RetryThenStop { delay_ms, .. } => *delay_ms,
            _ => 0,
        };

        let mut last_error = String::new();

        for attempt in 0..=max_retries {
            if attempt > 0 {
                stats.total_retries += 1;
                if retry_delay > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(retry_delay));
                }
                tracing::debug!(
                    "Replay retry #{} for event #{} ({})",
                    attempt,
                    index,
                    event.data.event_type()
                );
            }

            match cb(event) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_error = e.to_string();
                    if attempt < max_retries {
                        continue;
                    }
                }
            }
        }

        Err(ReplayError {
            event_index: index,
            event_type: event.data.event_type().to_string(),
            message: last_error,
            retries: max_retries,
        })
    }

    /// 打印时间线
    pub fn print_timeline(&self) {
        println!(
            "📋 Recording {} ({} events, {}ms total)",
            self.recording.id,
            self.recording.events.len(),
            self.recording.total_duration_ms()
        );
        println!("─────────────────────────────────────────────────────────────");

        for (i, entry) in self.recording.timeline_summary().iter().enumerate() {
            let trace = entry.trace_id.as_deref().unwrap_or("-");
            println!(
                "{:3} | {} | {:20} | trace={} | {}",
                i,
                entry.timestamp.format("%H:%M:%S%.3f"),
                entry.event_type,
                &trace[..trace.len().min(8)],
                entry.summary
            );
        }

        println!("─────────────────────────────────────────────────────────────");
        let counts = self.recording.event_counts();
        for (event_type, count) in &counts {
            println!("  {:20} : {} 次", event_type, count);
        }
    }
}

impl std::fmt::Debug for EventPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventPlayer")
            .field("events", &self.recording.events.len())
            .field("speed", &self.speed)
            .field("has_callback", &self.callback.is_some())
            .finish()
    }
}

/// 事件摘要（用于时间线显示）
impl AgentEventData {
    pub fn summary(&self) -> String {
        match self {
            Self::UserMessage { message } => {
                let msg = if message.len() > 50 {
                    &message[..50]
                } else {
                    message
                };
                format!("input: {}", msg)
            }
            Self::AgentMatched {
                agent_id,
                match_score,
                ..
            } => format!("agent={}, score={:.2}", agent_id, match_score),
            Self::ChainSelected { chain_name, .. } => format!("chain={}", chain_name),
            Self::AgentStarted { agent_id, .. } => format!("agent={}", agent_id),
            Self::AgentCompleted {
                agent_id,
                duration_ms,
                ..
            } => format!("agent={}, {}ms", agent_id, duration_ms),
            Self::AgentFailed {
                agent_id, error, ..
            } => format!("agent={}, error={}", agent_id, error),
            Self::FlowStarted { flow_type, .. } => format!("flow={}", flow_type),
            Self::FlowStepExecuted {
                step_index,
                step_name,
                ..
            } => format!("#{} {}", step_index, step_name),
            Self::FlowCompleted { iterations, .. } => format!("{} iterations", iterations),
            Self::LLMCalling { messages_count, .. } => format!("{} messages", messages_count),
            Self::LLMResponded {
                tokens_used,
                duration_ms,
                ..
            } => format!("{} tokens, {}ms", tokens_used, duration_ms),
            Self::LLMStreamChunk { chunk } => {
                let c = if chunk.len() > 30 {
                    &chunk[..30]
                } else {
                    chunk
                };
                format!("chunk: {}", c)
            }
            Self::ToolCalling { tool_name, .. } => format!("tool={}", tool_name),
            Self::ToolResponded {
                tool_name,
                success,
                duration_ms,
                ..
            } => format!("tool={}, ok={}, {}ms", tool_name, success, duration_ms),
            Self::MemoryWritten { key, category } => format!("{}={}", category, key),
            Self::MemoryRetrieved { results_count, .. } => format!("{} results", results_count),
            Self::SpanStarted { span_name, .. } => format!("span={}", span_name),
            Self::SpanEnded {
                span_name,
                duration_ms,
                ..
            } => format!("span={}, {}ms", span_name, duration_ms),
            Self::GraphStarted {
                graph_name,
                entry_node,
                ..
            } => {
                format!("graph={}, entry={}", graph_name, entry_node)
            }
            Self::NodeExecuteRequested {
                node_name, step, ..
            } => {
                format!("req node={}, step={}", node_name, step)
            }
            Self::NodeCompleted {
                node_name,
                success,
                duration_ms,
                ..
            } => format!("done node={}, ok={}, {}ms", node_name, success, duration_ms),
            Self::NodeFailed {
                node_name, error, ..
            } => format!("fail node={}, err={}", node_name, error),
            Self::GraphCompleted {
                success,
                total_steps,
                ..
            } => format!("graph done, ok={}, {} steps", success, total_steps),
            Self::ActorTaskRequested { node_name, .. } => {
                format!("actor task: node={}", node_name)
            }
            Self::ActorBid {
                actor_id, score, ..
            } => {
                format!("actor bid: {} score={}", actor_id, score)
            }
            Self::NodeTaskAssigned { actor_name, .. } => {
                format!("task assigned: {}", actor_name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AgentEventData, EventBus};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_recorder() {
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        bus.emit(AgentEventData::UserMessage {
            message: "test".to_string(),
        })
        .await;

        bus.emit(AgentEventData::LLMCalling {
            messages_count: 1,
            model: None,
        })
        .await;

        bus.emit(AgentEventData::LLMResponded {
            response: "response".to_string(),
            tokens_used: 10,
            duration_ms: 100,
        })
        .await;

        // 等待异步处理
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let recording = recorder.get_recording().await;
        assert_eq!(recording.events.len(), 3);
        assert_eq!(recording.event_counts().get("user_message"), Some(&1));
    }

    #[tokio::test]
    async fn test_replay() {
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        bus.emit(AgentEventData::UserMessage {
            message: "hello".to_string(),
        })
        .await;

        bus.emit(AgentEventData::AgentStarted {
            agent_id: "default".to_string(),
            input: "hello".to_string(),
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        assert_eq!(recording.events.len(), 2);

        // 回放
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let player = EventPlayer::new(recording).with_callback(move |_event| {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });

        let stats = player.replay_sync();

        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(stats.succeeded, 2);
        assert!(stats.is_all_success());
    }

    #[tokio::test]
    async fn test_replay_with_retry() {
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        bus.emit(AgentEventData::UserMessage {
            message: "hello".to_string(),
        })
        .await;

        bus.emit(AgentEventData::LLMCalling {
            messages_count: 1,
            model: None,
        })
        .await;

        bus.emit(AgentEventData::LLMResponded {
            response: "ok".to_string(),
            tokens_used: 5,
            duration_ms: 10,
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        assert_eq!(recording.events.len(), 3);

        // 第2个事件失败，重试2次后跳过
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let player = EventPlayer::new(recording)
            .with_callback(move |event| {
                counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                // LLMCalling 事件失败
                if event.data.event_type() == "llm_calling" {
                    return Err("simulated failure".into());
                }
                Ok(())
            })
            .with_strategy(ReplayStrategy::RetryThenSkip {
                max_retries: 2,
                delay_ms: 0,
            });

        let stats = player.replay_sync();

        // 3 个事件，第2个失败重试2次后跳过
        assert_eq!(stats.total_events, 3);
        assert_eq!(stats.succeeded, 2); // 第1、3个成功
        assert_eq!(stats.failed, 1); // 第2个失败
        assert_eq!(stats.skipped, 1); // 第2个跳过
        assert_eq!(stats.total_retries, 2); // 重试2次
        assert_eq!(stats.errors.len(), 1);
    }

    #[tokio::test]
    async fn test_replay_stop_on_error() {
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        for i in 0..3 {
            bus.emit(AgentEventData::UserMessage {
                message: format!("msg_{}", i),
            })
            .await;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        assert_eq!(recording.events.len(), 3);

        // 第2个事件失败，StopOnError 策略
        let player = EventPlayer::new(recording)
            .with_callback(|event| {
                if event.data.event_type() == "user_message"
                    && matches!(&event.data, AgentEventData::UserMessage { message } if message == "msg_1")
                {
                    return Err("stop here".into());
                }
                Ok(())
            })
            .with_strategy(ReplayStrategy::StopOnError);

        let stats = player.replay_sync();

        assert_eq!(stats.succeeded, 1); // 只有第1个成功
        assert_eq!(stats.failed, 1); // 第2个失败
        assert_eq!(stats.skipped, 0); // 没有跳过
        assert_eq!(stats.errors.len(), 1);
    }

    #[tokio::test]
    async fn test_replay_llm_unavailable_with_retry_then_skip() {
        // 模拟完整 Agent 执行流程，其中 LLM 服务不可用
        let bus = EventBus::new(64);
        let recorder = EventRecorder::new();
        bus.subscribe(Arc::new(recorder.clone())).await;

        // 1. 用户消息
        bus.emit(AgentEventData::UserMessage {
            message: "帮我写一首诗".to_string(),
        })
        .await;

        // 2. Agent 匹配
        bus.emit(AgentEventData::AgentMatched {
            agent_id: "default".to_string(),
            agent_name: "Default Agent".to_string(),
            match_score: 0.95,
            candidates: vec!["default".to_string()],
        })
        .await;

        // 3. 策略链选择
        bus.emit(AgentEventData::ChainSelected {
            chain_name: "direct".to_string(),
            strategy: "direct".to_string(),
        })
        .await;

        // 4. Agent 启动
        bus.emit(AgentEventData::AgentStarted {
            agent_id: "default".to_string(),
            input: "帮我写一首诗".to_string(),
        })
        .await;

        // 5. Flow 启动
        bus.emit(AgentEventData::FlowStarted {
            flow_type: "React".to_string(),
            input: "帮我写一首诗".to_string(),
        })
        .await;

        // 6. LLM 调用（模拟 LLM 服务不可用，但事件仍然记录）
        bus.emit(AgentEventData::LLMCalling {
            messages_count: 3,
            model: Some("doubao-pro".to_string()),
        })
        .await;

        // 7. LLM 响应失败 -> Agent 失败
        bus.emit(AgentEventData::AgentFailed {
            agent_id: "default".to_string(),
            error: "LLM service unavailable: connection refused".to_string(),
            duration_ms: 5000,
        })
        .await;

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let recording = recorder.get_recording().await;
        assert_eq!(recording.events.len(), 7);

        // 回放：模拟 LLM 调用时服务不可用，重试 3 次后跳过
        let call_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let call_counter_clone = call_counter.clone();
        let llm_call_attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let llm_call_attempts_clone = llm_call_attempts.clone();

        let player = EventPlayer::new(recording)
            .with_callback(move |event| {
                call_counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                match &event.data {
                    // LLM 调用事件：模拟服务不可用，始终失败
                    AgentEventData::LLMCalling { model, .. } => {
                        llm_call_attempts_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Err(format!(
                            "LLM service unavailable (model={})",
                            model.as_deref().unwrap_or("unknown")
                        )
                        .into())
                    }
                    // Agent 失败事件：也失败
                    AgentEventData::AgentFailed { error, .. } => {
                        Err(format!("Agent failed: {}", error).into())
                    }
                    // 其他事件正常处理
                    _ => Ok(()),
                }
            })
            .with_strategy(ReplayStrategy::RetryThenSkip {
                max_retries: 3,
                delay_ms: 0,
            });

        let stats = player.replay_sync();

        // 验证回放统计
        assert_eq!(stats.total_events, 7);
        assert_eq!(stats.succeeded, 5); // 5 个非 LLM 事件成功
        assert_eq!(stats.failed, 2); // LLMCalling + AgentFailed 失败
        assert_eq!(stats.skipped, 2); // 2 个被跳过
        assert_eq!(stats.total_retries, 6); // 2 个失败事件各重试 3 次 = 6
        assert_eq!(stats.errors.len(), 2);

        // 验证 LLM 调用被重试了 3 次（1 次初始 + 3 次重试 = 4 次）
        assert_eq!(
            llm_call_attempts.load(std::sync::atomic::Ordering::SeqCst),
            4
        );

        // 验证错误详情
        let llm_error = stats
            .errors
            .iter()
            .find(|e| e.event_type == "llm_calling")
            .expect("should have LLM calling error");
        assert!(llm_error.message.contains("LLM service unavailable"));
        assert!(llm_error.message.contains("doubao-pro"));
        assert_eq!(llm_error.retries, 3);

        let agent_error = stats
            .errors
            .iter()
            .find(|e| e.event_type == "agent_failed")
            .expect("should have agent failed error");
        assert!(agent_error.message.contains("connection refused"));

        // 验证并非所有事件都被处理（跳过了 2 个）
        assert!(!stats.is_all_success());

        // 回放完成后仍处理了所有事件（包括重试）
        // 5 成功 + 4 LLM 尝试 + 4 AgentFailed 尝试 = 13 次回调调用
        assert_eq!(call_counter.load(std::sync::atomic::Ordering::SeqCst), 13);
    }

    #[tokio::test]
    async fn test_json_export_import() {
        let recorder = EventRecorder::new();
        recorder.start().await;

        // 模拟记录
        {
            let mut rec = recorder.inner.write().await;
            rec.events.push(Event::new(AgentEventData::UserMessage {
                message: "test".to_string(),
            }));
        }

        let json = recorder.to_json().await.unwrap();
        assert!(json.contains("user_message"));

        let recorder2 = EventRecorder::new();
        recorder2.from_json(&json).await.unwrap();

        let recording = recorder2.get_recording().await;
        assert_eq!(recording.events.len(), 1);
    }
}
