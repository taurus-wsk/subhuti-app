//! # 框架级会话上下文（Session Context）
//!
//! ## 为什么要有这一层
//!
//! 会话上下文是**框架级资产**，不是某个专家的私有状态：
//!
//! - **谁都能读**：编排层、任意专家、HTTP/MCP 查询接口、藏经阁沉淀，读的是同一份上下文。
//! - **谁写都能回流**：专家执行过程中产生的问答（含「哪个专家说的」）会回流进框架上下文，
//!   后续专家或其他消费方能看见——而不是用完即弃。
//! - **与具体存储解耦**：本模块只定义内存结构与行为；持久化由
//!   [`crate::domain::ports::SessionContextPort`] 出站端口承担（当前实现为 SQLite）。
//!
//! ## 与「多轮历史注入」的关系
//!
//! 注入 LLM 的 messages 只是上下文的**一种消费方式**（`to_llm_messages`）。
//! 同一个 `SessionContext` 还能被：查询接口展示、专家间传递、沉淀为长期记忆。
//!
//! ## 并发安全
//!
//! 消息列表用 `RwLock`、已落盘水位用 `AtomicUsize`，整体 `Send + Sync`，
//! 可被多个并发请求共享（`Arc<SessionContext>`）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::traits::{DomainMessage, DomainRole};

/// 上下文消息的角色/产出者分类
///
/// 比 LLM 的 `user/assistant/system` 更细：`Expert` 表示这条内容由某位专家产出，
/// 从而在框架层面保住「谁的记忆」这一信息——这是跨专家协作的基础。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRole {
    /// 终端用户（HTTP / MCP 调用方）的输入
    User,
    /// 框架最终返回给用户的结果
    Assistant,
    /// 某位专家执行过程中产出的内容（保留 expert_id / source）
    Expert,
    /// 框架自身的过程性提示（路由、计划等）
    System,
    /// 工具/外部系统产出（编译结果、检索结果等）
    Tool,
}

impl ContextRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextRole::User => "user",
            ContextRole::Assistant => "assistant",
            ContextRole::Expert => "expert",
            ContextRole::System => "system",
            ContextRole::Tool => "tool",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "assistant" => ContextRole::Assistant,
            "expert" => ContextRole::Expert,
            "system" => ContextRole::System,
            "tool" => ContextRole::Tool,
            _ => ContextRole::User,
        }
    }
}

/// 框架上下文中的一条消息
#[derive(Debug, Clone)]
pub struct ContextMessage {
    /// 产出者分类
    pub role: ContextRole,
    /// 正文
    pub content: String,
    /// 可读来源名：框架 = 「框架」，专家 = 专家名，工具 = 工具名
    pub source: String,
    /// 产出专家 ID（仅 `ContextRole::Expert` 有值）—— 用于按专家检索记忆
    pub expert_id: Option<String>,
    /// Unix 秒级时间戳
    pub created_at: i64,
}

impl ContextMessage {
    pub fn new(role: ContextRole, content: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            source: source.into(),
            expert_id: None,
            created_at: now_secs(),
        }
    }

    pub fn with_expert_id(mut self, expert_id: impl Into<String>) -> Self {
        self.expert_id = Some(expert_id.into());
        self
    }

    /// 转成 LLM 消息：expert/tool/system 归入 assistant 或 system，
    /// 并在正文前加来源前缀，让后续专家知道这段话是谁说的。
    pub fn to_domain_message(&self) -> DomainMessage {
        let (role, content) = match self.role {
            ContextRole::User => (DomainRole::User, self.content.clone()),
            ContextRole::System => (DomainRole::System, self.content.clone()),
            ContextRole::Assistant => (DomainRole::Assistant, self.content.clone()),
            // 专家产出的内容 → 作为 assistant，带来源前缀
            ContextRole::Expert => (
                DomainRole::Assistant,
                if self.source.is_empty() {
                    self.content.clone()
                } else {
                    format!("【{}】{}", self.source, self.content)
                },
            ),
            // 工具结果 → 作为 user 上下文（模型习惯把它当作「观察到的输入」）
            ContextRole::Tool => (
                DomainRole::User,
                format!("（{}输出）{}", self.source, self.content),
            ),
        };
        DomainMessage { role, content }
    }
}

