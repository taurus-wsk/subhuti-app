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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::event::{AgentEventData, Event, EventFilter, EventHandler};

use crate::application::observer::{SpanData, TraceObserverPort};
use subhuti_core::progress::ProgressEvent;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

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

/// # 进度事件桥（EventBus → ProgressEvent，per-request 订阅）
///
/// **每次 `/orchestrate` 请求开始时**构造一个实例、订阅到框架 EventBus，
/// 把携带本请求 `trace_id` 的「框架动作事件」翻译成 `ProgressEvent::Step`（带 phase），
/// 直接 `try_send` 到本次请求专属的 `Sender<ProgressEvent>` 通道。
/// 该通道与专家经 `AgentContext.progress` 发出的 `ProgressEvent` 是同一条，
/// 由 `orchestrate_stream` 统一映射为前端 `StreamEvent`（取代旧版全局 stream_registry）。
///
/// 与早期实现的区别：不再依赖全局 `stream_registry`（trace_id → tx / agent 的
/// `OnceLock<Mutex<HashMap>>`）。每个请求自己持有 `tx` 与已匹配到的专家名，
/// 无全局可变状态、无手动 register/unregister、无跨请求泄漏/误投风险。
///
/// 与 `TraceEventBridge` 的区别：
/// - `TraceEventBridge`：事件 → `SpanData` → trace 观察者（事后查询链路树）
/// - `ProgressEventBridge`：事件 → `ProgressEvent::Step` → 统一进度流（仅本请求实时进度）
pub struct ProgressEventBridge {
    /// 本请求独有的 trace_id，用于从共享 EventBus 中挑出属于自己的事件
    trace_id: String,
    /// 本请求专属的进度发送端（与专家共用同一条 ProgressEvent 流）
    tx: Sender<ProgressEvent>,
    /// route 阶段（AgentMatched）记下的专家名，供后续 think/tool/retrieve 阶段回填 source
    agent: Mutex<Option<String>>,
    /// 本请求累计 token 用量（LLMResponded.tokens_used 求和），由调用方在 Done.meta 读出
    token_counter: Arc<AtomicU64>,
    /// 本请求 LLM 调用次数
    llm_calls: Arc<AtomicU64>,
}

impl ProgressEventBridge {
    pub fn new(
        trace_id: String,
        tx: Sender<ProgressEvent>,
        token_counter: Arc<AtomicU64>,
        llm_calls: Arc<AtomicU64>,
    ) -> Self {
        Self {
            trace_id,
            tx,
            agent: Mutex::new(None),
            token_counter,
            llm_calls,
        }
    }

    /// 把框架事件翻译成 ProgressEvent::Step（无对应 phase 的事件返回 None）
    ///
    /// `agent` 为已匹配到的专家名（route 阶段记录），用于给 think/tool/retrieve
    /// 阶段补全 source；查不到则回落为「框架」。
    fn to_step(agent: Option<&str>, data: &AgentEventData) -> Option<ProgressEvent> {
        use AgentEventData::*;
        let source = || agent.unwrap_or("框架").to_string();
        let (phase, message, source) = match data {
            AgentMatched { agent_name, .. } => (
                "route",
                format!("🧭 匹配专家: {agent_name}"),
                agent_name.clone(),
            ),
            LLMCalling { .. } => ("think", "🤔 模型推理中…".to_string(), source()),
            ToolCalling { tool_name, .. } => {
                ("tool", format!("🔧 调用工具: {tool_name}"), source())
            }
            ToolResponded {
                tool_name, success, ..
            } => (
                "tool",
                format!(
                    "✅ 工具完成: {} ({})",
                    tool_name,
                    if *success { "成功" } else { "失败" }
                ),
                source(),
            ),
            MemoryRetrieved {
                query,
                results_count,
            } => (
                "retrieve",
                format!("📚 检索记忆: {query} ({results_count} 条)"),
                source(),
            ),
            // FlowRunner 逐步执行：step_name 即 analyze/plan/edit/verify/done，
            // 以它为 phase 原样渲染，前端阶段徽标与旧分析→规划→...面容保持一致。
            FlowStepExecuted { step_name, .. } => ("flow", format!("📈 {}", step_name), source()),
            // 其余事件（AgentStarted/Completed、FlowStarted/FlowCompleted、LLMResponded 等）
            // 不在此桥渲染，避免与编排层自身发出的 run/done 阶段重复
            _ => return None,
        };
        Some(ProgressEvent::Step {
            message,
            source,
            phase: Some(phase.to_string()),
            todo_state: None,
            done: None,
            total: None,
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
            "llm_responded",
            "tool_calling",
            "tool_responded",
            "memory_retrieved",
            // FlowRunner 逐步执行的阶段徽标（analyze/plan/edit/verify/done）：
            // FlowRunner 发射 FlowStepExecuted，落为前端阶段渲染，保持阶段流不变。
            "flow_step_executed",
        ])
    }

