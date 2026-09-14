//! # 框架级会话上下文管理者
//!
//! `SessionManager` 是会话上下文的**唯一入口**，也是「框架级」的落点：
//!
//! ```text
//!   编排服务 ──┐
//!   领域专家 ──┼──> SessionManager ──> Arc<SessionContext>（内存，进程内共享）
//!   HTTP 查询 ─┘         │
//!                        └──> SessionContextPort ──> SQLite（跨进程 / 跨重启共享）
//! ```
//!
//! ## 职责
//!
//! - **生命周期**：首次访问某 session 时从持久化恢复，之后常驻内存缓存；
//!   同一进程内 HTTP 与 MCP 请求读到的是**同一个** `Arc<SessionContext>`。
//! - **记忆回流**：任何持有 `Arc<SessionContext>` 的一方（尤其是领域专家）写入的内容，
//!   其它消费方立即可见——这就是「专家记忆同步到框架上下文」。
//! - **增量落盘**：`flush` 只写未落盘区间（水位由 `SessionContext` 维护），
//!   避免每轮重复写历史。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::domain::ports::SessionContextPort;
use crate::domain::session_context::{ContextMessage, ContextRole, SessionContext};

/// 从持久化恢复时读取的历史条数上限
const DEFAULT_LOAD_LIMIT: usize = 40;
/// 注入 LLM 的历史条数上限（条数 ≈ 轮数 × 2）
const DEFAULT_INJECT_LIMIT: usize = 6;

/// 框架级会话上下文管理者
pub struct SessionManager {
    store: Option<Arc<dyn SessionContextPort>>,
    cache: RwLock<HashMap<String, Arc<SessionContext>>>,
    load_limit: usize,
    inject_limit: usize,
}

impl SessionManager {
    pub fn new(store: Option<Arc<dyn SessionContextPort>>) -> Self {
        Self {
            store,
            cache: RwLock::new(HashMap::new()),
            load_limit: DEFAULT_LOAD_LIMIT,
            inject_limit: DEFAULT_INJECT_LIMIT,
        }
    }

    /// 自定义恢复上限与注入上限
    pub fn with_limits(mut self, load_limit: usize, inject_limit: usize) -> Self {
        if load_limit > 0 {
            self.load_limit = load_limit;
        }
        if inject_limit > 0 {
            self.inject_limit = inject_limit;
        }
        self
    }

    /// 注入 LLM 的历史条数上限
    pub fn inject_limit(&self) -> usize {
        self.inject_limit
    }

    /// 是否启用了持久化
    pub fn is_persistent(&self) -> bool {
        self.store.is_some()
    }

    /// 取（必要时恢复）某会话的框架级上下文
    ///
    /// 返回 `Arc`，调用方拿到的就是框架里那一份——写入即对所有人可见。
    pub fn context(&self, session_id: &str) -> Arc<SessionContext> {
        if session_id.is_empty() {
            // 无 session 时返回一个临时上下文，保证调用方无需判空
            return Arc::new(SessionContext::new(""));
        }

        // 快路径：已缓存
        if let Ok(cache) = self.cache.read() {
            if let Some(ctx) = cache.get(session_id) {
                return ctx.clone();
            }
        }

        // 慢路径：从持久化恢复（无存储则为空上下文）
        let ctx = match &self.store {
            Some(store) => {
                let history = store.load(session_id, self.load_limit);
                Arc::new(SessionContext::restore(session_id, history))
            }
            None => Arc::new(SessionContext::new(session_id)),
        };

        if let Ok(mut cache) = self.cache.write() {
            cache.insert(session_id.to_string(), ctx.clone());
        }
        ctx
    }

    /// 记录用户轮次
    pub fn record_user(&self, session_id: &str, content: &str) {
        if session_id.is_empty() || content.trim().is_empty() {
            return;
        }
        self.context(session_id).push_user(content);
    }

    /// 记录框架最终回答（source 一般为专家链或「框架」）
    pub fn record_assistant(&self, session_id: &str, content: &str, source: &str) {
        if session_id.is_empty() || content.trim().is_empty() {
            return;
        }
        self.context(session_id).push_assistant(content, source);
    }

