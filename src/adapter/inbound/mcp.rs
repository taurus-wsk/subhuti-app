//! # MCP Server 适配器（stdio）
//!
//! 把 Subhuti 的三个入站端口（chat / expert / skill）通过 MCP 协议暴露给
//! WorkBuddy 或其他 MCP client 调用。`mcp::run` 直接复用 `CompositionRoot`，
//! **不经过 HTTP**，避免「自己调自己」。
//!
//! 协议：stdio 上的 newline-delimited JSON-RPC 2.0（MCP 2024-11-05）。
//!
//! ⚠️ 关键约束：**stdout 只输出 JSON-RPC 协议消息**，所有日志走 stderr / 文件，
//! 绝不写入 stdout，否则会破坏 WorkBuddy 对 JSON-RPC 流的解析。

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;
use tracing_subscriber::prelude::*;

use crate::application::composition_root::Composition;
use crate::application::CompositionRoot;
use crate::domain::dto::OrchestrateRequest;
use crate::infra::config::AppConfig;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// 启动 MCP server（stdio 模式）
///
/// - `debug` / `log_level`：控制日志级别（日志走 stderr，不影响 stdout 协议通道）
/// - `concurrency`：并发工具调用上限（限流，防止本地 LLM 被同时打爆），默认 2
pub async fn run(
    debug: bool,
    log_level: Option<String>,
    concurrency: Option<usize>,
) -> anyhow::Result<()> {
    // 加载 .env（LLM API key 等），与 HTTP server 一致
    dotenvy::dotenv().ok();

    init_mcp_logging(debug, log_level.as_deref());

    let app_config = AppConfig::load().unwrap_or_else(|e| {
        eprintln!("⚠️ 配置加载失败: {}，使用默认配置", e);
        crate::infra::config::default_config()
    });

    let composition = CompositionRoot::build(&app_config).await?;
    let comp = Arc::new(composition);

    let limit = concurrency.unwrap_or(2).max(1);
    let sem = Arc::new(Semaphore::new(limit));

    eprintln!(
        "🚀 Subhuti MCP server 已就绪（并发上限={}，协议版本={}）",
        limit, PROTOCOL_VERSION
    );
    eprintln!(
        "   可用工具: subhuti_chat / subhuti_list_experts / subhuti_match_expert / subhuti_skill_list / subhuti_skill_run"
    );

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);

    // ⚠️ stdout 走「专用 OS 线程 + 标准库 stdout」，不用 `tokio::io::stdout()`。
    //
    // 原因：`tokio::io::stdout()` 内部是 `Blocking<Stdout>`，它在写/flush 时会 poll
    // 内部那个一次性 JoinHandle；在「每请求独立 spawn + Mutex 共享 stdout」的场景下
    // `flush()` 会二次 poll 已完成的任务，直接 panic：
    //     `JoinHandle polled after completion`（tokio 已知缺陷）
    // 表现为：MCP 启动正常，但第 2 个响应开始就再也写不出去。
    //
    // 这里改为：业务任务只负责把响应字符串 `send` 进 std::sync::mpsc，由专用线程
    // 同步写出。既不触碰该缺陷，也天然保序（FIFO），且不占用 tokio worker。
    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::Write;
        let mut out = std::io::stdout();
        while let Ok(msg) = stdout_rx.recv() {
            let _ = out.write_all(msg.as_bytes());
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
    });

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF：客户端断开
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("⚠️ 无法解析 JSON-RPC 请求: {}", e);
                continue;
            }
        };

        let comp = comp.clone();
        let sem = sem.clone();
        let stdout_tx = stdout_tx.clone();
        // 并发处理（受 Semaphore 限流），每个请求独立 spawn
        tokio::spawn(async move {
            let _permit = sem.acquire().await.ok();
            if let Some(resp) = handle(req, &comp).await {
                // 交给专用写线程；通道关闭（进程退出）时静默丢弃
                let _ = stdout_tx.send(resp);
            }
        });
    }

    Ok(())
}

// ─── JSON-RPC 协议 ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

fn make_response(id: Option<Value>, result: Value) -> String {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    });
    serde_json::to_string(&resp).unwrap_or_default()
}

fn make_error(id: Option<Value>, code: i32, message: &str) -> String {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    });
    serde_json::to_string(&resp).unwrap_or_default()
}

async fn handle(req: JsonRpcRequest, comp: &Composition) -> Option<String> {
    match req.method.as_str() {
        "initialize" => Some(make_response(req.id, initialize_result())),
        // notification：无 id，不回响应
        "notifications/initialized" => None,
        "notifications/cancelled" => None,
        "ping" => Some(make_response(req.id, json!({}))),
        "tools/list" => Some(make_response(req.id, list_tools_result())),
        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            match call_tool(name, arguments, comp).await {
                Ok(text) => Some(make_response(
                    req.id,
                    json!({
                        "content": [{ "type": "text", "text": text }],
                        "isError": false,
                    }),
                )),
                Err(e) => Some(make_response(
                    req.id,
                    json!({
                        "content": [{ "type": "text", "text": format!("❌ 工具执行失败: {}", e) }],
                        "isError": true,
                    }),
                )),
            }
        }
        // 未知方法：notification（无 id）静默忽略；request 返回 method not found
        _ => {
            if req.id.is_none() {
                None
            } else {
                Some(make_error(
                    req.id,
                    -32601,
                    &format!("Method not found: {}", req.method),
                ))
            }
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "subhuti", "version": "0.1.0" },
    })
}

