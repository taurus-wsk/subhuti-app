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
//!
//! ## 工具层设计（本模块内部）
//!
//! - **声明式注册**：`Tool` 枚举是 tool 的单一事实来源，`Tool::def()` 产出
//!   name/description/input_schema，`list_tools_result()` 与 `call_tool()` 都从它派生，
//!   消除「schema 一份、参数解析另一份」的双份硬编码。
//! - **参数强类型**：每个工具一个 `#[derive(Deserialize)]` 参数 struct，
//!   serde 直接反序列化 + 必填校验，告别手写 `args.get("x").and_then(...)`。
//! - **流式**：`subhuti_chat` 走 `orchestrate_stream()`，`StreamEvent::Step` 经
//!   MCP `notifications/progress` 透出（对齐 HTTP SSE）；无 progressToken 时降级 stderr 日志。
//! - **真取消**：request id → `AbortHandle` 注册表，`notifications/cancelled` 触发 abort。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;
use tokio::task::{AbortHandle, JoinSet};
use tracing_subscriber::prelude::*;

use crate::application::composition_root::Composition;
use crate::application::ports::StreamEvent;
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
        "🚀 Subhuti MCP server 已就绪（并发上限={}，协议版本={}，构建={} v{}）",
        limit,
        PROTOCOL_VERSION,
        env!("SUBHUTI_BUILD_TIME"),
        env!("CARGO_PKG_VERSION")
    );
    eprintln!(
        "   可用工具: subhuti_chat / subhuti_list_experts / subhuti_match_expert / subhuti_memory"
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
    // 这里改为：业务任务只负责把响应字符串（含 progress notification）`send` 进
    // std::sync::mpsc，由专用线程同步写出。既不触碰该缺陷，也天然保序（FIFO），
    // 且不占用 tokio worker。
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

    // 请求取消注册表：request id → AbortHandle。notifications/cancelled 据此 abort 对应任务。
    let cancel_registry: Arc<Mutex<HashMap<String, AbortHandle>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let mut tasks: JoinSet<()> = JoinSet::new();

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            break; // EOF：客户端断开，等 spawned task 完成
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

        // 取消通知（无 id）：abort 对应 in-flight 请求，无需 spawn、无需回响应。
        // 读取循环是单线程串行的，故「spawn 后注册句柄」早于「处理下一次取消」，无竞态。
        if req.method == "notifications/cancelled" {
            if let Some(rid) = req.params.as_ref().and_then(|p| p.get("requestId")) {
                let key = id_key(rid);
                if let Some(handle) = cancel_registry.lock().unwrap().remove(&key) {
                    handle.abort();
                    eprintln!("⏹️ 已取消请求 {}", key);
                }
            }
            continue;
        }

        let comp = comp.clone();
        let sem = sem.clone();
        let stdout_tx = stdout_tx.clone();
        let registry = cancel_registry.clone();
        let cancel_key = req.id.as_ref().map(id_key);
        let cancel_key_cleanup = cancel_key.clone();

        // 并发处理（受 Semaphore 限流），每个请求独立 spawn；返回 AbortHandle 用于注册取消
        let abort_handle = tasks.spawn(async move {
            let _permit = sem.acquire().await.ok();
            // handle 内部流式编排需要 progress notification 通道，故克隆一份 sender 传入
            if let Some(resp) = handle(req, &comp, stdout_tx.clone()).await {
                let _ = stdout_tx.send(resp);
            }
            // 收敛（含被 abort）后清理注册表，避免句柄泄漏
            if let Some(k) = &cancel_key_cleanup {
                registry.lock().unwrap().remove(k);
            }
        });

        if let Some(k) = cancel_key {
            cancel_registry.lock().unwrap().insert(k, abort_handle);
        }
    }

    // EOF 后等待所有 spawned 请求完成（最多 30 秒）
    tokio::select! {
        _ = async { while tasks.join_next().await.is_some() {} } => {}
        _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
            eprintln!("⚠️ 部分请求超时未完成，放弃等待");
        }
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

/// 归一化 JSON-RPC id 为注册表键：`"1"`（字符串）与 `1`（数字）统一成 `1`，避免类型失配。
fn id_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

