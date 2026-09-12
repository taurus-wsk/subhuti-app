//! # Trace 事件桥
//!
//! 实现框架 `EventHandler` trait，订阅框架 EventBus，
//! 将带 trace_id 的事件转换为应用层 `SpanData`，并通过 `TraceObserverPort.record_span`
//! 写入观察者（InMemory / 共享 SQLite 两种后端），供 get_span_tree 查询组装树。
//!
//! 六边形架构：
//! - 属于 outbound adapter（适配框架 EventHandler → 应用层 TraceObserverPort）
//! - 依赖：框架层 subhuti_core::event::EventHandler、应用层 TraceObserverPort/SpanData

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::event::{AgentEventData, Event, EventFilter, EventHandler};

use crate::application::observer::{SpanData, TraceObserverPort};
use crate::application::ports::StreamEvent;
use crate::application::stream_registry;

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

/// # 进度事件桥（EventBus → SSE）
///
/// 订阅框架 EventBus，把携带 trace_id 的「框架动作事件」翻译成协议中立的
/// `StreamEvent::Step`（带 phase），按 `trace_id` 路由到对应请求的 SSE 通道。
///
/// 这一步把原本只对 trace 观察者可见的细粒度动作（专家匹配、LLM 推理、工具调用、
/// 记忆检索）也透传到流式输出，使前端能渲染出 WorkBuddy 式的阶段流
/// （route → think → tool → retrieve → … → done）。
///
/// 与 `TraceEventBridge` 的区别：
/// - `TraceEventBridge`：事件 → `SpanData` → trace 观察者（事后查询链路树）
/// - `ProgressEventBridge`：事件 → `StreamEvent::Step` → SSE（实时进度）
pub struct ProgressEventBridge;

impl ProgressEventBridge {
    pub fn new() -> Self {
        Self
    }

    /// 把框架事件翻译成流式 Step 事件（无对应 phase 的事件返回 None）
    fn to_step(data: &AgentEventData) -> Option<StreamEvent> {
        use AgentEventData::*;
        let (phase, message, expert) = match data {
            AgentMatched { agent_name, .. } => (
                "route",
                format!("🧭 匹配专家: {agent_name}"),
                Some(agent_name.clone()),
            ),
            LLMCalling { .. } => ("think", "🤔 模型推理中…".to_string(), None),
            ToolCalling { tool_name, .. } => ("tool", format!("🔧 调用工具: {tool_name}"), None),
            ToolResponded {
                tool_name, success, ..
            } => (
                "tool",
                format!(
                    "✅ 工具完成: {} ({})",
                    tool_name,
                    if *success { "成功" } else { "失败" }
                ),
                None,
            ),
            MemoryRetrieved {
                query,
                results_count,
            } => (
                "retrieve",
                format!("📚 检索记忆: {query} ({results_count} 条)"),
                None,
            ),
            // 其余事件（AgentStarted/Completed、LLMResponded 等）不在此桥渲染，
            // 避免与编排层自身发出的 run/done 阶段重复
            _ => return None,
        };
        Some(StreamEvent::Step {
            message,
            expert,
            phase: Some(phase.to_string()),
            todo_state: None,
        })
    }
}

#[async_trait]
impl EventHandler for ProgressEventBridge {
    fn name(&self) -> &str {
        "progress_event_bridge"
    }

    fn filter(&self) -> EventFilter {
        EventFilter::Types(vec![
            "agent_matched",
            "llm_calling",
            "tool_calling",
            "tool_responded",
            "memory_retrieved",
        ])
    }

    async fn handle(&self, event: &Event) {
        // 仅处理带 trace_id 的事件（否则无法路由到具体会话）
        let trace_id = match event.metadata.trace_id.as_ref() {
            Some(t) if !t.is_empty() => t,
            _ => return,
        };
        let tx = match stream_registry::get_stream_tx(trace_id) {
            Some(tx) => tx,
            None => return, // 该 trace 没有活跃流式会话（如 MCP 调用、离线 trace）
        };
        if let Some(step) = Self::to_step(&event.data) {
            // 非阻塞投递：通道满或已关闭则丢弃，绝不反向阻塞 EventBus
            let _ = tx.try_send(step);
        }
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn test_to_step_agent_matched() {
        let e = AgentEventData::AgentMatched {
            agent_id: "rust-expert".into(),
            agent_name: "Rust 编程专家".into(),
            match_score: 1.0,
            candidates: vec![],
        };
        let (phase, expert, message) =
            match ProgressEventBridge::to_step(&e).expect("应映射为 Step") {
                StreamEvent::Step {
                    phase,
                    expert,
                    message,
                    ..
                } => (phase, expert, message),
                _ => panic!("应为 Step 变体"),
            };
        assert_eq!(phase.as_deref(), Some("route"));
        assert_eq!(expert.as_deref(), Some("Rust 编程专家"));
        assert!(message.contains("Rust 编程专家"));
    }

    #[test]
    fn test_to_step_llm_calling_is_think() {
        let e = AgentEventData::LLMCalling {
            messages_count: 3,
            model: Some("zhipu".into()),
        };
        let (phase, expert) = match ProgressEventBridge::to_step(&e).expect("应映射为 Step") {
            StreamEvent::Step { phase, expert, .. } => (phase, expert),
            _ => panic!("应为 Step 变体"),
        };
        assert_eq!(phase.as_deref(), Some("think"));
        assert_eq!(expert, None);
    }

    #[test]
    fn test_to_step_agent_completed_is_none() {
        // AgentCompleted 不在本桥渲染范围内
        let e = AgentEventData::AgentCompleted {
            agent_id: "x".into(),
            output: "ok".into(),
            duration_ms: 1,
        };
        assert!(ProgressEventBridge::to_step(&e).is_none());
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
