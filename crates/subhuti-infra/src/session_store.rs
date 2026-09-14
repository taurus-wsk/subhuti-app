//! # 多轮会话 SQLite 持久化
//!
//! 按 `session_id` 落盘每轮对话（user / assistant），让**多轮上下文跨进程、跨重启生效**：
//!
//! - HTTP 服务进程与 MCP stdio 进程是两个独立进程，内存态 Session 无法共享；
//!   落 SQLite 后二者共用同一个库，MCP 每次被拉起也能读到上一轮对话。
//! - 与 `trace_store` 一样走「独立 OS 线程 + current_thread runtime + std_mpsc 回复」
//!   的同步桥接，避免在宿主 runtime 内 `block_on` 导致 panic。
//!
//! ## 表结构
//!
//! `session_messages(id, session_id, role, content, created_at)`，`(session_id, id)` 索引。

use std::sync::mpsc as std_mpsc;

use sqlx::Row;
use tokio::sync::mpsc as tokio_mpsc;

/// 会话库默认路径：统一数据目录下的 `sessions.sqlite`（`SUBHUTI_SESSION_SQLITE` 可覆盖）
pub fn default_session_db_path() -> String {
    crate::data_dir::session_db_path()
}

/// 一条历史消息
///
/// `source` / `expert_id` 用于保住「这条记忆是谁产出的」——
/// 框架级上下文要能回答「哪个专家说过什么」，而不只是 user/assistant。
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub role: String,
    pub content: String,
    /// 可读来源名（「用户」「框架」或专家名）
    pub source: String,
    /// 产出专家 ID（无则为空）
    pub expert_id: String,
}

enum SessionCmd {
    Append {
        session_id: String,
        role: String,
        content: String,
        source: String,
        expert_id: String,
        done: Option<std_mpsc::Sender<()>>,
    },
    Recent {
        session_id: String,
        limit: usize,
        resp: std_mpsc::Sender<Vec<StoredMessage>>,
    },
    Clear {
        session_id: String,
        done: Option<std_mpsc::Sender<()>>,
    },
}

/// 会话历史存储（同步 API，内部由 worker 线程驱动 sqlx）
pub struct SqliteSessionStore {
    tx: tokio_mpsc::UnboundedSender<SessionCmd>,
    _worker: std::thread::JoinHandle<()>,
}

impl SqliteSessionStore {
    /// 打开（必要时创建）会话库并建表
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let (tx, rx) = tokio_mpsc::unbounded_channel::<SessionCmd>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<anyhow::Result<()>>();
        let path = path.to_string();

        let worker = std::thread::spawn(move || {
            worker_loop(path, rx, ready_tx);
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                tx,
                _worker: worker,
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => {
                let _ = worker.join();
                Err(anyhow::anyhow!("session store 后台线程启动失败"))
            }
        }
    }

    /// 追加一条消息（默认不等待落盘，避免拖慢主链路；传 wait=true 可同步等待）
    ///
    /// - `source`: 可读来源名（「用户」「框架」或专家名）
    /// - `expert_id`: 产出专家 ID（无则传空串）
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        source: &str,
        expert_id: &str,
        wait: bool,
    ) {
        if session_id.is_empty() || content.trim().is_empty() {
            return;
        }
        let mk = |done: Option<std_mpsc::Sender<()>>| SessionCmd::Append {
            session_id: session_id.to_string(),
            role: role.to_string(),
            content: content.to_string(),
            source: source.to_string(),
            expert_id: expert_id.to_string(),
            done,
        };
        let done = if wait {
            let (tx, rx) = std_mpsc::channel();
            let _ = self.tx.send(mk(Some(tx.clone())));
            Some(rx)
        } else {
            let _ = self.tx.send(mk(None));
            None
        };
        if let Some(rx) = done {
            let _ = rx.recv();
        }
    }

    /// 读取最近 `limit` 条历史（按时间正序，越新越靠后）
    pub fn recent(&self, session_id: &str, limit: usize) -> Vec<StoredMessage> {
        if session_id.is_empty() || limit == 0 {
            return Vec::new();
        }
        let (resp, rx) = std_mpsc::channel();
        if self
            .tx
            .send(SessionCmd::Recent {
                session_id: session_id.to_string(),
                limit,
                resp,
            })
            .is_err()
        {
            return Vec::new();
        }
        rx.recv().unwrap_or_default()
    }

    /// 清空某会话历史
    #[allow(dead_code)]
    pub fn clear(&self, session_id: &str) {
        let _ = self.tx.send(SessionCmd::Clear {
            session_id: session_id.to_string(),
            done: None,
        });
    }
}