async fn handle(
    req: JsonRpcRequest,
    comp: &Composition,
    stdout_tx: std::sync::mpsc::Sender<String>,
) -> Option<String> {
    match req.method.as_str() {
        "initialize" => Some(make_response(req.id, initialize_result())),
        // notification：无 id，不回响应
        "notifications/initialized" => None,
        "ping" => Some(make_response(req.id, json!({}))),
        "tools/list" => Some(make_response(req.id, list_tools_result())),
        "tools/call" => {
            let params = req.params.unwrap_or(Value::Null);
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            // MCP 进度协商：client 在 _meta.progressToken 提供 token 后，编排阶段经
            // notifications/progress 透出（与 HTTP SSE 对齐）；无 token 则降级 stderr 日志。
            let progress_token = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .cloned();
            let mut reporter = ProgressReporter::new(progress_token, stdout_tx);

            match Tool::from_name(name) {
                Some(tool) => match call_tool(tool, arguments, comp, &mut reporter).await {
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
                },
                None => Some(make_response(
                    req.id,
                    json!({
                        "content": [{ "type": "text", "text": format!("❌ 未知工具: {}", name) }],
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
        "serverInfo": {
            "name": "subhuti",
            "version": env!("CARGO_PKG_VERSION"),
            "buildTime": env!("SUBHUTI_BUILD_TIME"),
        },
    })
}

// ─── 工具注册表（声明式：name/description/schema 单一来源）────────

/// 工具名枚举：编译期穷尽匹配（同 ReactStage 思路），工具的唯一调度入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Chat,
    ListExperts,
    MatchExpert,
    Memory,
}

/// 工具的静态注册信息。
struct ToolDef {
    name: &'static str,
    description: &'static str,
    input_schema: Value,
}

impl Tool {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "subhuti_chat" => Some(Tool::Chat),
            "subhuti_list_experts" => Some(Tool::ListExperts),
            "subhuti_match_expert" => Some(Tool::MatchExpert),
            "subhuti_memory" => Some(Tool::Memory),
            _ => None,
        }
    }

    /// schema 与实现同源：name/description/input_schema 与参数 struct 在此处一并定义，
    /// `list_tools_result()` 与 `call_tool()` 都从它派生，消除双份硬编码。
    fn def(self) -> ToolDef {
        match self {
            Tool::Chat => ToolDef {
                name: "subhuti_chat",
                description: "向 Subhuti 智能体提问，触发编排（专家路由 + 藏经阁 RAG）。流式返回进度与最终输出。",
                input_schema: ChatArgs::schema(),
            },
            Tool::ListExperts => ToolDef {
                name: "subhuti_list_experts",
                description: "列出 Subhuti 当前注册的所有领域专家（含 tags）。",
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            Tool::MatchExpert => ToolDef {
                name: "subhuti_match_expert",
                description: "根据问题描述匹配最合适的专家。",
                input_schema: MatchExpertArgs::schema(),
            },
            Tool::Memory => ToolDef {
                name: "subhuti_memory",
                description: "读写藏经阁长期记忆。action=recall 检索；write 写入（自动抽取+持久化）；stats 统计；collections 集合列表。**每个会话开始时至少调一次 recall**。",
                input_schema: MemoryArgs::schema(),
            },
        }
    }
}

fn list_tools_result() -> Value {
    let tools: Vec<Value> = [
        Tool::Chat,
        Tool::ListExperts,
        Tool::MatchExpert,
        Tool::Memory,
    ]
    .into_iter()
    .map(|t| {
        let d = t.def();
        json!({
            "name": d.name,
            "description": d.description,
            "inputSchema": d.input_schema,
        })
    })
    .collect();
    json!({ "tools": tools })
}

// ─── 工具参数 struct（serde 强类型反序列化 + 必填校验）────────────

#[derive(Debug, Deserialize)]
struct ChatArgs {
    message: String,
    #[serde(default)]
    expert_id: Option<String>,
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    workspace_folder: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    extra: Option<Value>,
}

impl ChatArgs {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "用户问题 / 任务" },
                "expert_id": { "type": "string", "description": "强制路由到指定专家 ID（可选）" },
                "skill_id": { "type": "string", "description": "显式指定该专家下的技能 ID，直接执行该技能、跳过内部LLM技能规划（可选）。未传或无效时由内部LLM规划兜底" },
                "system_prompt": { "type": "string", "description": "覆盖默认系统提示词（可选）" },
                "workspace_folder": { "type": "string", "description": "项目工作目录（可选）" },
                "session_id": { "type": "string", "description": "**多轮会话必传**。同一对话的所有调用必须复用同一个 session_id，才能保持上下文连续。首轮不传会自动生成并返回。" }
            },
            "required": ["message"]
        })
    }
}

