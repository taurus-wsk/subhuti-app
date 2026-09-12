//! # Trace SQLite 持久化存储
//!
//! 把 trace / span / fn_call / fn_log 落盘到共享 SQLite 文件，
//! 让 HTTP 服务进程与 MCP 进程（两个独立进程）的 trace 数据自然汇聚到同一个库。
//!
//! ## 同步 / 异步桥接
//!
//! `TraceObserverPort` 是同步 trait，而 sqlx 的 SQLite 操作是异步的。这里采用的桥接方式：
//!
//! - 本结构**不持有运行时**，而是在 `open()` 时启动一个**独立的 OS 线程（worker）**，
//!   该线程内部用 `current_thread` runtime 驱动所有 sqlx 异步操作；
//! - 命令通过 `tokio::mpsc::unbounded_channel` 发给 worker 线程；需要结果时，
//!   调用方在 **`std::sync::mpsc`（普通标准库通道）** 上 `recv()` 等待。
//!   `std::sync::mpsc::recv` 是**普通线程阻塞**，不会像 `Runtime::block_on` /
//!   `oneshot::blocking_recv` 那样在 "已处于 runtime 内" 时 panic。
//!
//! 之所以不直接在宿主 runtime 里 `block_on` 一个独立 runtime，是因为
//! `mcp::run` / HTTP `main` 本身都跑在 `#[tokio::main]` 运行时内，
//! 再 `block_on` 新 runtime 会直接 panic。独立线程方案天然规避该问题，
//! 且读 future 只依赖 worker 线程自己的 pool，不与宿主 runtime 互相阻塞。
//!
//! ## 并发
//!
//! 使用 WAL 日志模式 + busy_timeout，支持两个进程并发读写同一文件。

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;

use chrono::{DateTime, Utc};
use sqlx::Row;
use subhuti_core::observe::{FnCallData, LogEntry, LogLevel, SpanData, TraceHandle};
use tokio::sync::mpsc as tokio_mpsc;

/// trace 库默认路径：统一数据目录下的 `traces.sqlite`。
///
/// 走 `data_dir::trace_db_path()`，即 `SUBHUTI_TRACE_SQLITE` > `<data_dir>/traces.sqlite`，
/// 而 `<data_dir>` 默认是 `~/.subhuti/data`（与 cwd 无关）。这样 HTTP 进程与 MCP 进程
/// 必定解析到同一个绝对路径，链路自然汇聚。
pub fn default_trace_db_path() -> String {
    crate::data_dir::trace_db_path()
}

/// 解析实际使用的 trace 库路径（环境变量优先）
pub fn resolve_trace_db_path() -> String {
    crate::data_dir::trace_db_path()
}

/// 后台 worker 与同步方法之间的消息协议
enum TraceStoreMsg {
    WriteTrace(TraceHandle, Option<std_mpsc::Sender<()>>),
    WriteSpan {
        trace_id: String,
        span: SpanData,
        ack: Option<std_mpsc::Sender<()>>,
    },
    WriteFnCall {
        trace_id: String,
        fc: FnCallData,
        ack: Option<std_mpsc::Sender<()>>,
    },
    WriteFnLog {
        trace_id: String,
        log: LogEntry,
        ack: Option<std_mpsc::Sender<()>>,
    },
    ReadSummaries(std_mpsc::Sender<Vec<serde_json::Value>>),
    ReadTrace(String, std_mpsc::Sender<Option<serde_json::Value>>),
    ReadSpans(String, std_mpsc::Sender<Vec<SpanData>>),
    ReadFnCalls(String, std_mpsc::Sender<Vec<FnCallData>>),
    ReadFnLogs(String, std_mpsc::Sender<Vec<LogEntry>>),
}

/// SQLite 持久化 trace 存储
///
/// 只持有指向后台 worker 线程的命令通道；真正的 pool 与运行时都在 worker 线程内。
pub struct SqliteTraceStore {
    tx: tokio_mpsc::UnboundedSender<TraceStoreMsg>,
    /// 保持 worker 线程存活（store 被 drop 时 tx 关闭，worker 自然退出）
    _worker: std::thread::JoinHandle<()>,
}