/// 框架级会话上下文：一次会话的完整记忆
///
/// 生命周期由 [`crate::application::session_manager::SessionManager`] 管理：
/// 首次访问时从持久化恢复，执行过程中累加，请求结束时增量落盘。
pub struct SessionContext {
    session_id: String,
    messages: RwLock<Vec<ContextMessage>>,
    /// 会话级元数据（工作目录、最近专家链、用户偏好等），供任意消费方读写
    metadata: RwLock<HashMap<String, String>>,
    /// 已落盘消息条数（水位）：`flush` 只写 `[persisted..len)` 区间，避免重复落盘
    persisted: AtomicUsize,
}

impl SessionContext {
    /// 创建空上下文（新会话）
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            messages: RwLock::new(Vec::new()),
            metadata: RwLock::new(HashMap::new()),
            persisted: AtomicUsize::new(0),
        }
    }

    /// 从持久化恢复（历史消息视为已落盘，水位设为 len）
    pub fn restore(session_id: impl Into<String>, messages: Vec<ContextMessage>) -> Self {
        let n = messages.len();
        Self {
            session_id: session_id.into(),
            messages: RwLock::new(messages),
            metadata: RwLock::new(HashMap::new()),
            persisted: AtomicUsize::new(n),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 追加一条消息
    pub fn push(&self, msg: ContextMessage) {
        if msg.content.trim().is_empty() {
            return;
        }
        if let Ok(mut ms) = self.messages.write() {
            ms.push(msg);
        }
    }

    /// 记录用户轮次（来源固定为「用户」）
    pub fn push_user(&self, content: impl Into<String>) {
        self.push(ContextMessage::new(ContextRole::User, content, "用户"));
    }

    /// 记录框架最终回答
    pub fn push_assistant(&self, content: impl Into<String>, source: impl Into<String>) {
        self.push(ContextMessage::new(ContextRole::Assistant, content, source));
    }

    /// 记录框架过程性提示（路由、计划等）
    pub fn push_system(&self, content: impl Into<String>) {
        self.push(ContextMessage::new(ContextRole::System, content, "框架"));
    }

    /// **专家记忆回流入口**：把专家本轮的问题与回答写回框架上下文。
    ///
    /// 这是「专家用完即弃」与「专家记忆沉淀到框架」的分水岭——
    /// 写入后，后续专家、查询接口、藏经阁沉淀都能看到这位专家说了什么。
    pub fn push_expert_exchange(
        &self,
        expert_id: &str,
        expert_name: &str,
        question: &str,
        answer: &str,
    ) {
        if !question.trim().is_empty() {
            self.push(
                ContextMessage::new(ContextRole::User, question, "用户").with_expert_id(expert_id),
            );
        }
        if !answer.trim().is_empty() {
            self.push(
                ContextMessage::new(ContextRole::Expert, answer, expert_name)
                    .with_expert_id(expert_id),
            );
        }
    }

    /// 记录工具产出（编译结果、检索结果等）
    pub fn push_tool(&self, source: impl Into<String>, content: impl Into<String>) {
        self.push(ContextMessage::new(ContextRole::Tool, content, source));
    }

    /// 最近 `limit` 条（按时间正序，越新越靠后）
    pub fn recent(&self, limit: usize) -> Vec<ContextMessage> {
        if limit == 0 {
            return Vec::new();
        }
        match self.messages.read() {
            Ok(ms) => {
                let start = ms.len().saturating_sub(limit);
                ms[start..].to_vec()
            }
            Err(_) => Vec::new(),
        }
    }

    /// 全部消息
    pub fn all(&self) -> Vec<ContextMessage> {
        self.messages
            .read()
            .map(|ms| ms.clone())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.messages.read().map(|ms| ms.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// **只取某位专家的记忆**（跨轮次、跨专家协作时用）
    pub fn expert_memory(&self, expert_id: &str) -> Vec<ContextMessage> {
        if expert_id.is_empty() {
            return Vec::new();
        }
        self.messages
            .read()
            .map(|ms| {
                ms.iter()
                    .filter(|m| m.expert_id.as_deref() == Some(expert_id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 转成可注入 LLM 的消息序列（取最近 `limit` 条）
    ///
    /// 这是上下文的**消费方式之一**：把它压平成模型能理解的 user/assistant/system。
    pub fn to_llm_messages(&self, limit: usize) -> Vec<DomainMessage> {
        self.recent(limit)
            .into_iter()
            .map(|m| m.to_domain_message())
            .collect()
    }

    /// 生成给「其他地方」使用的文本摘要（如查询接口、藏经阁沉淀、专家间传递）
    pub fn briefing(&self, limit: usize) -> String {
        self.recent(limit)
            .into_iter()
            .map(|m| format!("[{}|{}] {}", m.role.as_str(), m.source, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 尚未落盘的消息（增量 flush 用）
    pub fn pending(&self) -> Vec<ContextMessage> {
        let water = self.persisted.load(Ordering::SeqCst);
        match self.messages.read() {
            Ok(ms) => {
                if ms.len() > water {
                    ms[water..].to_vec()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
    }

    /// 推进落盘水位
    pub fn mark_persisted(&self, count: usize) {
        self.persisted.fetch_add(count, Ordering::SeqCst);
    }

    pub fn persisted_count(&self) -> usize {
        self.persisted.load(Ordering::SeqCst)
    }

    pub fn set_metadata(&self, key: impl Into<String>, value: impl Into<String>) {
        if let Ok(mut md) = self.metadata.write() {
            md.insert(key.into(), value.into());
        }
    }

    pub fn get_metadata(&self, key: &str) -> Option<String> {
        self.metadata.read().ok()?.get(key).cloned()
    }

    pub fn metadata(&self) -> HashMap<String, String> {
        self.metadata.read().map(|m| m.clone()).unwrap_or_default()
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expert_exchange_is_recorded_with_source() {
        let ctx = SessionContext::new("s1");
        ctx.push_expert_exchange("rust-chat", "Rust 专家", "写个函数", "fn main() {}");
        let all = ctx.all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].role, ContextRole::Expert);
        assert_eq!(all[1].source, "Rust 专家");
        assert_eq!(all[1].expert_id.as_deref(), Some("rust-chat"));
    }

    #[test]
    fn expert_memory_filters_by_expert() {
        let ctx = SessionContext::new("s1");
        ctx.push_expert_exchange("a", "专家A", "q1", "a1");
        ctx.push_expert_exchange("b", "专家B", "q2", "b2");
        assert_eq!(ctx.expert_memory("a").len(), 2); // 该专家的 user + expert 两条
        assert_eq!(ctx.expert_memory("b").len(), 2);
        assert!(ctx.expert_memory("c").is_empty());
    }

    #[test]
    fn pending_tracks_unpersisted_messages() {
        let ctx = SessionContext::new("s1");
        ctx.push_user("hi");
        assert_eq!(ctx.pending().len(), 1);
        ctx.mark_persisted(1);
        assert_eq!(ctx.pending().len(), 0);
        ctx.push_user("again");
        assert_eq!(ctx.pending().len(), 1);
    }

    #[test]
    fn restore_marks_all_persisted() {
        let ctx = SessionContext::restore(
            "s1",
            vec![ContextMessage::new(ContextRole::User, "old", "用户")],
        );
        assert_eq!(ctx.len(), 1);
        assert!(ctx.pending().is_empty());
    }

    #[test]
    fn to_llm_messages_prefixes_expert_source() {
        let ctx = SessionContext::new("s1");
        ctx.push_expert_exchange("b", "Blender 专家", "q", "回答内容");
        let msgs = ctx.to_llm_messages(10);
        assert!(msgs
            .iter()
            .any(|m| m.content.contains("【Blender 专家】回答内容")));
    }
}
