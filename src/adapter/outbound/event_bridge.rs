//! # Trace 事件桥
//!
//! 实现框架 `EventHandler` trait，订阅框架 EventBus，
//! 将带 trace_id 的事件转换为应用层 `SpanData`，并通过 `TraceObserverPort.record_span`
//! 写入 SubhutiTraceObserverAdapter.spans HashMap，供 get_span_tree 查询组装树。
//!
//! 六边形架构：
//! - 属于 outbound adapter（适配框架 EventHandler → 应用层 TraceObserverPort）
//! - 依赖：框架层 subhuti::event::EventHandler、应用层 TraceObserverPort/SpanData

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use subhuti::event::{AgentEventData, Event, EventFilter, EventHandler};

use crate::application::observer::{SpanData, TraceObserverPort};

/// Trace 事件桥接处理器
pub struct TraceEventBridge {
    trace_observer: Arc<dyn TraceObserverPort>,
}

impl TraceEventBridge {
    pub fn new(trace_observer: Arc<dyn TraceObserverPort>) -> Self {
        Self { trace_observer }
    }

    /// 将单条框架事件转换为 SpanData（可能产出 None：非 span 类型/无 trace_id）
    fn convert(event: &Event) -> Option<SpanData> {
        // 无 trace_id 的事件直接忽略（非桥接目标）
        let trace_id = event.metadata.trace_id.as_ref()?;
        if trace_id.is_empty() {
            return None;
        }

        let timestamp = event.metadata.timestamp.0;
        let span_type = event.data.event_type().to_string();
        let mut extra = HashMap::new();
        if let Some(ref sid) = event.metadata.session_id {
            extra.insert("session_id".into(), sid.clone());
        }
        if let Some(ref uid) = event.metadata.user_id {
            extra.insert("user_id".into(), uid.clone());
        }

        // span.extra 统一在末尾填 extra（避免每个 arm move extra）
        let extra_empty = || HashMap::new();
        let mut span = match &event.data {
            AgentEventData::UserMessage { message } => SpanData {
                span_type,
                name: "user_message".into(),
                input: Some(message.clone()),
                output: None,
                duration_ms: None,
                tokens: None,
                timestamp,
                success: None,
                extra: extra_empty(),
            },

            AgentEventData::AgentMatched {
                agent_id,
                agent_name,
                match_score,
                candidates,
            } => {
                extra.insert("agent_id".into(), agent_id.clone());
                extra.insert("match_score".into(), match_score.to_string());
                extra.insert("candidates".into(), candidates.join(","));
                SpanData {
                    span_type,
                    name: agent_name.clone(),
                    input: None,
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::ChainSelected {
                chain_name,
                strategy,
            } => {
                extra.insert("strategy".into(), strategy.clone());
                SpanData {
                    span_type,
                    name: chain_name.clone(),
                    input: None,
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::AgentStarted { agent_id, input } => {
                extra.insert("agent_id".into(), agent_id.clone());
                SpanData {
                    span_type,
                    name: agent_id.clone(),
                    input: Some(input.clone()),
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::AgentCompleted {
                agent_id,
                output,
                duration_ms,
            } => {
                extra.insert("agent_id".into(), agent_id.clone());
                SpanData {
                    span_type,
                    name: agent_id.clone(),
                    input: None,
                    output: Some(output.clone()),
                    duration_ms: Some(*duration_ms),
                    tokens: None,
                    timestamp,
                    success: Some(true),
                    extra: extra_empty(),
                }
            }

            AgentEventData::AgentFailed {
                agent_id,
                error,
                duration_ms,
            } => {
                extra.insert("agent_id".into(), agent_id.clone());
                extra.insert("error".into(), error.clone());
                SpanData {
                    span_type,
                    name: agent_id.clone(),
                    input: None,
                    output: None,
                    duration_ms: Some(*duration_ms),
                    tokens: None,
                    timestamp,
                    success: Some(false),
                    extra: extra_empty(),
                }
            }

            AgentEventData::FlowStarted { flow_type, input } => SpanData {
                span_type,
                name: flow_type.clone(),
                input: Some(input.clone()),
                output: None,
                duration_ms: None,
                tokens: None,
                timestamp,
                success: None,
                extra: extra_empty(),
            },

            AgentEventData::FlowStepExecuted {
                step_index,
                step_name,
                result,
            } => {
                extra.insert("step_index".into(), step_index.to_string());
                SpanData {
                    span_type,
                    name: step_name.clone(),
                    input: None,
                    output: Some(result.clone()),
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::FlowCompleted { output, iterations } => {
                extra.insert("iterations".into(), iterations.to_string());
                SpanData {
                    span_type,
                    name: "flow".into(),
                    input: None,
                    output: Some(output.clone()),
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::GraphStarted {
                graph_name,
                run_id,
                entry_node,
            } => {
                extra.insert("run_id".into(), run_id.clone());
                extra.insert("entry_node".into(), entry_node.clone());
                SpanData {
                    span_type,
                    name: graph_name.clone(),
                    input: None,
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::NodeExecuteRequested {
                run_id,
                node_name,
                step,
                state: _,
            } => {
                extra.insert("run_id".into(), run_id.clone());
                extra.insert("step".into(), step.to_string());
                SpanData {
                    span_type,
                    name: node_name.clone(),
                    input: None,
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::NodeCompleted {
                run_id,
                node_name,
                output,
                success,
                duration_ms,
                next_nodes,
                state_updates: _,
            } => {
                extra.insert("run_id".into(), run_id.clone());
                extra.insert("next_nodes".into(), next_nodes.join(","));
                SpanData {
                    span_type,
                    name: node_name.clone(),
                    input: None,
                    output: Some(output.clone()),
                    duration_ms: Some(*duration_ms),
                    tokens: None,
                    timestamp,
                    success: Some(*success),
                    extra: extra_empty(),
                }
            }

            AgentEventData::NodeFailed {
                run_id,
                node_name,
                error,
                duration_ms,
            } => {
                extra.insert("run_id".into(), run_id.clone());
                extra.insert("error".into(), error.clone());
                SpanData {
                    span_type,
                    name: node_name.clone(),
                    input: None,
                    output: None,
                    duration_ms: Some(*duration_ms),
                    tokens: None,
                    timestamp,
                    success: Some(false),
                    extra: extra_empty(),
                }
            }

            AgentEventData::GraphCompleted {
                run_id,
                success,
                total_steps,
                duration_ms,
            } => {
                extra.insert("run_id".into(), run_id.clone());
                extra.insert("total_steps".into(), total_steps.to_string());
                SpanData {
                    span_type,
                    name: "graph".into(),
                    input: None,
                    output: None,
                    duration_ms: Some(*duration_ms),
                    tokens: None,
                    timestamp,
                    success: Some(*success),
                    extra: extra_empty(),
                }
            }

            // LLM/Tool/Memory/Span 事件也可扩展，当前方案聚焦 orchestrator + graph，留空
            _ => return None,
        };
        // 统一填 extra（包含 trace_id 追加）
        extra.insert("trace_id".into(), trace_id.clone());
        span.extra = extra;
        Some(span)
    }
}

#[async_trait]
impl EventHandler for TraceEventBridge {
    fn name(&self) -> &str {
        "trace_event_bridge"
    }

    fn filter(&self) -> EventFilter {
        // 处理全部；无 trace_id 的在 convert 中过滤
        EventFilter::All
    }

    async fn handle(&self, event: &Event) {
        if let Some(ref trace_id) = event.metadata.trace_id {
            if trace_id.is_empty() {
                return;
            }
            if let Some(span) = Self::convert(event) {
                self.trace_observer.record_span(trace_id, span);
            }
        }
    }
}
