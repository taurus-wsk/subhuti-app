//! # 观察者适配器
//!
//! 实现 TraceObserverPort / SessionObserverPort。
//!
//! ## 两种 trace 后端
//!
//! - `InMemoryTraceObserverAdapter`：进程内内存存储（默认降级后端）。
//! - `SqliteTraceObserverAdapter`：共享 SQLite 持久化（默认后端），
//!   HTTP 进程与 MCP 进程写入同一个文件，trace 自然汇聚，可在任意进程的 `/traces` 查到。
//!
//! span 树 / 函数调用树的组装逻辑抽成 `build_span_tree` / `build_fn_call_tree`
//! 自由函数，两种后端共用，避免重复。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use uuid::Uuid;

use crate::application::{
    FnCallData, LogEntry, SessionObserverPort, SessionRecordParams, SpanData, TraceHandle,
    TraceObserverPort,
};
use subhuti_infra::trace_store::SqliteTraceStore;

// ───────────────────────────────────────────────────────────────────
// 内存版（降级后端）
// ───────────────────────────────────────────────────────────────────

/// 追踪观察者适配器（进程内内存存储，降级后端）
pub struct InMemoryTraceObserverAdapter {
    traces: Mutex<Vec<TraceHandle>>,
    spans: Mutex<HashMap<String, Vec<SpanData>>>,
    fn_calls: Mutex<HashMap<String, Vec<FnCallData>>>,
    fn_logs: Mutex<HashMap<String, Vec<LogEntry>>>,
}

