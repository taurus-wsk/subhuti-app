//! # 观察者适配器
//!
//! 实现 TraceObserverPort / SessionObserverPort，自包含内存存储。
//!
//! 设计说明：
//! - 框架层（crates/subhuti）的 TraceObserver/SessionObserver 是空 stub，
//!   store_trace 是 `{}`、list_summaries 返回 `Vec::new()`，数据无法落库。
//! - 本适配器在应用层自包含存储（`Mutex<Vec<...>>`），不再转发给空框架实现，
//!   让 `/traces` 和 `/sessions` 路由能查到真实数据。
//! - 临界区极短（push/iter），使用 `std::sync::Mutex`（Port 方法是同步签名）。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::application::{
    SessionObserverPort, SessionRecordParams, SpanData, TraceHandle, TraceObserverPort,
};

/// 追踪观察者适配器（自包含内存存储）
///
/// 实现 TraceObserverPort，将追踪记录存入内存 Vec，
/// 供 `/traces` 路由查询。不再依赖框架空实现。
///
/// `spans` 字段：存储由 TraceEventBridge 从框架事件转换来的细粒度 span，
/// key = trace_id，value = 按 timestamp 升序 push 的 SpanData 列表。
pub struct SubhutiTraceObserverAdapter {
    traces: Mutex<Vec<TraceHandle>>,
    spans: Mutex<HashMap<String, Vec<SpanData>>>,
}

impl SubhutiTraceObserverAdapter {
    /// 创建新的追踪观察者适配器
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(Vec::new()),
            spans: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for SubhutiTraceObserverAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceObserverPort for SubhutiTraceObserverAdapter {
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle {
        // 应用层自己生成 trace_id，不依赖框架
        let trace_id = uuid::Uuid::new_v4().to_string();
        TraceHandle::new(
            trace_id,
            user_id.to_string(),
            session_id.to_string(),
            message.to_string(),
        )
    }

    fn store_trace(&self, trace: TraceHandle) {
        let mut guard = self.traces.lock().expect("trace mutex poisoned");
        guard.push(trace);
    }

    fn record_span(&self, trace_id: &str, span: SpanData) {
        let mut guard = self.spans.lock().expect("span mutex poisoned");
        guard.entry(trace_id.to_string()).or_default().push(span);
    }

    fn list_summaries(&self) -> Vec<serde_json::Value> {
        let guard = self.traces.lock().expect("trace mutex poisoned");
        guard
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.trace_id,
                    "user_id": t.user_id(),
                    "session_id": t.session_id(),
                    "input": t.message(),
                    "output": t.output(),
                    "status": format!("{:?}", t.status()),
                    "total_duration_ms": t.duration_ms(),
                    "chain_name": t.chain_name(),
                    "expert_chain": t.expert_chain_list(),
                })
            })
            .collect()
    }

    fn get_trace(&self, id: &str) -> Option<serde_json::Value> {
        let guard = self.traces.lock().expect("trace mutex poisoned");
        guard.iter().find(|t| t.trace_id == id).map(|t| {
            serde_json::json!({
                "id": t.trace_id,
                "user_id": t.user_id(),
                "session_id": t.session_id(),
                "input": t.message(),
                "output": t.output(),
                "error": t.error(),
                "total_duration_ms": t.duration_ms(),
                "chain_name": t.chain_name(),
                "expert_chain": t.expert_chain_list(),
                "status": format!("{:?}", t.status()),
            })
        })
    }

    fn get_span_tree(&self, id: &str) -> Option<serde_json::Value> {
        let spans_guard = self.spans.lock().expect("span mutex poisoned");
        let spans = spans_guard.get(id)?;

        let mut sorted = spans.clone();
        sorted.sort_by_key(|s| s.timestamp);

        let n = sorted.len();
        if n == 0 {
            return None;
        }

        // 用 parent 索引数组记录每个 span 的父节点（避免可变引用借用问题）
        // 嵌套规则（基于 span_type 语义）：
        //   user_message → root（无 parent）
        //   chain_selected / agent_matched → root child，记为 chain_idx
        //   agent_started / agent_completed / agent_failed → chain_idx child
        //   graph_started → chain_idx child，记为 graph_idx
        //   node_execute_requested / node_completed / node_failed → graph_idx child
        //   graph_completed → graph_idx child，然后 graph_idx = None
        //   flow_started → chain_idx child，记为 flow_idx
        //   flow_step_executed → flow_idx child
        //   flow_completed → flow_idx child，然后 flow_idx = None
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut root_idx: Option<usize> = None;
        let mut chain_idx: Option<usize> = None;
        let mut graph_idx: Option<usize> = None;
        let mut flow_idx: Option<usize> = None;

        for (i, span) in sorted.iter().enumerate() {
            match span.span_type.as_str() {
                "user_message" => {
                    root_idx = Some(i);
                }
                "chain_selected" | "agent_matched" => {
                    parent[i] = root_idx;
                    chain_idx = Some(i);
                }
                "agent_started" | "agent_completed" | "agent_failed" => {
                    parent[i] = chain_idx.or(root_idx);
                }
                "graph_started" => {
                    parent[i] = chain_idx.or(root_idx);
                    graph_idx = Some(i);
                }
                "node_execute_requested" | "node_completed" | "node_failed" => {
                    parent[i] = graph_idx.or(chain_idx).or(root_idx);
                }
                "graph_completed" => {
                    parent[i] = graph_idx.or(chain_idx).or(root_idx);
                    graph_idx = None;
                }
                "flow_started" => {
                    parent[i] = chain_idx.or(root_idx);
                    flow_idx = Some(i);
                }
                "flow_step_executed" => {
                    parent[i] = flow_idx.or(chain_idx).or(root_idx);
                }
                "flow_completed" => {
                    parent[i] = flow_idx.or(chain_idx).or(root_idx);
                    flow_idx = None;
                }
                _ => {
                    parent[i] = graph_idx.or(flow_idx).or(chain_idx).or(root_idx);
                }
            }
        }

        // 如果没有 user_message，追加一个 fake root（索引 n），所有 parent=None 的挂到它下面
        let (all_spans, effective_parent) = if root_idx.is_none() {
            let fake = SpanData {
                span_type: "trace".into(),
                name: id.to_string(),
                input: None,
                output: None,
                duration_ms: None,
                tokens: None,
                timestamp: sorted[0].timestamp,
                success: None,
                extra: HashMap::new(),
            };
            let mut all = sorted.clone();
            all.push(fake);
            let mut p = parent.clone();
            for i in 0..n {
                if p[i].is_none() {
                    p[i] = Some(n);
                }
            }
            (all, p)
        } else {
            (sorted.clone(), parent)
        };

        // 递归构建 JSON 树
        fn build_json(
            idx: usize,
            spans: &[SpanData],
            parent: &[Option<usize>],
        ) -> serde_json::Value {
            let span = &spans[idx];
            let children: Vec<serde_json::Value> = (0..spans.len())
                .filter(|&j| parent[j] == Some(idx))
                .map(|j| build_json(j, spans, parent))
                .collect();
            serde_json::json!({
                "span_type": span.span_type,
                "name": span.name,
                "timestamp": span.timestamp,
                "duration_ms": span.duration_ms,
                "input": span.input,
                "output": span.output,
                "success": span.success,
                "tokens": span.tokens,
                "extra": &span.extra,
                "children": children,
            })
        }

        // 找到 root（parent 为 None 的节点）
        let root_index = (0..all_spans.len()).find(|&i| effective_parent[i].is_none())?;
        Some(build_json(root_index, &all_spans, &effective_parent))
    }
}

