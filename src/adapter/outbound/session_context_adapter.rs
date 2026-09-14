//! # 会话上下文持久化适配器（出站适配器）
//!
//! 把领域端口 [`SessionContextPort`] 接到 SQLite 存储（`subhuti-infra::session_store`）。
//!
//! 依赖方向：`domain::ports::SessionContextPort`（接口） ← 本适配器 → `subhuti-infra`（实现）。
//! 领域层与应用层只见接口，不感知 SQLite。

use std::sync::Arc;

use crate::domain::ports::SessionContextPort;
use crate::domain::session_context::{ContextMessage, ContextRole};

/// 基于 SQLite 的会话上下文存储
pub struct SqliteSessionContextAdapter {
    store: Arc<subhuti_infra::session_store::SqliteSessionStore>,
}

impl SqliteSessionContextAdapter {
    pub fn new(store: Arc<subhuti_infra::session_store::SqliteSessionStore>) -> Self {
        Self { store }
    }
}

impl SessionContextPort for SqliteSessionContextAdapter {
    fn load(&self, session_id: &str, limit: usize) -> Vec<ContextMessage> {
        self.store
            .recent(session_id, limit)
            .into_iter()
            .map(|m| ContextMessage {
                role: ContextRole::from_str(&m.role),
                content: m.content,
                source: if m.source.is_empty() {
                    // 旧数据没有 source：按角色回填一个可读值
                    match ContextRole::from_str(&m.role) {
                        ContextRole::User => "用户".to_string(),
                        _ => "框架".to_string(),
                    }
                } else {
                    m.source
                },
                expert_id: if m.expert_id.is_empty() {
                    None
                } else {
                    Some(m.expert_id)
                },
                created_at: 0,
            })
            .collect()
    }

    fn append(&self, session_id: &str, message: &ContextMessage) {
        self.store.append(
            session_id,
            message.role.as_str(),
            &message.content,
            &message.source,
            message.expert_id.as_deref().unwrap_or(""),
            false,
        );
    }

    fn clear(&self, session_id: &str) {
        self.store.clear(session_id);
    }
}