#[derive(Debug, Deserialize)]
struct MatchExpertArgs {
    message: String,
}

impl MatchExpertArgs {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string", "description": "用于匹配专家的问题" }
            },
            "required": ["message"]
        })
    }
}

#[derive(Debug, Deserialize)]
struct MemoryArgs {
    action: String,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    top_k: Option<u64>,
}

impl MemoryArgs {
    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["recall", "write", "stats", "collections"],
                    "description": "recall=检索, write=写入, stats=统计, collections=集合"
                },
                "query": { "type": "string", "description": "recall 的检索词" },
                "content": { "type": "string", "description": "write 的记忆正文" },
                "domain": { "type": "string", "description": "领域（rust/blender/general），默认 general" },
                "top_k": { "type": "integer", "description": "recall 返回条数，默认 5" }
            },
            "required": ["action"]
        })
    }
}

// ─── 工具实现（直接复用入站端口）────────────────────────────────

async fn call_tool(
    tool: Tool,
    args: Value,
    comp: &Composition,
    progress: &mut ProgressReporter,
) -> anyhow::Result<String> {
    match tool {
        Tool::Chat => {
            let a: ChatArgs = serde_json::from_value(args)?;
            chat_stream(a, comp, progress).await
        }
        Tool::ListExperts => list_experts(comp).await,
        Tool::MatchExpert => {
            let a: MatchExpertArgs = serde_json::from_value(args)?;
            match_expert(a, comp).await
        }
        Tool::Memory => {
            let a: MemoryArgs = serde_json::from_value(args)?;
            memory(a, comp).await
        }
    }
}

async fn list_experts(comp: &Composition) -> anyhow::Result<String> {
    let experts = comp.expert_port.list_experts().await;
    if experts.is_empty() {
        Ok("(无已注册专家)".to_string())
    } else {
        let mut s = String::from("已注册专家:\n");
        for e in experts {
            s.push_str(&format!("- {} (id={}): tags={:?}\n", e.name, e.id, e.tags));
            if !e.skills.is_empty() {
                let skills = e
                    .skills
                    .iter()
                    .map(|sk| format!("{}({})", sk.name, sk.id))
                    .collect::<Vec<_>>()
                    .join(", ");
                s.push_str(&format!("    技能: {}\n", skills));
            }
        }
        Ok(s)
    }
}

async fn match_expert(a: MatchExpertArgs, comp: &Composition) -> anyhow::Result<String> {
    let experts = comp.expert_port.match_expert(&a.message).await;
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

async fn memory(a: MemoryArgs, comp: &Composition) -> anyhow::Result<String> {
    let sutra = comp
        .sutra_library
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("藏经阁未初始化"))?;
    match a.action.as_str() {
        "recall" => {
            let q = a
                .query
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("recall 需要 'query' 参数"))?;
            let top_k = a.top_k.unwrap_or(5) as usize;
            Ok(sutra.library_retrieve(q, top_k).await)
        }
        "write" => {
            let content = a
                .content
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("write 需要 'content' 参数"))?;
            if content.trim().is_empty() {
                return Err(anyhow::anyhow!("content 不能为空"));
            }
            let domain = a.domain.as_deref().unwrap_or("general");
            // 从内容提取标题（第一句或前24字）
            let title = content.lines().next().unwrap_or("").trim().to_string();
            let title = if title.chars().count() > 24 {
                title.chars().take(24).collect()
            } else {
                title
            };
            let entries = vec![(title, content.to_string())];
            let n = sutra.seed_knowledge(domain, &entries).await;
            Ok(format!("✅ 已写入 {} 条新记忆到 {} 领域", n, domain))
        }
        "stats" => {
            let json = sutra.stats_json();
            Ok(serde_json::to_string_pretty(&json).unwrap_or_else(|_| "统计数据解析失败".into()))
        }
        "collections" => Ok(sutra.list_collections()),
        other => Err(anyhow::anyhow!("未知 action: {}", other)),
    }
}

