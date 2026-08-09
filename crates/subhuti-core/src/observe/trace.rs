//! # Trace Interface
//!
//! 追踪接口定义。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub id: (String,),
    pub user_id: String,
    pub session_id: String,
    pub input: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub events: Vec<TraceEvent>,
    pub total_duration_ms: Option<u64>,
    pub chain_name: Option<String>,
    pub expert_chain: Vec<String>,
    pub status: crate::observe::TraceStatus,
    pub output: Option<String>,
}

impl Trace {
    pub fn new(user_id: &str, session_id: &str, input: &str) -> Self {
        Self {
            id: (uuid::Uuid::new_v4().to_string(),),
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            input: input.to_string(),
            timestamp: chrono::Utc::now(),
            events: Vec::new(),
            total_duration_ms: None,
            chain_name: None,
            expert_chain: Vec::new(),
            status: crate::observe::TraceStatus::InProgress,
            output: None,
        }
    }

    pub fn add_event(&mut self, event: TraceEvent) {
        self.events.push(event);
    }

    pub fn complete_success(&mut self, _output: String, _duration_ms: u64) {}

    pub fn complete_failed(&mut self, _error: String, _duration_ms: u64) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub name: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub data: serde_json::Value,
}

impl TraceEvent {
    pub fn new(name: &str, data: serde_json::Value) -> Self {
        Self {
            name: name.to_string(),
            timestamp: chrono::Utc::now(),
            data,
        }
    }
}