impl SqliteTraceStore {
    /// 打开（必要时创建）共享 SQLite 库，并建表。
    ///
    /// 在**独立 OS 线程**内创建 current-thread runtime 并建池/建表，
    /// 因此无论调用方是否处于某个 tokio runtime 内都能安全工作。
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let (tx, rx) = tokio_mpsc::unbounded_channel::<TraceStoreMsg>();
        let (ready_tx, ready_rx) = std_mpsc::channel::<anyhow::Result<()>>();
        let path = path.to_string();

        let worker = std::thread::spawn(move || {
            worker_loop(path, rx, ready_tx);
        });

        // ready_rx 是 std mpsc：普通线程阻塞，在 runtime 内调用也不会 panic
        // （不会像 tokio oneshot::blocking_recv / Runtime::block_on 那样被 runtime 拒绝）。
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
                Err(anyhow::anyhow!("trace store 后台线程启动失败"))
            }
        }
    }

    // ─── 同步桥接：写（store_trace 阻塞保证落盘）────────────────────

    pub fn write_trace_sync(&self, trace: TraceHandle) {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::WriteTrace(trace, Some(tx)));
        let _ = rx.recv();
    }

    // ─── 同步桥接：写（fire-and-forget，不阻塞编排热路径）──────────

    pub fn write_span_fire_and_forget(&self, trace_id: String, span: SpanData) {
        let _ = self.tx.send(TraceStoreMsg::WriteSpan {
            trace_id,
            span,
            ack: None,
        });
    }

    pub fn write_fn_call_fire_and_forget(&self, trace_id: String, fc: FnCallData) {
        let _ = self.tx.send(TraceStoreMsg::WriteFnCall {
            trace_id,
            fc,
            ack: None,
        });
    }

    pub fn write_fn_log_fire_and_forget(&self, trace_id: String, log: LogEntry) {
        let _ = self.tx.send(TraceStoreMsg::WriteFnLog {
            trace_id,
            log,
            ack: None,
        });
    }

    // ─── 同步桥接：读（阻塞取回结果）────────────────────────────────

    pub fn read_summaries_sync(&self) -> Vec<serde_json::Value> {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::ReadSummaries(tx));
        rx.recv().unwrap_or_default()
    }

    pub fn read_trace_sync(&self, id: &str) -> Option<serde_json::Value> {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::ReadTrace(id.to_string(), tx));
        rx.recv().unwrap_or_default()
    }

    pub fn read_spans_sync(&self, id: &str) -> Vec<SpanData> {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::ReadSpans(id.to_string(), tx));
        rx.recv().unwrap_or_default()
    }

    pub fn read_fn_calls_sync(&self, id: &str) -> Vec<FnCallData> {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::ReadFnCalls(id.to_string(), tx));
        rx.recv().unwrap_or_default()
    }

    pub fn read_fn_logs_sync(&self, id: &str) -> Vec<LogEntry> {
        let (tx, rx) = std_mpsc::channel();
        let _ = self.tx.send(TraceStoreMsg::ReadFnLogs(id.to_string(), tx));
        rx.recv().unwrap_or_default()
    }

    // ─── 异步实现（仅在 worker 线程的 runtime 内被调用）─────────────

    async fn init_schema(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS trace_summaries (
                trace_id    TEXT PRIMARY KEY,
                user_id    TEXT NOT NULL DEFAULT '',
                session_id TEXT NOT NULL DEFAULT '',
                message    TEXT NOT NULL DEFAULT '',
                output     TEXT,
                error      TEXT,
                status     TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER,
                chain_name TEXT,
                expert_chain TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS trace_spans (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id   TEXT NOT NULL,
                span_type  TEXT NOT NULL,
                name       TEXT NOT NULL DEFAULT '',
                input      TEXT,
                output     TEXT,
                duration_ms INTEGER,
                tokens     INTEGER,
                timestamp  TEXT NOT NULL,
                success    INTEGER,
                extra      TEXT NOT NULL DEFAULT '{}'
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS trace_fn_calls (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id     TEXT NOT NULL,
                fn_name      TEXT NOT NULL,
                input        TEXT,
                output       TEXT,
                input_bytes  INTEGER,
                output_bytes INTEGER,
                duration_ms  INTEGER,
                timestamp    TEXT NOT NULL,
                parent_fn_name TEXT,
                success      INTEGER,
                memory_entry INTEGER,
                memory_exit  INTEGER,
                extra        TEXT NOT NULL DEFAULT '{}'
            )",
        )
        .execute(pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS trace_fn_logs (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                trace_id  TEXT NOT NULL,
                level     TEXT NOT NULL,
                message   TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                fn_name   TEXT
            )",
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    async fn write_trace_impl(pool: &sqlx::SqlitePool, trace: TraceHandle) {
        let status = format!("{:?}", trace.status());
        let expert_chain =
            serde_json::to_string(&trace.expert_chain_list()).unwrap_or_else(|_| "[]".to_string());
        let _ = sqlx::query(
            "INSERT OR REPLACE INTO trace_summaries \
             (trace_id,user_id,session_id,message,output,error,status,duration_ms,chain_name,expert_chain,created_at) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(trace.trace_id.as_str())
        .bind(trace.user_id())
        .bind(trace.session_id())
        .bind(trace.message())
        .bind(trace.output())
        .bind(trace.error())
        .bind(status)
        .bind(trace.duration_ms().map(|d| d as i64))
        .bind(trace.chain_name())
        .bind(expert_chain)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await;
    }

    async fn write_span_impl(pool: &sqlx::SqlitePool, trace_id: String, span: SpanData) {
        let extra = serde_json::to_string(&span.extra).unwrap_or_else(|_| "{}".to_string());
        let _ = sqlx::query(
            "INSERT INTO trace_spans \
             (trace_id,span_type,name,input,output,duration_ms,tokens,timestamp,success,extra) \
             VALUES (?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(trace_id)
        .bind(span.span_type)
        .bind(span.name)
        .bind(span.input)
        .bind(span.output)
        .bind(span.duration_ms.map(|d| d as i64))
        .bind(span.tokens.map(|t| t as i64))
        .bind(span.timestamp.to_rfc3339())
        .bind(span.success.map(|b| b as i64))
        .bind(extra)
        .execute(pool)
        .await;
    }

    async fn write_fn_call_impl(pool: &sqlx::SqlitePool, trace_id: String, fc: FnCallData) {
        let extra = serde_json::to_string(&fc.extra).unwrap_or_else(|_| "{}".to_string());
        let _ = sqlx::query(
            "INSERT INTO trace_fn_calls \
             (trace_id,fn_name,input,output,input_bytes,output_bytes,duration_ms,timestamp,parent_fn_name,success,memory_entry,memory_exit,extra) \
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(trace_id)
        .bind(fc.fn_name)
        .bind(fc.input)
        .bind(fc.output)
        .bind(fc.input_bytes.map(|b| b as i64))
        .bind(fc.output_bytes.map(|b| b as i64))
        .bind(fc.duration_ms.map(|d| d as i64))
        .bind(fc.timestamp.to_rfc3339())
        .bind(fc.parent_fn_name)
        .bind(fc.success.map(|b| b as i64))
        .bind(fc.memory_entry.map(|m| m as i64))
        .bind(fc.memory_exit.map(|m| m as i64))
        .bind(extra)
        .execute(pool)
        .await;
    }

    async fn write_fn_log_impl(pool: &sqlx::SqlitePool, trace_id: String, log: LogEntry) {
        let _ = sqlx::query(
            "INSERT INTO trace_fn_logs (trace_id,level,message,timestamp,fn_name) \
             VALUES (?,?,?,?,?)",
        )
        .bind(trace_id)
        .bind(log.level.to_string())
        .bind(log.message)
        .bind(log.timestamp.to_rfc3339())
        .bind(log.fn_name)
        .execute(pool)
        .await;
    }

    async fn read_summaries_impl(pool: &sqlx::SqlitePool) -> Vec<serde_json::Value> {
        let rows = sqlx::query(
            "SELECT trace_id,user_id,session_id,message,output,error,status,duration_ms,chain_name,expert_chain \
             FROM trace_summaries ORDER BY rowid ASC",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        rows.iter()
            .map(|row| {
                let expert_chain: Vec<String> = serde_json::from_str(
                    &row.try_get::<String, _>("expert_chain").unwrap_or_default(),
                )
                .unwrap_or_default();
                let duration: Option<i64> =
                    row.try_get::<Option<i64>, _>("duration_ms").ok().flatten();
                serde_json::json!({
                    "id": row.try_get::<String, _>("trace_id").unwrap_or_default(),
                    "user_id": row.try_get::<String, _>("user_id").unwrap_or_default(),
                    "session_id": row.try_get::<String, _>("session_id").unwrap_or_default(),
                    "input": row.try_get::<String, _>("message").unwrap_or_default(),
                    "output": row.try_get::<Option<String>, _>("output").ok().flatten(),
                    "status": row.try_get::<String, _>("status").unwrap_or_default(),
                    "total_duration_ms": duration.map(|d| d as u64),
                    "chain_name": row.try_get::<Option<String>, _>("chain_name").ok().flatten(),
                    "expert_chain": expert_chain,
                })
            })
            .collect()
    }

    async fn read_trace_impl(pool: &sqlx::SqlitePool, id: &str) -> Option<serde_json::Value> {
        let row = sqlx::query(
            "SELECT trace_id,user_id,session_id,message,output,error,status,duration_ms,chain_name,expert_chain \
             FROM trace_summaries WHERE trace_id=?",
        )
        .bind(id)
        .fetch_optional(pool)
        .await
        .ok()??;

        let expert_chain: Vec<String> =
            serde_json::from_str(&row.try_get::<String, _>("expert_chain").unwrap_or_default())
                .unwrap_or_default();
        let duration: Option<i64> = row.try_get::<Option<i64>, _>("duration_ms").ok().flatten();

        Some(serde_json::json!({
            "id": row.try_get::<String, _>("trace_id").unwrap_or_default(),
            "user_id": row.try_get::<String, _>("user_id").unwrap_or_default(),
            "session_id": row.try_get::<String, _>("session_id").unwrap_or_default(),
            "input": row.try_get::<String, _>("message").unwrap_or_default(),
            "output": row.try_get::<Option<String>, _>("output").ok().flatten(),
            "error": row.try_get::<Option<String>, _>("error").ok().flatten(),
            "total_duration_ms": duration.map(|d| d as u64),
            "chain_name": row.try_get::<Option<String>, _>("chain_name").ok().flatten(),
            "expert_chain": expert_chain,
            "status": row.try_get::<String, _>("status").unwrap_or_default(),
        }))
    }

    async fn read_spans_impl(pool: &sqlx::SqlitePool, id: &str) -> Vec<SpanData> {
        let rows = sqlx::query(
            "SELECT span_type,name,input,output,duration_ms,tokens,timestamp,success,extra \
             FROM trace_spans WHERE trace_id=? ORDER BY timestamp ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        rows.iter()
            .map(|row| {
                let extra_str: String = row.try_get("extra").unwrap_or_default();
                let extra: HashMap<String, String> =
                    serde_json::from_str(&extra_str).unwrap_or_default();
                SpanData {
                    span_type: row.try_get("span_type").unwrap_or_default(),
                    name: row.try_get("name").unwrap_or_default(),
                    input: row.try_get::<Option<String>, _>("input").ok().flatten(),
                    output: row.try_get::<Option<String>, _>("output").ok().flatten(),
                    duration_ms: row
                        .try_get::<Option<i64>, _>("duration_ms")
                        .ok()
                        .flatten()
                        .map(|v| v as u64),
                    tokens: row
                        .try_get::<Option<i64>, _>("tokens")
                        .ok()
                        .flatten()
                        .map(|v| v as u64),
                    timestamp: parse_datetime(
                        &row.try_get::<String, _>("timestamp").unwrap_or_default(),
                    ),
                    success: row
                        .try_get::<Option<i64>, _>("success")
                        .ok()
                        .flatten()
                        .map(|v| v != 0),
                    extra,
                }
            })
            .collect()
    }

    async fn read_fn_calls_impl(pool: &sqlx::SqlitePool, id: &str) -> Vec<FnCallData> {
        let rows = sqlx::query(
            "SELECT fn_name,input,output,input_bytes,output_bytes,duration_ms,timestamp,parent_fn_name,success,memory_entry,memory_exit,extra \
             FROM trace_fn_calls WHERE trace_id=? ORDER BY timestamp ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        rows.iter()
            .map(|row| {
                let extra_str: String = row.try_get("extra").unwrap_or_default();
                let extra: HashMap<String, String> =
                    serde_json::from_str(&extra_str).unwrap_or_default();
                FnCallData {
                    fn_name: row.try_get("fn_name").unwrap_or_default(),
                    input: row.try_get::<Option<String>, _>("input").ok().flatten(),
                    output: row.try_get::<Option<String>, _>("output").ok().flatten(),
                    input_bytes: row
                        .try_get::<Option<i64>, _>("input_bytes")
                        .ok()
                        .flatten()
                        .map(|v| v as usize),
                    output_bytes: row
                        .try_get::<Option<i64>, _>("output_bytes")
                        .ok()
                        .flatten()
                        .map(|v| v as usize),
                    duration_ms: row
                        .try_get::<Option<i64>, _>("duration_ms")
                        .ok()
                        .flatten()
                        .map(|v| v as u64),
                    timestamp: parse_datetime(
                        &row.try_get::<String, _>("timestamp").unwrap_or_default(),
                    ),
                    parent_fn_name: row
                        .try_get::<Option<String>, _>("parent_fn_name")
                        .ok()
                        .flatten(),
                    success: row
                        .try_get::<Option<i64>, _>("success")
                        .ok()
                        .flatten()
                        .map(|v| v != 0),
                    memory_entry: row
                        .try_get::<Option<i64>, _>("memory_entry")
                        .ok()
                        .flatten()
                        .map(|v| v as u64),
                    memory_exit: row
                        .try_get::<Option<i64>, _>("memory_exit")
                        .ok()
                        .flatten()
                        .map(|v| v as u64),
                    logs: vec![],
                    extra,
                }
            })
            .collect()
    }

    async fn read_fn_logs_impl(pool: &sqlx::SqlitePool, id: &str) -> Vec<LogEntry> {
        let rows = sqlx::query(
            "SELECT level,message,timestamp,fn_name FROM trace_fn_logs WHERE trace_id=? ORDER BY timestamp ASC",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        rows.iter()
            .map(|row| LogEntry {
                level: parse_log_level(&row.try_get::<String, _>("level").unwrap_or_default()),
                message: row.try_get("message").unwrap_or_default(),
                timestamp: parse_datetime(
                    &row.try_get::<String, _>("timestamp").unwrap_or_default(),
                ),
                fn_name: row.try_get::<Option<String>, _>("fn_name").ok().flatten(),
            })
            .collect()
    }
}

/// 后台 worker：拥有自己的 current-thread runtime 与 SQLite pool，
/// 通过命令通道处理所有读写请求。
fn worker_loop(
    path: String,
    mut rx: tokio_mpsc::UnboundedReceiver<TraceStoreMsg>,
    ready: std_mpsc::Sender<anyhow::Result<()>>,
) {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
    use std::str::FromStr;
    use std::time::Duration;

    // 父目录可能不存在（首次运行 ~/.subhuti/data 尚未创建），先建出来
    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let _ = ready.send(Err(anyhow::anyhow!(
                    "trace 数据目录创建失败 ({}): {}",
                    parent.display(),
                    e
                )));
                return;
            }
        }
    }

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = ready.send(Err(anyhow::anyhow!("trace runtime 创建失败: {}", e)));
            return;
        }
    };

    rt.block_on(async move {
        let opts = match SqliteConnectOptions::from_str(&path) {
            Ok(o) => o,
            Err(e) => {
                let _ = ready.send(Err(anyhow::anyhow!("SQLite 连接选项错误: {}", e)));
                return;
            }
        }
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

        let pool = match SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                let _ = ready.send(Err(anyhow::anyhow!("SQLite 连接失败: {}", e)));
                return;
            }
        };

        if let Err(e) = SqliteTraceStore::init_schema(&pool).await {
            let _ = ready.send(Err(anyhow::anyhow!("SQLite 建表失败: {}", e)));
            return;
        }

        // 初始化成功，通知 open() 可以返回
        let _ = ready.send(Ok(()));

        while let Some(msg) = rx.recv().await {
            match msg {
                TraceStoreMsg::WriteTrace(t, ack) => {
                    SqliteTraceStore::write_trace_impl(&pool, t).await;
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                }
                TraceStoreMsg::WriteSpan {
                    trace_id,
                    span,
                    ack,
                } => {
                    SqliteTraceStore::write_span_impl(&pool, trace_id, span).await;
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                }
                TraceStoreMsg::WriteFnCall { trace_id, fc, ack } => {
                    SqliteTraceStore::write_fn_call_impl(&pool, trace_id, fc).await;
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                }
                TraceStoreMsg::WriteFnLog { trace_id, log, ack } => {
                    SqliteTraceStore::write_fn_log_impl(&pool, trace_id, log).await;
                    if let Some(a) = ack {
                        let _ = a.send(());
                    }
                }
                TraceStoreMsg::ReadSummaries(ack) => {
                    let v = SqliteTraceStore::read_summaries_impl(&pool).await;
                    let _ = ack.send(v);
                }
                TraceStoreMsg::ReadTrace(id, ack) => {
                    let v = SqliteTraceStore::read_trace_impl(&pool, &id).await;
                    let _ = ack.send(v);
                }
                TraceStoreMsg::ReadSpans(id, ack) => {
                    let v = SqliteTraceStore::read_spans_impl(&pool, &id).await;
                    let _ = ack.send(v);
                }
                TraceStoreMsg::ReadFnCalls(id, ack) => {
                    let v = SqliteTraceStore::read_fn_calls_impl(&pool, &id).await;
                    let _ = ack.send(v);
                }
                TraceStoreMsg::ReadFnLogs(id, ack) => {
                    let v = SqliteTraceStore::read_fn_logs_impl(&pool, &id).await;
                    let _ = ack.send(v);
                }
            }
        }
    });
}

// ─── 行解码辅助 ──────────────────────────────────────────────────

fn parse_datetime(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_log_level(s: &str) -> LogLevel {
    match s {
        "TRACE" => LogLevel::Trace,
        "DEBUG" => LogLevel::Debug,
        "INFO" => LogLevel::Info,
        "WARN" => LogLevel::Warn,
        "ERROR" => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    /// 生成一个本次测试独占的临时库路径，并清掉旧文件，避免跨进程/跨用例串扰。
    fn test_db_path(suffix: &str) -> String {
        let mut p = std::env::temp_dir();
        p.push(format!("subhuti-trace-smoketest-{}.sqlite", suffix));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path));
        let _ = std::fs::remove_file(format!("{}-shm", path));
    }

    #[test]
    fn sqlite_store_roundtrip() {
        let path = test_db_path("roundtrip");
        let store = SqliteTraceStore::open(&path).expect("open sqlite store");

        let trace_id = "smoke-trace-001".to_string();
        let mut th = TraceHandle::new(
            trace_id.clone(),
            "user-1".into(),
            "sess-1".into(),
            "hello world".into(),
        );
        th.set_chain_name(Some("main".into()));
        th.set_expert_chain(Some(vec!["expert_a".into(), "expert_b".into()]));
        store.write_trace_sync(th);

        let now = Utc::now();
        store.write_span_fire_and_forget(
            trace_id.clone(),
            SpanData {
                span_type: "agent".into(),
                name: "expert_a".into(),
                input: Some("in".into()),
                output: Some("out".into()),
                duration_ms: Some(12),
                tokens: Some(34),
                timestamp: now,
                success: Some(true),
                extra: HashMap::new(),
            },
        );

        store.write_fn_call_fire_and_forget(
            trace_id.clone(),
            FnCallData {
                fn_name: "ChatPort::orchestrate".into(),
                input: Some("{\"q\":1}".into()),
                output: Some("{\"a\":2}".into()),
                input_bytes: Some(7),
                output_bytes: Some(7),
                duration_ms: Some(56),
                timestamp: now,
                parent_fn_name: None,
                success: Some(true),
                memory_entry: Some(1000),
                memory_exit: Some(1200),
                logs: vec![],
                extra: HashMap::new(),
            },
        );

        store.write_fn_log_fire_and_forget(
            trace_id.clone(),
            LogEntry {
                level: LogLevel::Info,
                message: "fn started".into(),
                timestamp: now,
                fn_name: Some("ChatPort::orchestrate".into()),
            },
        );

        // 读回并断言
        let summaries = store.read_summaries_sync();
        assert_eq!(summaries.len(), 1, "应写入 1 条 summary");
        assert_eq!(summaries[0]["id"], trace_id);
        assert_eq!(summaries[0]["input"], "hello world");
        assert_eq!(summaries[0]["status"], "InProgress");
        assert_eq!(
            summaries[0]["expert_chain"],
            serde_json::json!(["expert_a", "expert_b"])
        );

        let trace = store.read_trace_sync(&trace_id).expect("read trace");
        assert_eq!(trace["id"], trace_id);
        assert_eq!(trace["chain_name"], "main");

        let spans = store.read_spans_sync(&trace_id);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "expert_a");
        assert_eq!(spans[0].tokens, Some(34));
        assert_eq!(spans[0].success, Some(true));

        let calls = store.read_fn_calls_sync(&trace_id);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].fn_name, "ChatPort::orchestrate");
        assert_eq!(calls[0].memory_entry, Some(1000));
        assert_eq!(calls[0].memory_exit, Some(1200));

        let logs = store.read_fn_logs_sync(&trace_id);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, LogLevel::Info);
        assert_eq!(logs[0].fn_name.as_deref(), Some("ChatPort::orchestrate"));

        cleanup(&path);
    }

    /// 验证两个 store 实例（模拟 HTTP 进程 + MCP 进程）写入同一文件会汇聚。
    #[test]
    fn two_stores_converge_on_same_file() {
        let path = test_db_path("converge");
        let store_a = SqliteTraceStore::open(&path).expect("open A");
        let store_b = SqliteTraceStore::open(&path).expect("open B");

        let id_a = "trace-A".to_string();
        let mut th_a = TraceHandle::new(id_a.clone(), "u".into(), "s".into(), "A".into());
        th_a.set_chain_name(Some("chain-a".into()));
        store_a.write_trace_sync(th_a);

        let id_b = "trace-B".to_string();
        let mut th_b = TraceHandle::new(id_b.clone(), "u".into(), "s".into(), "B".into());
        th_b.set_chain_name(Some("chain-b".into()));
        store_b.write_trace_sync(th_b);

        // 从 B 的视角也能看到 A 写入的数据（共享库汇聚）
        let summaries = store_b.read_summaries_sync();
        assert_eq!(summaries.len(), 2, "两个进程写入应汇聚到同一库");
        let ids: Vec<String> = summaries
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect();
        assert!(ids.contains(&id_a));
        assert!(ids.contains(&id_b));

        cleanup(&path);
    }
}
