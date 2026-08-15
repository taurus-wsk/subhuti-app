//! # Trace 事件桥
//!
//! 实现框架 `EventHandler` trait，订阅框架 EventBus，
//! 将带 trace_id 的事件转换为应用层 `SpanData`，并通过 `TraceObserverPort.record_span`
//! 写入 SubhutiTraceObserverAdapter.spans HashMap，供 get_span_tree 查询组装树。
//!
//! 六边形架构：
//! - 属于 outbound adapter（适配框架 EventHandler → 应用层 TraceObserverPort）
//! - 依赖：框架层 subhuti_core::event::EventHandler、应用层 TraceObserverPort/SpanData

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::event::{AgentEventData, Event, EventFilter, EventHandler};

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
                actor_name: _,
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

            // ── LLM 层事件 ──
            AgentEventData::LLMCalling {
                messages_count,
                model,
            } => {
                extra.insert("messages_count".into(), messages_count.to_string());
                if let Some(m) = model {
                    extra.insert("model".into(), m.clone());
                }
                SpanData {
                    span_type,
                    name: "llm_call".into(),
                    input: None,
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::LLMResponded {
                response,
                tokens_used,
                duration_ms,
            } => {
                extra.insert("tokens_used".into(), tokens_used.to_string());
                SpanData {
                    span_type,
                    name: "llm_response".into(),
                    input: None,
                    output: Some(response.clone()),
                    duration_ms: Some(*duration_ms),
                    tokens: Some(*tokens_used),
                    timestamp,
                    success: Some(true),
                    extra: extra_empty(),
                }
            }

            // ── 工具层事件 ──
            AgentEventData::ToolCalling { tool_name, args } => {
                extra.insert("tool_args".into(), args.to_string());
                SpanData {
                    span_type,
                    name: tool_name.clone(),
                    input: Some(args.to_string()),
                    output: None,
                    duration_ms: None,
                    tokens: None,
                    timestamp,
                    success: None,
                    extra: extra_empty(),
                }
            }

            AgentEventData::ToolResponded {
                tool_name,
                result,
                success,
                duration_ms,
            } => SpanData {
                span_type,
                name: tool_name.clone(),
                input: None,
                output: Some(result.clone()),
                duration_ms: Some(*duration_ms),
                tokens: None,
                timestamp,
                success: Some(*success),
                extra: extra_empty(),
            },

            // Memory/Span 等事件暂不处理，后续扩展
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

#[cfg(test)]
mod tests {
    use super::*;
    use subhuti_core::event::EventMetadata;

    fn make_event(data: AgentEventData, trace_id: &str, session_id: Option<&str>) -> Event {
        let mut meta = EventMetadata::new();
        meta.trace_id = Some(trace_id.to_string());
        meta.session_id = session_id.map(|s| s.to_string());
        Event {
            metadata: meta,
            data,
        }
    }

    #[test]
    fn test_convert_llm_calling() {
        let event = make_event(
            AgentEventData::LLMCalling {
                messages_count: 2,
                model: Some("gpt-4".into()),
            },
            "trace-123",
            Some("session-456"),
        );
        let span = TraceEventBridge::convert(&event).expect("LLMCalling 不应返回 None");
        assert_eq!(span.span_type, "llm_calling");
        assert_eq!(span.name, "llm_call");
        assert_eq!(span.success, None);
        assert_eq!(span.duration_ms, None);
        assert_eq!(span.tokens, None);
        assert_eq!(
            span.extra.get("trace_id").map(|s| s.as_str()),
            Some("trace-123")
        );
        assert_eq!(
            span.extra.get("messages_count").map(|s| s.as_str()),
            Some("2")
        );
        assert_eq!(span.extra.get("model").map(|s| s.as_str()), Some("gpt-4"));
    }

    #[test]
    fn test_convert_llm_responded() {
        let event = make_event(
            AgentEventData::LLMResponded {
                response: "Hello!".into(),
                tokens_used: 42,
                duration_ms: 1500,
            },
            "trace-123",
            None,
        );
        let span = TraceEventBridge::convert(&event).expect("LLMResponded 不应返回 None");
        assert_eq!(span.span_type, "llm_responded");
        assert_eq!(span.name, "llm_response");
        assert_eq!(span.output, Some("Hello!".into()));
        assert_eq!(span.duration_ms, Some(1500));
        assert_eq!(span.tokens, Some(42));
        assert_eq!(span.success, Some(true));
    }

    #[test]
    fn test_convert_tool_calling() {
        let event = make_event(
            AgentEventData::ToolCalling {
                tool_name: "write_files".into(),
                args: serde_json::json!({"path": "/tmp/test"}),
            },
            "trace-123",
            None,
        );
        let span = TraceEventBridge::convert(&event).expect("ToolCalling 不应返回 None");
        assert_eq!(span.span_type, "tool_calling");
        assert_eq!(span.name, "write_files");
        assert_eq!(span.success, None);
        assert_eq!(span.duration_ms, None);
    }

    #[test]
    fn test_convert_tool_responded() {
        let event = make_event(
            AgentEventData::ToolResponded {
                tool_name: "cargo_check".into(),
                result: "编译通过".into(),
                success: true,
                duration_ms: 500,
            },
            "trace-123",
            None,
        );
        let span = TraceEventBridge::convert(&event).expect("ToolResponded 不应返回 None");
        assert_eq!(span.span_type, "tool_responded");
        assert_eq!(span.name, "cargo_check");
        assert_eq!(span.output, Some("编译通过".into()));
        assert_eq!(span.duration_ms, Some(500));
        assert_eq!(span.success, Some(true));
    }

    #[test]
    fn test_convert_no_trace_id_returns_none() {
        let event = Event::new(AgentEventData::LLMCalling {
            messages_count: 1,
            model: None,
        });
        let span = TraceEventBridge::convert(&event);
        assert!(span.is_none(), "无 trace_id 应返回 None");
    }

    #[test]
    fn test_convert_other_events_return_none() {
        // MemoryWritten 事件应返回 None（暂未处理）
        let event = make_event(
            AgentEventData::MemoryWritten {
                key: "test".into(),
                category: "chat".into(),
            },
            "trace-123",
            None,
        );
        let span = TraceEventBridge::convert(&event);
        assert!(span.is_none(), "MemoryWritten 应返回 None（暂未处理）");
    }

    #[test]
    fn test_convert_empty_trace_id_returns_none() {
        let mut event = Event::new(AgentEventData::LLMCalling {
            messages_count: 1,
            model: None,
        });
        event.metadata.trace_id = Some("".into());
        let span = TraceEventBridge::convert(&event);
        assert!(span.is_none(), "空 trace_id 应返回 None");
    }
}