    async fn handle(&self, event: &Event) {
        // 仅处理带 trace_id 的事件（否则无法路由到具体会话）
        let trace_id = match event.metadata.trace_id.as_ref() {
            Some(t) if !t.is_empty() => t,
            _ => return,
        };
        // 只处理本请求自己的事件：多个 per-request 桥共存时互不干扰
        if trace_id != &self.trace_id {
            return;
        }

        // LLMResponded 不渲染成步骤（避免与编排层 done 阶段重复），
        // 但要把 token 用量累加进本请求计数器，供 Done.meta 透出
        if let AgentEventData::LLMResponded { tokens_used, .. } = &event.data {
            self.token_counter
                .fetch_add(*tokens_used, Ordering::Relaxed);
            self.llm_calls.fetch_add(1, Ordering::Relaxed);
            return;
        }

        // route 阶段：记下专家名，供后续 think/tool/retrieve 阶段回填 source
        let mut agent_guard = self.agent.lock().await;
        if let AgentEventData::AgentMatched { agent_name, .. } = &event.data {
            *agent_guard = Some(agent_name.clone());
        }
        let agent = agent_guard.take();
        drop(agent_guard);

        if let Some(step) = Self::to_step(agent.as_deref(), &event.data) {
            // 非阻塞投递：通道满或已关闭则丢弃，绝不反向阻塞 EventBus
            let _ = self.tx.try_send(step);
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
        let (phase, source, message) =
            match ProgressEventBridge::to_step(None, &e).expect("应映射为 Step") {
                ProgressEvent::Step {
                    phase,
                    source,
                    message,
                    ..
                } => (phase, source, message),
                _ => panic!("应为 Step 变体"),
            };
        assert_eq!(phase.as_deref(), Some("route"));
        assert_eq!(source, "Rust 编程专家");
        assert!(message.contains("Rust 编程专家"));
    }

    #[test]
    fn test_to_step_llm_calling_is_think() {
        // 已记录专家名，验证 think 阶段能回填 source
        let e = AgentEventData::LLMCalling {
            messages_count: 3,
            model: Some("zhipu".into()),
        };
        let (phase, source) = match ProgressEventBridge::to_step(Some("Rust 编程专家"), &e)
            .expect("应映射为 Step")
        {
            ProgressEvent::Step { phase, source, .. } => (phase, source),
            _ => panic!("应为 Step 变体"),
        };
        assert_eq!(phase.as_deref(), Some("think"));
        assert_eq!(source, "Rust 编程专家");
    }

    #[test]
    fn test_to_step_agent_completed_is_none() {
        // AgentCompleted 不在本桥渲染范围内
        let e = AgentEventData::AgentCompleted {
            agent_id: "x".into(),
            output: "ok".into(),
            duration_ms: 1,
        };
        assert!(ProgressEventBridge::to_step(None, &e).is_none());
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