fn worker_loop(
    path: String,
    mut rx: tokio_mpsc::UnboundedReceiver<SessionCmd>,
    ready_tx: std_mpsc::Sender<anyhow::Result<()>>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = ready_tx.send(Err(anyhow::anyhow!("创建 runtime 失败: {}", e)));
            return;
        }
    };

    rt.block_on(async move {
        let url = format!("sqlite://{}?mode=rwc", path);
        let pool = match sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_lazy(&url)
        {
            Ok(p) => p,
            Err(e) => {
                let _ = ready_tx.send(Err(anyhow::anyhow!("连接会话库失败: {}", e)));
                return;
            }
        };

        if let Err(e) = init_schema(&pool).await {
            let _ = ready_tx.send(Err(anyhow::anyhow!("建表失败: {}", e)));
            return;
        }
        let _ = ready_tx.send(Ok(()));

        while let Some(cmd) = rx.recv().await {
            match cmd {
                SessionCmd::Append {
                    session_id,
                    role,
                    content,
                    source,
                    expert_id,
                    done,
                } => {
                    let _ = sqlx::query(
                        "INSERT INTO session_messages \
                         (session_id, role, content, source, expert_id, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                    )
                    .bind(session_id)
                    .bind(role)
                    .bind(content)
                    .bind(source)
                    .bind(expert_id)
                    .execute(&pool)
                    .await;
                    if let Some(d) = done {
                        let _ = d.send(());
                    }
                }
                SessionCmd::Recent {
                    session_id,
                    limit,
                    resp,
                } => {
                    // 先按 id 倒序取最近 N 条，再翻转成正序（旧 → 新）
                    let rows = sqlx::query(
                        "SELECT role, content, COALESCE(source,'') AS source, \
                         COALESCE(expert_id,'') AS expert_id FROM session_messages \
                         WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2",
                    )
                    .bind(session_id)
                    .bind(limit as i64)
                    .fetch_all(&pool)
                    .await;
                    let mut out: Vec<StoredMessage> = match rows {
                        Ok(rs) => rs
                            .iter()
                            .map(|r| StoredMessage {
                                role: r.get::<String, _>("role"),
                                content: r.get::<String, _>("content"),
                                source: r.get::<String, _>("source"),
                                expert_id: r.get::<String, _>("expert_id"),
                            })
                            .collect(),
                        Err(_) => Vec::new(),
                    };
                    out.reverse();
                    let _ = resp.send(out);
                }
                SessionCmd::Clear { session_id, done } => {
                    let _ = sqlx::query("DELETE FROM session_messages WHERE session_id = ?1")
                        .bind(session_id)
                        .execute(&pool)
                        .await;
                    if let Some(d) = done {
                        let _ = d.send(());
                    }
                }
            }
        }
    });
}

async fn init_schema(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    let ddl = [
        "PRAGMA journal_mode=WAL;",
        "PRAGMA busy_timeout=5000;",
        "CREATE TABLE IF NOT EXISTS session_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id TEXT NOT NULL DEFAULT '',
            role TEXT NOT NULL DEFAULT 'user',
            content TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT ''
        )",
        "CREATE INDEX IF NOT EXISTS idx_session_messages_sid ON session_messages(session_id, id)",
    ];
    for sql in ddl {
        sqlx::query(sql).execute(pool).await?;
    }
    // 增量迁移：旧库没有 source / expert_id 列，按需补（SQLite 支持 ADD COLUMN + DEFAULT）
    for (col, ddl) in [
        (
            "source",
            "ALTER TABLE session_messages ADD COLUMN source TEXT NOT NULL DEFAULT ''",
        ),
        (
            "expert_id",
            "ALTER TABLE session_messages ADD COLUMN expert_id TEXT NOT NULL DEFAULT ''",
        ),
    ] {
        if !column_exists(pool, "session_messages", col).await? {
            sqlx::query(ddl).execute(pool).await?;
        }
    }
    Ok(())
}

/// 判断表中是否存在某列（PRAGMA table_info）
///
/// ⚠️ SQLite 的 PRAGMA 不接受绑定参数（`PRAGMA table_info(?1)` 会报语法错误），
/// 必须用字面量拼表名。此处 `table` 始终是受控常量（`session_messages`），
/// 不存在注入风险——但为了绝对安全仍只拼白名单内的表名。
async fn column_exists(pool: &sqlx::SqlitePool, table: &str, column: &str) -> anyhow::Result<bool> {
    if table != "session_messages" {
        return Ok(false);
    }
    let rows = sqlx::query(&format!("PRAGMA table_info({})", table))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().any(|r| r.get::<String, _>("name") == column))
}
