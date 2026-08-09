//! # Session Management
//!
//! 会话管理接口定义。

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use super::llm::{Message, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_messages: usize,
    pub system_prompt: Option<String>,
    pub temperature: f32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_messages: 50,
            system_prompt: None,
            temperature: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    messages: VecDeque<Message>,
    config: SessionConfig,
    metadata: std::collections::HashMap<String, serde_json::Value>,
}

impl Session {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            messages: VecDeque::new(),
            config: SessionConfig::default(),
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn messages(&self) -> Vec<Message> {
        self.messages.iter().cloned().collect()
    }

    pub fn add_message(&mut self, role: Role, content: &str) {
        self.messages.push_back(Message {
            role,
            content: content.to_string(),
            tool_call_id: None,
        });

        while self.messages.len() > self.config.max_messages {
            self.messages.pop_front();
        }
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.config.system_prompt.as_deref()
    }

    pub fn set_system_prompt(&mut self, prompt: &str) {
        self.config.system_prompt = Some(prompt.to_string());
    }

    pub fn set_metadata(&mut self, key: &str, value: serde_json::Value) {
        self.metadata.insert(key.to_string(), value);
    }

    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }
}