fn list_tools_result() -> Value {
    json!({
        "tools": [
            {
                "name": "subhuti_chat",
                "description": "向 Subhuti 智能体提问，触发编排（专家路由 + 技能执行 + 藏经阁 RAG）。返回最终输出及所用专家链。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string", "description": "用户问题 / 任务" },
                        "expert_id": { "type": "string", "description": "强制路由到指定专家 ID（可选）" },
                        "graph": { "type": "string", "description": "指定图编排名称（可选）" },
                        "workspace_folder": { "type": "string", "description": "项目工作目录（可选）" },
                        "system_prompt": { "type": "string", "description": "覆盖默认系统提示词（可选）" },
                        "session_id": { "type": "string", "description": "会话 ID，用于多轮上下文（可选）" }
                    },
                    "required": ["message"]
                }
            },
            {
                "name": "subhuti_list_experts",
                "description": "列出 Subhuti 当前注册的所有领域专家（含 tags）。",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "subhuti_match_expert",
                "description": "根据问题描述匹配最合适的专家。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": { "type": "string", "description": "用于匹配专家的问题" }
                    },
                    "required": ["message"]
                }
            },
            {
                "name": "subhuti_skill_list",
                "description": "列出所有专家暴露的可用技能。",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "subhuti_skill_run",
                "description": "按 ID 执行某个技能，args 作为技能输入。",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "skill_id": { "type": "string", "description": "技能 ID" },
                        "args": { "type": "string", "description": "传给技能的参数 / 输入" }
                    },
                    "required": ["skill_id"]
                }
            }
        ]
    })
}

// ─── 工具实现（直接复用三个入站端口）───────────────────────────────

async fn call_tool(name: &str, args: Value, comp: &Composition) -> anyhow::Result<String> {
    match name {
        "subhuti_chat" => {
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少必填参数 'message'"))?;

            let req = OrchestrateRequest {
                message: message.to_string(),
                user_id: Some("mcp".to_string()),
                session_id: Some(uuid::Uuid::new_v4().to_string()),
                chain: None,
                graph: args.get("graph").and_then(|v| v.as_str()).map(String::from),
                expert_id: args
                    .get("expert_id")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                trace_id: None,
                workspace_folder: args
                    .get("workspace_folder")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                system_prompt: args
                    .get("system_prompt")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };

            let resp = comp.chat_port.orchestrate(req).await;
            if resp.success {
                Ok(format!(
                    "{}\n\n— — —\n[meta] 专家链={:?} 耗时={}ms trace_id={} session_id={}",
                    resp.output,
                    resp.expert_chain,
                    resp.duration_ms,
                    resp.trace_id,
                    resp.session_id
                ))
            } else {
                Ok(format!("❌ 编排失败: {}", resp.error.unwrap_or_default()))
            }
        }
        "subhuti_list_experts" => {
            let experts = comp.expert_port.list_experts().await;
            if experts.is_empty() {
                Ok("(无已注册专家)".to_string())
            } else {
                let mut s = String::from("已注册专家:\n");
                for e in experts {
                    s.push_str(&format!("- {} (id={}): tags={:?}\n", e.name, e.id, e.tags));
                }
                Ok(s)
            }
        }
        "subhuti_match_expert" => {
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少必填参数 'message'"))?;
            let experts = comp.expert_port.match_expert(message).await;
            if experts.is_empty() {
                Ok("未匹配到专家".to_string())
            } else {
                let mut s = String::from("匹配到的专家:\n");
                for e in experts {
                    s.push_str(&format!("- {} (id={}): tags={:?}\n", e.name, e.id, e.tags));
                }
                Ok(s)
            }
        }
        "subhuti_skill_list" => {
            let skills = comp.skill_port.skill_list().await;
            if skills.is_empty() {
                Ok("(无可用技能)".to_string())
            } else {
                let mut s = String::from("可用技能:\n");
                for sk in skills {
                    s.push_str(&format!(
                        "- {} (id={}): {}\n",
                        sk.name, sk.id, sk.description
                    ));
                }
                Ok(s)
            }
        }
        "subhuti_skill_run" => {
            let skill_id = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少必填参数 'skill_id'"))?;
            let args_str = args.get("args").and_then(|v| v.as_str()).unwrap_or("");
            let resp = comp
                .skill_port
                .execute_skill(skill_id, args_str, "", "")
                .await;
            if resp.success {
                Ok(resp.output)
            } else {
                Ok(format!(
                    "❌ 技能执行失败: {}",
                    resp.error.unwrap_or_default()
                ))
            }
        }
        other => Err(anyhow::anyhow!("未知工具: {}", other)),
    }
}

// ─── 日志（仅 stderr / 文件，绝不写 stdout）─────────────────────────

fn init_mcp_logging(debug: bool, log_level: Option<&str>) {
    let level = log_level.unwrap_or(if debug { "debug" } else { "info" });
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new(format!(
            "{},subhuti=debug,tower_http=off,hyper=off,reqwest=off",
            level
        ))
    });

    // stderr：客户端可见的日志，不影响 stdout 协议通道
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    // 文件：可选持久化（WorkerGuard 必须保持存活，否则日志线程退出）
    let file_appender = tracing_appender::rolling::never("./logs", "mcp.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(guard);
    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(file_writer);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer)
        .with(file_layer)
        .try_init();
}