impl InMemoryTraceObserverAdapter {
    pub fn new() -> Self {
        Self {
            traces: Mutex::new(Vec::new()),
            spans: Mutex::new(HashMap::new()),
            fn_calls: Mutex::new(HashMap::new()),
            fn_logs: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTraceObserverAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TraceObserverPort for InMemoryTraceObserverAdapter {
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle {
        TraceHandle::new(
            Uuid::new_v4().to_string(),
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

    fn total_tokens(&self, trace_id: &str) -> u64 {
        let guard = self.spans.lock().expect("span mutex poisoned");
        guard
            .get(trace_id)
            .map(|spans| spans.iter().map(|s| s.tokens.unwrap_or(0)).sum())
            .unwrap_or(0)
    }

    fn record_fn_call(&self, trace_id: &str, fn_call: FnCallData) {
        let mut guard = self.fn_calls.lock().expect("fn_call mutex poisoned");
        guard.entry(trace_id.to_string()).or_default().push(fn_call);
    }

    fn record_fn_log(&self, trace_id: &str, log: LogEntry) {
        let mut guard = self.fn_logs.lock().expect("fn_log mutex poisoned");
        guard.entry(trace_id.to_string()).or_default().push(log);
    }

    fn get_fn_logs(&self, trace_id: &str) -> Vec<LogEntry> {
        let guard = self.fn_logs.lock().expect("fn_log mutex poisoned");
        guard.get(trace_id).cloned().unwrap_or_default()
    }

    fn get_fn_call_tree(&self, trace_id: &str) -> Option<serde_json::Value> {
        let calls = self
            .fn_calls
            .lock()
            .expect("fn_call mutex poisoned")
            .get(trace_id)
            .cloned()
            .unwrap_or_default();
        let logs = self
            .fn_logs
            .lock()
            .expect("fn_log mutex poisoned")
            .get(trace_id)
            .cloned()
            .unwrap_or_default();
        build_fn_call_tree(&calls, &logs, trace_id)
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
        let spans = self
            .spans
            .lock()
            .expect("span mutex poisoned")
            .get(id)
            .cloned()
            .unwrap_or_default();
        build_span_tree(&spans)
    }
}

// ───────────────────────────────────────────────────────────────────
// SQLite 持久化版（默认后端，跨进程汇聚）
// ───────────────────────────────────────────────────────────────────

/// 追踪观察者适配器（共享 SQLite 持久化）
///
/// 包装 `SqliteTraceStore`：写操作 fire-and-forget（发往后台 worker 线程），
/// 读操作阻塞在 oneshot 上取回结果。两个进程（HTTP / MCP）写入同一文件，trace 汇聚。
pub struct SqliteTraceObserverAdapter {
    store: Arc<SqliteTraceStore>,
}

impl SqliteTraceObserverAdapter {
    pub fn new(store: Arc<SqliteTraceStore>) -> Self {
        Self { store }
    }
}

impl TraceObserverPort for SqliteTraceObserverAdapter {
    fn create_trace(&self, user_id: &str, session_id: &str, message: &str) -> TraceHandle {
        TraceHandle::new(
            Uuid::new_v4().to_string(),
            user_id.to_string(),
            session_id.to_string(),
            message.to_string(),
        )
    }

    fn store_trace(&self, trace: TraceHandle) {
        // 阻塞落盘：保证 trace 摘要行在请求结束后可查（每个请求仅一次）
        self.store.write_trace_sync(trace);
    }

    fn record_span(&self, trace_id: &str, span: SpanData) {
        self.store
            .write_span_fire_and_forget(trace_id.to_string(), span);
    }

    fn total_tokens(&self, trace_id: &str) -> u64 {
        let spans = self.store.read_spans_sync(trace_id);
        spans.iter().map(|s| s.tokens.unwrap_or(0)).sum()
    }

    fn record_fn_call(&self, trace_id: &str, fn_call: FnCallData) {
        self.store
            .write_fn_call_fire_and_forget(trace_id.to_string(), fn_call);
    }

    fn record_fn_log(&self, trace_id: &str, log: LogEntry) {
        self.store
            .write_fn_log_fire_and_forget(trace_id.to_string(), log);
    }

    fn get_fn_logs(&self, trace_id: &str) -> Vec<LogEntry> {
        self.store.read_fn_logs_sync(trace_id)
    }

    fn get_fn_call_tree(&self, trace_id: &str) -> Option<Value> {
        let calls = self.store.read_fn_calls_sync(trace_id);
        let logs = self.store.read_fn_logs_sync(trace_id);
        build_fn_call_tree(&calls, &logs, trace_id)
    }

    fn list_summaries(&self) -> Vec<Value> {
        self.store.read_summaries_sync()
    }

    fn get_trace(&self, id: &str) -> Option<Value> {
        self.store.read_trace_sync(id)
    }

    fn get_span_tree(&self, id: &str) -> Option<Value> {
        let spans = self.store.read_spans_sync(id);
        build_span_tree(&spans)
    }
}

// ───────────────────────────────────────────────────────────────────
// 会话观察者（仍用内存，未要求持久化；如需跨进程请用同模式扩展）
// ───────────────────────────────────────────────────────────────────

/// 会话观察者适配器（内存存储）
pub struct SubhutiSessionObserverAdapter {
    sessions: Mutex<Vec<SessionRecordParams>>,
}

impl SubhutiSessionObserverAdapter {
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

// ───────────────────────────────────────────────────────────────────
// 树组装（两种后端共用）
// ───────────────────────────────────────────────────────────────────

/// 由 span 列表组装 span 树（JSON）。无 span 时返回 None。
pub(crate) fn build_span_tree(spans: &[SpanData]) -> Option<Value> {
    let mut sorted = spans.to_vec();
    sorted.sort_by_key(|s| s.timestamp);

    let n = sorted.len();
    if n == 0 {
        return None;
    }

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

    let (all_spans, effective_parent) = if root_idx.is_none() {
        let fake = SpanData {
            span_type: "trace".into(),
            name: String::new(),
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
        for slot in p.iter_mut() {
            if slot.is_none() {
                *slot = Some(n);
            }
        }
        (all, p)
    } else {
        (sorted.clone(), parent)
    };

    fn build_json(idx: usize, spans: &[SpanData], parent: &[Option<usize>]) -> Value {
        let span = &spans[idx];
        let children: Vec<Value> = (0..spans.len())
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

    let root_index = (0..all_spans.len()).find(|&i| effective_parent[i].is_none())?;
    Some(build_json(root_index, &all_spans, &effective_parent))
}

/// 由函数调用列表 + 日志组装函数调用树（JSON）。
pub(crate) fn build_fn_call_tree(
    calls: &[FnCallData],
    logs: &[LogEntry],
    trace_id: &str,
) -> Option<Value> {
    let mut sorted = calls.to_vec();
    sorted.sort_by_key(|c| c.timestamp);

    fn logs_for_fn(logs: &[LogEntry], fn_name: &str) -> Vec<Value> {
        logs.iter()
            .filter(|l| l.fn_name.as_deref() == Some(fn_name))
            .map(|l| {
                serde_json::json!({
                    "level": l.level.to_string(),
                    "message": l.message,
                    "timestamp": l.timestamp.to_rfc3339(),
                })
            })
            .collect()
    }

    fn build_children(parent_name: &str, calls: &[FnCallData], logs: &[LogEntry]) -> Vec<Value> {
        calls
            .iter()
            .filter(|c| c.parent_fn_name.as_deref() == Some(parent_name))
            .map(|c| {
                let children = build_children(&c.fn_name, calls, logs);
                let node_logs = logs_for_fn(logs, &c.fn_name);
                let mut json = serde_json::json!({
                    "fn_name": c.fn_name,
                    "input": c.input,
                    "output": c.output,
                    "input_bytes": c.input_bytes,
                    "output_bytes": c.output_bytes,
                    "duration_ms": c.duration_ms,
                    "success": c.success,
                    "memory_entry": c.memory_entry,
                    "memory_exit": c.memory_exit,
                    "memory_diff": c.memory_entry.zip(c.memory_exit).map(|(en, ex)| en.abs_diff(ex)),
                    "logs": node_logs,
                    "extra": c.extra,
                });
                if !children.is_empty() {
                    json["children"] = serde_json::Value::Array(children);
                }
                json
            })
            .collect()
    }

    let roots: Vec<Value> = sorted
        .iter()
        .filter(|c| c.parent_fn_name.is_none())
        .map(|c| {
            let children = build_children(&c.fn_name, &sorted, logs);
            let node_logs = logs_for_fn(logs, &c.fn_name);
            let mut json = serde_json::json!({
                "fn_name": c.fn_name,
                "input": c.input,
                "output": c.output,
                "input_bytes": c.input_bytes,
                "output_bytes": c.output_bytes,
                "duration_ms": c.duration_ms,
                "success": c.success,
                "memory_entry": c.memory_entry,
                "memory_exit": c.memory_exit,
                "memory_diff": c.memory_entry.zip(c.memory_exit).map(|(en, ex)| en.abs_diff(ex)),
                "logs": node_logs,
                "extra": c.extra,
            });
            if !children.is_empty() {
                json["children"] = serde_json::Value::Array(children);
            }
            json
        })
        .collect();

    Some(serde_json::json!({
        "trace_id": trace_id,
        "fn_calls": roots,
    }))
}