/// 会话观察者适配器（自包含内存存储）
///
/// 实现 SessionObserverPort，将会话记录存入内存 Vec，
/// 供 `/sessions` 路由查询。
pub struct SubhutiSessionObserverAdapter {
    sessions: Mutex<Vec<SessionRecordParams>>,
}

impl SubhutiSessionObserverAdapter {
    /// 创建新的会话观察者适配器
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
        }
    }
}

impl Default for SubhutiSessionObserverAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionObserverPort for SubhutiSessionObserverAdapter {
    fn record_request(&self, params: SessionRecordParams) {
        let mut guard = self.sessions.lock().expect("session mutex poisoned");
        guard.push(params);
    }

    fn list_sessions(&self) -> Vec<serde_json::Value> {
        let guard = self.sessions.lock().expect("session mutex poisoned");
        guard
            .iter()
            .map(|s| {
                serde_json::json!({
                    "session_id": s.session_id,
                    "user_id": s.user_id,
                    "trace_id": s.trace_id,
                    "input": s.input,
                    "output": s.output,
                    "duration_ms": s.duration_ms,
                    "status": s.status,
                    "timestamp": s.timestamp,
                })
            })
            .collect()
    }

    fn get_session(&self, id: &str) -> Option<serde_json::Value> {
        let guard = self.sessions.lock().expect("session mutex poisoned");
        guard.iter().find(|s| s.session_id == id).map(|s| {
            serde_json::json!({
                "session_id": s.session_id,
                "user_id": s.user_id,
                "trace_id": s.trace_id,
                "input": s.input,
                "output": s.output,
                "duration_ms": s.duration_ms,
                "status": s.status,
                "timestamp": s.timestamp,
            })
        })
    }
}
