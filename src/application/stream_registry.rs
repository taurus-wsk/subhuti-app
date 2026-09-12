//! # 流式通道注册表（按 trace_id 关联会话）
//!
//! 框架 EventBus 是全局广播，无法区分事件属于哪个请求。本注册表在每次
//! `orchestrate_stream` 请求开始时，把当前请求的 `trace_id` 映射到该请求专属的
//! `StreamEvent` 发送端 `tx`，使 `ProgressEventBridge` 能把框架事件按 `trace_id`
//! 路由到正确的 SSE 通道。
//!
//! 与 `domain_expert_adapter::PROGRESS_TX_REGISTRY`（按 session_id 关联 progress 文本）
//! 同构，但本表按 trace_id 关联**结构化流式事件**。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tokio::sync::mpsc::Sender;

use crate::application::ports::StreamEvent;

static STREAM_TX_REGISTRY: OnceLock<Mutex<HashMap<String, Sender<StreamEvent>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Sender<StreamEvent>>> {
    STREAM_TX_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 注册某次请求的流式发送端（请求开始时调用）
pub fn register_stream_tx(trace_id: &str, tx: Sender<StreamEvent>) {
    if let Ok(mut map) = registry().lock() {
        map.insert(trace_id.to_string(), tx);
    }
}

/// 注销（请求结束时调用，避免内存泄漏 / 误投递）
pub fn unregister_stream_tx(trace_id: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(trace_id);
    }
}

/// 按 trace_id 取出发送端（EventBus→SSE 桥在投递前调用）
pub fn get_stream_tx(trace_id: &str) -> Option<Sender<StreamEvent>> {
    registry()
        .lock()
        .ok()
        .and_then(|map| map.get(trace_id).cloned())
}
