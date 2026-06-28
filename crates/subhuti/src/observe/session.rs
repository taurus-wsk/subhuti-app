//! Session 记录管理
//!
//! 记录每个 Session 的所有请求历史
//! 存储方式：内存 + JSON 文件持久化（与 Trace 一致）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Session 中的单条请求记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRequest {
    /// Trace ID
    pub trace_id: String,
    /// 用户输入
    pub input: String,
    /// AI 输出
    pub output: Option<String>,
    /// 耗时（毫秒）
    pub duration_ms: Option<u64>,
    /// 匹配的 Skill
    pub matched_skill: Option<String>,
    /// Token 使用
    pub token_usage: Option<String>,
    /// 状态
    pub status: String,
    /// 时间戳
    pub timestamp: String,
}

/// Session 完整记录
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Session ID
    pub session_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 创建时间
    pub created_at: String,
    /// 最后活跃时间
    pub last_active: String,
    /// 请求历史
    pub requests: Vec<SessionRequest>,
    /// 总请求数
    pub total_requests: usize,
}

/// record_request 的参数封装（避免参数过多触发 clippy::too-many-arguments）
pub struct SessionRecordParams<'a> {
    pub session_id: &'a str,
    pub user_id: &'a str,
    pub trace_id: &'a str,
    pub input: &'a str,
    pub output: Option<&'a str>,
    pub duration_ms: Option<u64>,
    pub matched_skill: Option<&'a str>,
    pub token_usage: Option<String>,
    pub status: &'a str,
}

/// Session 存储管理器
pub struct SessionStore {
    sessions: HashMap<String, SessionRecord>,
    max_sessions: usize,
    /// 持久化目录
    persist_dir: Option<String>,
}

impl SessionStore {
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions,
            persist_dir: None,
        }
    }

    /// 创建带持久化的存储
    pub fn with_persistence(max_sessions: usize, dir: &str) -> Self {
        let _ = std::fs::create_dir_all(dir);
        let mut store = Self {
            sessions: HashMap::new(),
            max_sessions,
            persist_dir: Some(dir.to_string()),
        };
        store.load_from_disk();
        store
    }

    /// 从磁盘加载
    fn load_from_disk(&mut self) {
        if let Some(ref dir) = self.persist_dir {
            let path = std::path::Path::new(dir);
            if path.exists() {
                if let Ok(entries) = std::fs::read_dir(path) {
                    for entry in entries.flatten() {
                        let file_path = entry.path();
                        if file_path.extension().and_then(|s| s.to_str()) == Some("json") {
                            if let Ok(content) = std::fs::read_to_string(&file_path) {
                                if let Ok(session) = serde_json::from_str::<SessionRecord>(&content)
                                {
                                    self.sessions.insert(session.session_id.clone(), session);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// 添加请求到 Session
    pub fn add_request(&mut self, session_id: &str, user_id: &str, request: SessionRequest) {
        let now = chrono::Utc::now().to_rfc3339();

        let session = self
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| SessionRecord {
                session_id: session_id.to_string(),
                user_id: user_id.to_string(),
                created_at: now.clone(),
                last_active: now.clone(),
                requests: Vec::new(),
                total_requests: 0,
            });

        session.requests.push(request);
        session.last_active = now;
        session.total_requests = session.requests.len();

        // 持久化
        self.persist_session(session_id);

        // 清理旧 Session
        if self.sessions.len() > self.max_sessions {
            self.evict_old_sessions();
        }
    }

    /// 持久化单个 Session
    fn persist_session(&self, session_id: &str) {
        if let Some(ref dir) = self.persist_dir {
            if let Some(session) = self.sessions.get(session_id) {
                let file_path = std::path::Path::new(dir).join(format!("{}.json", session_id));
                if let Ok(json) = serde_json::to_string_pretty(session) {
                    let _ = std::fs::write(file_path, json);
                }
            }
        }
    }

    /// 清理旧 Session（按最后活跃时间）
    fn evict_old_sessions(&mut self) {
        let to_remove = self.sessions.len().saturating_sub(self.max_sessions);
        if to_remove == 0 {
            return;
        }

        // 收集需要删除的 ID
        let mut sessions: Vec<_> = self.sessions.iter().collect();
        sessions.sort_by_key(|(_, s)| s.last_active.clone());

        let ids_to_remove: Vec<_> = sessions
            .iter()
            .take(to_remove)
            .map(|(id, _)| id.to_string())
            .collect();

        // 删除
        for id in ids_to_remove {
            self.sessions.remove(&id);
            // 删除文件
            if let Some(ref dir) = self.persist_dir {
                let file_path = std::path::Path::new(dir).join(format!("{}.json", id));
                let _ = std::fs::remove_file(file_path);
            }
        }
    }

    /// 获取 Session
    pub fn get_session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.sessions.get(session_id)
    }

    /// 获取所有 Session 摘要
    pub fn list_sessions(&self) -> Vec<SessionRecord> {
        self.sessions.values().cloned().collect()
    }

    /// 搜索 Session（按 user_id）
    pub fn search_by_user(&self, user_id: &str) -> Vec<&SessionRecord> {
        self.sessions
            .values()
            .filter(|s| s.user_id == user_id)
            .collect()
    }
}

/// Session 观察器（线程安全）
#[derive(Clone)]
pub struct SessionObserver {
    store: Arc<Mutex<SessionStore>>,
}

impl Default for SessionObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionObserver {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(SessionStore::new(1000))),
        }
    }

    /// 创建带持久化的观察器
    pub fn with_persistence(dir: &str) -> Self {
        Self {
            store: Arc::new(Mutex::new(SessionStore::with_persistence(1000, dir))),
        }
    }

    /// 记录请求（使用结构体参数，避免参数过多）
    pub fn record_request(&self, params: &SessionRecordParams<'_>) {
        let request = SessionRequest {
            trace_id: params.trace_id.to_string(),
            input: params.input.to_string(),
            output: params.output.map(|s| s.to_string()),
            duration_ms: params.duration_ms,
            matched_skill: params.matched_skill.map(|s| s.to_string()),
            token_usage: params.token_usage.as_ref().map(|v| v.to_string()),
            status: params.status.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Ok(mut store) = self.store.lock() {
            store.add_request(params.session_id, params.user_id, request);
        }
    }

    /// 获取 Session
    pub fn get_session(&self, session_id: &str) -> Option<SessionRecord> {
        self.store
            .lock()
            .ok()
            .and_then(|store| store.get_session(session_id).cloned())
    }

    /// 获取所有 Session
    pub fn list_sessions(&self) -> Vec<SessionRecord> {
        self.store
            .lock()
            .map(|store| store.list_sessions())
            .unwrap_or_default()
    }
}