    /// 记录最终交付给用户的答案
    ///
    /// 与 `record_assistant` 的区别：若最近一条已是专家回流的同内容消息，
    /// 则**不再重复写入**——专家已在 LLM 出口把回答回流进上下文，
    /// 编排层再写一份会导致同一份内容在上下文里出现两次（浪费 token 且干扰模型）。
    pub fn record_final_answer(&self, session_id: &str, content: &str, source: &str) {
        if session_id.is_empty() || content.trim().is_empty() {
            return;
        }
        let ctx = self.context(session_id);
        let duplicated = ctx
            .recent(1)
            .first()
            .map(|m| m.role == ContextRole::Expert && m.content == content)
            .unwrap_or(false);
        if duplicated {
            return;
        }
        ctx.push_assistant(content, source);
    }

    /// 记录框架过程性提示
    pub fn record_system(&self, session_id: &str, content: &str) {
        if session_id.is_empty() || content.trim().is_empty() {
            return;
        }
        self.context(session_id).push_system(content);
    }

    /// 把未落盘的增量写入持久化存储
    ///
    /// 内存上下文始终保留完整历史；存储只补增量，因此重复调用无害。
    pub fn flush(&self, session_id: &str) {
        let (store, ctx) = match (&self.store, session_id) {
            (Some(s), sid) if !sid.is_empty() => (s, self.context(sid)),
            _ => return,
        };
        let pending: Vec<ContextMessage> = ctx.pending();
        if pending.is_empty() {
            return;
        }
        let n = pending.len();
        for msg in pending {
            store.append(session_id, &msg);
        }
        ctx.mark_persisted(n);
    }

    /// 清空某会话（内存 + 持久化）
    pub fn clear(&self, session_id: &str) {
        if session_id.is_empty() {
            return;
        }
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(session_id);
        }
        if let Some(store) = &self.store {
            store.clear(session_id);
        }
    }

    /// 缓存中的会话数（可观测性用）
    pub fn cached_sessions(&self) -> usize {
        self.cache.read().map(|c| c.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session_context::ContextRole;

    /// 内存版测试替身：验证增量落盘与跨消费方可见性
    struct MemStore {
        rows: RwLock<Vec<(String, ContextMessage)>>,
    }
    impl SessionContextPort for MemStore {
        fn load(&self, session_id: &str, limit: usize) -> Vec<ContextMessage> {
            let rows = self.rows.read().unwrap();
            let mut out: Vec<ContextMessage> = rows
                .iter()
                .filter(|(s, _)| s == session_id)
                .map(|(_, m)| m.clone())
                .collect();
            let start = out.len().saturating_sub(limit);
            out.drain(..start);
            out
        }
        fn append(&self, session_id: &str, message: &ContextMessage) {
            self.rows
                .write()
                .unwrap()
                .push((session_id.to_string(), message.clone()));
        }
        fn clear(&self, session_id: &str) {
            self.rows.write().unwrap().retain(|(s, _)| s != session_id);
        }
    }

    #[test]
    fn same_context_shared_across_callers() {
        let mgr = SessionManager::new(None);
        let a = mgr.context("s1");
        let b = mgr.context("s1");
        a.push_user("你好");
        // b 与 a 是同一份 → 调用方之间立即可见
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn expert_memory_visible_to_other_consumers() {
        let mgr = SessionManager::new(None);
        let ctx = mgr.context("s1");
        ctx.push_expert_exchange("rust-chat", "Rust 专家", "写个函数", "fn main() {}");
        // 模拟「另一个专家 / 查询接口」来读
        let other = mgr.context("s1");
        assert_eq!(other.expert_memory("rust-chat").len(), 2);
    }

    #[test]
    fn flush_writes_only_pending_and_persists_across_managers() {
        let store = Arc::new(MemStore {
            rows: RwLock::new(Vec::new()),
        });
        let mgr = SessionManager::new(Some(store.clone()));
        mgr.record_user("s1", "第一轮");
        mgr.flush("s1");
        mgr.flush("s1"); // 重复调用不应重复写入
        assert_eq!(store.rows.read().unwrap().len(), 1);

        // 模拟新进程：新的 Manager 从存储恢复
        let mgr2 = SessionManager::new(Some(store.clone()));
        let ctx2 = mgr2.context("s1");
        assert_eq!(ctx2.len(), 1);
        assert_eq!(ctx2.all()[0].content, "第一轮");
        assert_eq!(ctx2.all()[0].role, ContextRole::User);
    }
}
