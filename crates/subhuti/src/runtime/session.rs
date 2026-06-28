//! # Session - 会话管理
//!
//! 所有状态归 Session，Runtime 无状态（可水平扩展）
//!
//! ## 滑动窗口设计
//!
//! - 短期工作记忆：Session 内部的消息队列，限制容量
//! - 超额自动归档：当消息超出容量时，返回待归档的消息对

use super::llm::{Message, Role};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// 会话配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    /// 会话 ID
    pub session_id: Option<String>,
    /// 用户 ID
    pub user_id: Option<String>,
    /// 系统提示词
    pub system_prompt: Option<String>,
    /// 短期记忆滑动窗口容量（消息对数量，默认 3 对 = 6 条消息）
    pub short_term_capacity: usize,
    /// 是否启用自动归档
    pub auto_archive: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_id: None,
            user_id: None,
            system_prompt: None,
            short_term_capacity: 3, // 默认 3 对消息，约 3 轮对话
            auto_archive: true,
        }
    }
}

/// 待归档的消息对
#[derive(Debug, Clone)]
pub struct ArchivedMessagePair {
    pub user_message: String,
    pub assistant_message: String,
    pub timestamp: DateTime<Utc>,
}

/// 会话状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// 等待输入
    Idle,
    /// 思考中
    Thinking,
    /// 执行工具中
    Acting,
    /// 完成
    Completed,
    /// 错误
    Error,
}

/// 会话
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    /// 会话 ID
    pub id: String,
    /// 用户 ID
    pub user_id: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 最后活跃时间
    pub last_active: DateTime<Utc>,
    /// 消息历史（滑动窗口）
    messages: Vec<Message>,
    /// 系统提示词
    system_prompt: Option<String>,
    /// 当前状态
    state: SessionState,
    /// 工具调用计数
    tool_calls: usize,
    /// 元数据
    metadata: HashMap<String, String>,
    /// 短期记忆滑动窗口容量（消息对数量）
    short_term_capacity: usize,
}

impl Session {
    /// 创建新的 Session（使用默认配置）
    pub fn new(user_id: impl Into<String>) -> Self {
        Self::with_config(SessionConfig {
            user_id: Some(user_id.into()),
            ..Default::default()
        })
    }

    /// 使用配置创建
    pub fn with_config(config: SessionConfig) -> Self {
        Self {
            id: config
                .session_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            user_id: config.user_id,
            created_at: Utc::now(),
            last_active: Utc::now(),
            messages: Vec::new(),
            system_prompt: config.system_prompt,
            state: SessionState::Idle,
            tool_calls: 0,
            metadata: HashMap::new(),
            short_term_capacity: config.short_term_capacity,
        }
    }

    /// 添加用户消息
    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            tool_call_id: None,
        });
        self.last_active = Utc::now();
    }

    /// 添加助手消息
    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.into(),
            tool_call_id: None,
        });
        self.last_active = Utc::now();
    }

    /// 添加消息（兼容旧接口）
    pub fn add_message(&mut self, role: Role, content: impl Into<String>) {
        self.messages.push(Message {
            role,
            content: content.into(),
            tool_call_id: None, // 默认没有 tool_call_id
        });
        self.last_active = Utc::now();
    }

    /// 添加工具消息（带 tool_call_id）
    pub fn add_tool_message(&mut self, content: impl Into<String>, tool_call_id: &str) {
        self.messages.push(Message {
            role: Role::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.to_string()),
        });
        self.last_active = Utc::now();
    }

    /// 添加一轮对话（用户 + 助手），返回超额的归档消息对
    ///
    /// 这是滑动窗口的核心方法：
    /// - 新消息对加入队列尾部
    /// - 如果超出容量，从头部移除最早的对话对并返回
    pub fn add_conversation_pair(
        &mut self,
        user: impl Into<String>,
        assistant: impl Into<String>,
    ) -> Vec<ArchivedMessagePair> {
        let user_msg = user.into();
        let assistant_msg = assistant.into();
        let timestamp = Utc::now();

        // 添加消息
        self.messages.push(Message {
            role: Role::User,
            content: user_msg,
            tool_call_id: None,
        });
        self.messages.push(Message {
            role: Role::Assistant,
            content: assistant_msg,
            tool_call_id: None,
        });

        // 超出容量时，从头部移除最早的对话对
        let mut archived = Vec::new();
        let max_messages = self.short_term_capacity * 2; // 每对 2 条消息

        while self.messages.len() > max_messages {
            // 移除最早的 2 条消息（一对）
            if self.messages.len() >= 2 {
                let user_content = self.messages.remove(0).content;
                let assistant_content = self.messages.remove(0).content;
                archived.push(ArchivedMessagePair {
                    user_message: user_content,
                    assistant_message: assistant_content,
                    timestamp,
                });
            }
        }

        self.last_active = Utc::now();
        archived
    }

    /// 获取所有消息
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// 获取可变消息列表
    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    /// 获取最后一条消息
    pub fn last_message(&self) -> Option<&Message> {
        self.messages.last()
    }

    /// 获取当前消息对数量
    pub fn conversation_pairs(&self) -> usize {
        self.messages.len() / 2
    }

    /// 获取滑动窗口容量
    pub fn capacity(&self) -> usize {
        self.short_term_capacity
    }

    /// 清空消息历史
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// 设置系统提示词
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// 获取系统提示词
    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    /// 更新状态
    pub fn set_state(&mut self, state: SessionState) {
        self.state = state;
    }

    /// 获取状态
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// 增加工具调用计数
    pub fn increment_tool_calls(&mut self) {
        self.tool_calls += 1;
    }

    /// 获取工具调用计数
    pub fn tool_calls(&self) -> usize {
        self.tool_calls
    }

    /// 重置工具调用计数
    pub fn reset_tool_calls(&mut self) {
        self.tool_calls = 0;
    }

    /// 添加元数据
    pub fn set_metadata(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.metadata.insert(key.into(), value.into());
    }

    /// 获取元数据
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// 获取上下文用于 LLM（包含系统提示词 + 消息历史）
    pub fn to_context(&self) -> Vec<Message> {
        let mut ctx = Vec::new();

        // 添加系统提示词
        if let Some(ref sys) = self.system_prompt {
            ctx.push(Message {
                role: Role::System,
                content: sys.clone(),
                tool_call_id: None,
            });
        }

        // 添加消息历史
        ctx.extend(self.messages.clone());

        ctx
    }

    /// 获取用于 LLM 的上下文描述（仅对话，不含系统提示词）
    pub fn conversation_context(&self) -> Vec<Message> {
        self.messages.clone()
    }

    /// 获取会话摘要
    pub fn summary(&self) -> String {
        format!(
            "Session {} ({} pairs, capacity: {})",
            self.id,
            self.conversation_pairs(),
            self.short_term_capacity
        )
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new("anonymous")
    }
}