/// 流式编排（subhuti_chat）：消费 `StreamEvent`，Step 经 progress notification 透出，
/// Chunk 累积、Done 取最终输出与 meta，Ask 降级为提示并入输出。
async fn chat_stream(
    a: ChatArgs,
    comp: &Composition,
    progress: &mut ProgressReporter,
) -> anyhow::Result<String> {
    let session_id = a
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let req = OrchestrateRequest {
        message: a.message.clone(),
        user_id: Some("mcp".to_string()),
        session_id: Some(session_id.clone()),
        chain: None,
        expert_id: a.expert_id.clone(),
        trace_id: None,
        system_prompt: a.system_prompt.clone(),
        extra: build_extra(
            a.workspace_folder.clone(),
            a.skill_id.clone(),
            a.extra.clone(),
        ),
    };

    let mut rx = comp.chat_port.orchestrate_stream(req);

    let mut output = String::new();
    let mut meta = Value::Null;
    let mut error: Option<String> = None;

    while let Some(ev) = rx.recv().await {
        match ev {
            StreamEvent::Start => {}
            StreamEvent::Step {
                message, source, ..
            } => {
                progress.report(&format!("[{}] {}", source, message));
            }
            StreamEvent::Chunk { content } => {
                output.push_str(&content);
            }
            StreamEvent::Ask {
                question, options, ..
            } => {
                // MCP tools/call 是单次 request/response，无法人机交互；把提问降级为待确认项并入输出
                let opts = if options.is_empty() {
                    String::new()
                } else {
                    format!(" (选项: {})", options.join(" / "))
                };
                output.push_str(&format!("\n[待确认问题] {}{}\n", question, opts));
            }
            StreamEvent::Done {
                output: out,
                meta: m,
            } => {
                output = out;
                meta = m;
            }
            StreamEvent::Error { error: e } => {
                error = Some(e);
            }
        }
    }

    if let Some(e) = error {
        return Err(anyhow::anyhow!("编排失败: {}", e));
    }

    Ok(format_output(output, &meta, &session_id))
}

/// 把 workspace_folder 与 extra 合并进 DTO.extra（与旧一次性路径语义一致）。
fn build_extra(
    workspace_folder: Option<String>,
    skill_id: Option<String>,
    extra: Option<Value>,
) -> Option<Value> {
    let mut extra_obj = serde_json::Map::new();
    if let Some(ws) = workspace_folder {
        extra_obj.insert("workspace_folder".into(), Value::String(ws));
    }
    // skill_id 透传进 metadata，由 domain_expert_adapter 读入并走 execute_skill 直达；
    // 无效值由领域层兜底回收回内部规划，不在此校验。
    if let Some(sid) = skill_id {
        extra_obj.insert("skill_id".into(), Value::String(sid));
    }
    if let Some(extra_val) = extra {
        if let Some(obj) = extra_val.as_object() {
            for (k, v) in obj {
                extra_obj.insert(k.clone(), v.clone());
            }
        }
    }
    if extra_obj.is_empty() {
        None
    } else {
        Some(Value::Object(extra_obj))
    }
}

/// 组装最终输出：正文 + `— — —` meta 页脚。
fn format_output(output: String, meta: &Value, session_id: &str) -> String {
    let chain: Vec<String> = meta
        .get("chain")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let duration_ms = meta
        .get("duration_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let tokens = meta
        .get("tokens_used")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let llm_calls = meta.get("llm_calls").and_then(|v| v.as_u64()).unwrap_or(0);
    format!(
        "{}\n\n— — —\n[meta] 专家链={:?} 耗时={}ms tokens={} llm_calls={} session_id={}",
        output, chain, duration_ms, tokens, llm_calls, session_id
    )
}

// ─── MCP 进度通知 ────────────────────────────────────────────────

/// 编排阶段进度透出器：持有 progressToken 时经 `notifications/progress` 发往客户端，
/// 无 token 时降级为 stderr 日志（不影响 stdout 协议通道）。
struct ProgressReporter {
    token: Option<Value>,
    tx: std::sync::mpsc::Sender<String>,
    step: usize,
}

impl ProgressReporter {
    fn new(token: Option<Value>, tx: std::sync::mpsc::Sender<String>) -> Self {
        Self { token, tx, step: 0 }
    }

    fn report(&mut self, message: &str) {
        match &self.token {
            None => {
                eprintln!("ℹ️ [progress] {}", message);
            }
            Some(token) => {
                self.step += 1;
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/progress",
                    "params": {
                        "progressToken": token,
                        "progress": self.step,
                        "message": message,
                    }
                });
                let _ = self
                    .tx
                    .send(serde_json::to_string(&notif).unwrap_or_default());
            }
        }
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
