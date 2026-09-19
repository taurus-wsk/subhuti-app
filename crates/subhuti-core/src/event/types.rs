//! # 事件类型定义
//!
//! Agent 生命周期中所有事件的类型。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::Arc;
use uuid::Uuid;

// ─── 事件标识 ──────────────────────────────────────────────

/// 事件唯一 ID
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub String);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

/// 事件时间戳
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTimestamp(pub DateTime<Utc>);

impl EventTimestamp {
    pub fn now() -> Self {
        Self(Utc::now())
    }
}

impl Default for EventTimestamp {
    fn default() -> Self {
        Self::now()
    }
}

/// 事件元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    /// 事件 ID
    pub id: EventId,
    /// 事件时间戳
    pub timestamp: EventTimestamp,
    /// 触发事件的 trace_id（关联 Trace 系统）
    pub trace_id: Option<String>,
    /// 触发事件的 session_id
    pub session_id: Option<String>,
    /// 触发事件的 user_id
    pub user_id: Option<String>,
}

impl EventMetadata {
    pub fn new() -> Self {
        Self {
            id: EventId::new(),
            timestamp: EventTimestamp::now(),
            trace_id: None,
            session_id: None,
            user_id: None,
        }
    }

    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self::new()
    }
}

// ─── 事件数据载荷 ──────────────────────────────────────────

/// Agent 生命周期事件数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEventData {
    // ── 编排层事件 ──
    /// 用户消息到达
    UserMessage { message: String },
    /// 专家匹配完成
    AgentMatched {
        agent_id: String,
        agent_name: String,
        match_score: f32,
        candidates: Vec<String>,
    },
    /// 策略链选择完成
    ChainSelected {
        chain_name: String,
        strategy: String,
    },
    /// Agent 执行开始
    AgentStarted { agent_id: String, input: String },
    /// Agent 执行完成
    AgentCompleted {
        agent_id: String,
        output: String,
        duration_ms: u64,
    },
    /// Agent 执行失败
    AgentFailed {
        agent_id: String,
        error: String,
        duration_ms: u64,
    },

    // ── Flow 层事件 ──
    /// Flow 执行开始
    FlowStarted { flow_type: String, input: String },
    /// Flow 步骤执行
    FlowStepExecuted {
        step_index: usize,
        step_name: String,
        result: String,
    },
    /// Flow 执行完成
    FlowCompleted { output: String, iterations: usize },

    // ── LLM 层事件 ──
    /// LLM 调用前
    LLMCalling {
        messages_count: usize,
        model: Option<String>,
    },
    /// LLM 响应后
    LLMResponded {
        response: String,
        tokens_used: u64,
        duration_ms: u64,
    },
    /// LLM 流式输出
    LLMStreamChunk { chunk: String },

    // ── 工具层事件 ──
    /// 工具调用前
    ToolCalling {
        tool_name: String,
        args: serde_json::Value,
    },
    /// 工具响应后
    ToolResponded {
        tool_name: String,
        result: String,
        success: bool,
        duration_ms: u64,
    },

    // ── 记忆层事件 ──
    /// 记忆写入
    MemoryWritten { key: String, category: String },
    /// 记忆检索
    MemoryRetrieved { query: String, results_count: usize },
}

// ─── 便捷类型别名 ──────────────────────────────────────────

/// 编排器事件
pub type OrchestratorEvent = AgentEventData;
/// Agent 事件
pub type AgentEvent = AgentEventData;
/// Flow 事件
pub type FlowEvent = AgentEventData;
/// LLM 事件
pub type LlmEvent = AgentEventData;
/// 工具事件
pub type ToolEvent = AgentEventData;
/// 记忆事件
pub type MemoryEvent = AgentEventData;
/// Trace 事件
pub type TraceEvent = AgentEventData;

// ─── 完整事件结构 ──────────────────────────────────────────

/// 完整的 Agent 事件（元数据 + 数据）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub metadata: EventMetadata,
    pub data: AgentEventData,
}

impl Event {
    pub fn new(data: AgentEventData) -> Self {
        Self {
            metadata: EventMetadata::new(),
            data,
        }
    }

    pub fn with_trace(mut self, trace_id: impl Into<String>) -> Self {
        self.metadata.trace_id = Some(trace_id.into());
        self
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.metadata.session_id = Some(session_id.into());
        self
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.metadata.user_id = Some(user_id.into());
        self
    }
}

/// 事件类型名（用于订阅过滤）
impl AgentEventData {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::UserMessage { .. } => "user_message",
            Self::AgentMatched { .. } => "agent_matched",
            Self::ChainSelected { .. } => "chain_selected",
            Self::AgentStarted { .. } => "agent_started",
            Self::AgentCompleted { .. } => "agent_completed",
            Self::AgentFailed { .. } => "agent_failed",
            Self::FlowStarted { .. } => "flow_started",
            Self::FlowStepExecuted { .. } => "flow_step_executed",
            Self::FlowCompleted { .. } => "flow_completed",
            Self::LLMCalling { .. } => "llm_calling",
            Self::LLMResponded { .. } => "llm_responded",
            Self::LLMStreamChunk { .. } => "llm_stream_chunk",
            Self::ToolCalling { .. } => "tool_calling",
            Self::ToolResponded { .. } => "tool_responded",
            Self::MemoryWritten { .. } => "memory_written",
            Self::MemoryRetrieved { .. } => "memory_retrieved",
        }
    }
}

/// 支持动态类型分发的事件（用于 EventHandler trait）
pub trait DynamicEvent: Any + Send + Sync + std::fmt::Debug {
    fn as_any(&self) -> &dyn Any;
}

impl DynamicEvent for Event {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 事件引用（Arc 包装，避免克隆大 payload）
pub type ArcEvent = Arc<Event>;
