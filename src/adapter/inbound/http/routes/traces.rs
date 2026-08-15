use axum::{
    extract::{Json, Path, Query, State},
    response::IntoResponse,
    routing::get,
};

use super::AppState;
use crate::adapter::inbound::http::adapters::ApiError;
use crate::adapter::inbound::http::route_adapter::RouteEntry;

pub async fn traces_list_handler(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let summaries = state.trace_observer.list_summaries();

    let format = params.get("format").map(|s| s.as_str()).unwrap_or("json");

    if format == "html" {
        let html = generate_traces_list_html();
        return (
            axum::http::StatusCode::OK,
            [("Content-Type", "text/html; charset=utf-8")],
            html,
        )
            .into_response();
    }

    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": summaries,
            "total": summaries.len(),
        })),
    )
        .into_response()
}

pub async fn traces_get_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    match state.trace_observer.get_trace(&id) {
        Some(trace) => {
            let format = params.get("format").map(|s| s.as_str()).unwrap_or("json");

            if format == "html" {
                let html = generate_trace_detail_html(&trace);
                return (
                    axum::http::StatusCode::OK,
                    [("Content-Type", "text/html; charset=utf-8")],
                    html,
                )
                    .into_response();
            }

            (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "data": trace,
                })),
            )
                .into_response()
        }
        None => ApiError::not_found("Trace not found").into_response(),
    }
}

pub async fn traces_tree_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.trace_observer.get_span_tree(&id) {
        Some(tree) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": tree,
            })),
        ),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Trace not found",
                "code": 404,
            })),
        ),
    }
}

/// GET /subhuti/api/v1/traces/:id/fn_call_tree
///
/// 返回函数调用链路树（JSON 格式）。
/// 每个节点包含函数名、输入输出数据、耗时、数据大小。
pub async fn fn_call_tree_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.trace_observer.get_fn_call_tree(&id) {
        Some(tree) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": tree,
            })),
        ),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Fn call tree not found (no function calls recorded for this trace)",
                "code": 404,
            })),
        ),
    }
}

/// GET /subhuti/api/v1/traces/:id/fn_call_report?format=html
///
/// 生成函数调用链路 HTML 报告，包含函数调用树、执行时间、传入传出数据、数据大小。
/// 如果 `id` 是 `last`，自动返回最后一次请求的 trace。
pub async fn fn_call_report_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let actual_id = if id == "last" {
        // 获取所有 trace 摘要，取最后一个
        let summaries = state.trace_observer.list_summaries();
        match summaries.last() {
            Some(last) => last["id"].as_str().unwrap_or(""),
            None => "",
        }
        .to_string()
    } else {
        id
    };

    if actual_id.is_empty() {
        return ApiError::not_found("No traces found (no requests have been made yet)")
            .into_response();
    }

    let trace = state.trace_observer.get_trace(&actual_id);
    let fn_tree = state.trace_observer.get_fn_call_tree(&actual_id);

    let html = generate_fn_call_report_html(&actual_id, trace.as_ref(), fn_tree.as_ref());
    (
        axum::http::StatusCode::OK,
        [("Content-Type", "text/html; charset=utf-8")],
        html,
    )
        .into_response()
}

fn generate_traces_list_html() -> String {
    String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trace 列表 - Subhuti</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 3px solid #4CAF50; padding-bottom: 10px; }
        p { color: #666; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔍 Trace 列表</h1>
        <p>Trace 列表功能已迁移到业务埋点层，调试链路请使用标准 tracing 生态工具。</p>
        <p>可通过 <code>RUST_LOG=debug</code> 环境变量查看详细日志，配合 <code>jq</code> 按 trace_id 过滤。</p>
    </div>
</body>
</html>"#,
    )
}

fn generate_trace_detail_html(trace: &serde_json::Value) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trace 详情 - Subhuti</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif; margin: 40px; background: #f5f5f5; }
        .container { max-width: 1200px; margin: 0 auto; background: white; padding: 30px; border-radius: 8px; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }
        h1 { color: #333; border-bottom: 3px solid #4CAF50; padding-bottom: 10px; }
        .info { background: #e8f5e9; padding: 15px; border-radius: 5px; margin: 15px 0; }
        .info p { margin: 8px 0; }
        .badge { padding: 4px 8px; border-radius: 4px; font-size: 12px; }
        .success { background: #4CAF50; color: white; }
        .error { background: #f44336; color: white; }
        pre { background: #f5f5f5; padding: 15px; border-radius: 5px; overflow-x: auto; }
        a { color: #4CAF50; text-decoration: none; }
        .expert-chain { margin: 8px 0; padding: 4px 8px; background: #fff3e0; border-radius: 4px; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🔍 Trace 详情</h1>"#,
    );

    html.push_str("<div class='info'>");
    let trace_id = trace["id"].as_str().unwrap_or("");
    html.push_str(&format!("<p><strong>Trace ID:</strong> {}</p>", trace_id));

    let user_id = trace["user_id"].as_str().unwrap_or("");
    html.push_str(&format!("<p><strong>用户 ID:</strong> {}</p>", user_id));

    let session_id = trace["session_id"].as_str().unwrap_or("");
    html.push_str(&format!("<p><strong>会话 ID:</strong> {}</p>", session_id));

    let input = trace["input"].as_str().unwrap_or("");
    html.push_str(&format!("<p><strong>用户输入:</strong> {}</p>", input));

    let duration_text = trace["total_duration_ms"]
        .as_u64()
        .map(|d| format!("{}ms ({:.1}秒)", d, d as f64 / 1000.0))
        .unwrap_or_else(|| "-".to_string());
    html.push_str(&format!(
        "<p><strong>总耗时:</strong> {}</p>",
        duration_text
    ));

    if let Some(chain_name) = trace["chain_name"].as_str() {
        html.push_str(&format!("<p><strong>链路名称:</strong> {}</p>", chain_name));
    }

    if let Some(expert_chain) = trace["expert_chain"].as_array() {
        if !expert_chain.is_empty() {
            html.push_str("<p><strong>专家执行链:</strong></p>");
            for (i, expert) in expert_chain.iter().enumerate() {
                let expert_name = expert.as_str().unwrap_or("");
                html.push_str(&format!(
                    "<div class='expert-chain'>步骤 {}: {}</div>",
                    i + 1,
                    expert_name
                ));
            }
        }
    }

    let status_text = trace["status"].as_str().unwrap_or("Unknown");
    let status_class = if status_text.contains("Success") {
        "success"
    } else {
        "error"
    };
    html.push_str(&format!(
        "<p><strong>状态:</strong> <span class='badge {}'>{}</span></p>",
        status_class, status_text
    ));

    if let Some(output) = trace["output"].as_str() {
        html.push_str("<p><strong>输出:</strong></p>");
        html.push_str(&format!("<pre>{}</pre>", output));
    }

    html.push_str("</div>");

    html.push_str("<p><a href='/subhuti/api/v1/traces?format=html'>← 返回 Trace 列表</a></p>");
    html.push_str("</div></body></html>");
    html
}

/// 格式化内存字节数为人类可读形式（如 "45.2 MB"）
fn format_mem_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// 生成函数调用链路 HTML 报告
fn generate_fn_call_report_html(
    trace_id: &str,
    trace: Option<&serde_json::Value>,
    fn_tree: Option<&serde_json::Value>,
) -> String {
    let mut html = String::from(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>函数调用链路报告 - Subhuti</title>
<style>
  :root {
    --bg: #0d1117; --card: #161b22; --border: #30363d;
    --text: #e6edf3; --text-secondary: #8b949e;
    --success: #3fb950; --fail: #f85149; --warn: #d29922;
    --info: #58a6ff; --accent: #58a6ff;
    --fn-call: #58a6ff; --fn-entry: #2ea043;
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    background: var(--bg); color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans SC', sans-serif;
    font-size: 14px; line-height: 1.6; padding: 20px;
  }
  .container { max-width: 1200px; margin: 0 auto; }
  .header {
    background: var(--card); border: 1px solid var(--border);
    border-radius: 12px; padding: 24px; margin-bottom: 20px;
  }
  .header-top { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 16px; }
  .header-title { font-size: 20px; font-weight: 600; }
  .header-title small { font-size: 13px; color: var(--text-secondary); font-weight: 400; margin-left: 8px; }
  .badge { display: inline-block; padding: 4px 12px; border-radius: 20px; font-size: 12px; font-weight: 600; }
  .badge-success { background: rgba(63,185,80,0.15); color: var(--success); }
  .badge-fail { background: rgba(248,81,73,0.15); color: var(--fail); }
  .header-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; }
  .header-item { padding: 8px 12px; background: rgba(255,255,255,0.03); border-radius: 8px; }
  .header-item label { font-size: 11px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px; }
  .header-item .value { font-size: 14px; font-family: 'SF Mono', 'Fira Code', monospace; margin-top: 2px; word-break: break-all; }
  .fn-tree { padding: 12px 0; }
  .fn-node { position: relative; margin: 2px 0; }
  .fn-children { position: relative; margin-left: 28px; border-left: 2px solid rgba(88,166,255,0.2); }
  .fn-card {
    background: rgba(255,255,255,0.02); border: 1px solid rgba(255,255,255,0.06);
    border-radius: 8px; margin: 4px 0; cursor: pointer; transition: all 0.15s;
  }
  .fn-card:hover { background: rgba(255,255,255,0.04); border-color: rgba(88,166,255,0.3); }
  .fn-card-header {
    display: flex; align-items: center; gap: 10px; padding: 10px 14px;
  }
  .fn-toggle { width: 18px; height: 18px; display: flex; align-items: center; justify-content: center; font-size: 10px; color: var(--text-secondary); flex-shrink: 0; border-radius: 3px; background: rgba(255,255,255,0.04); }
  .fn-icon { width: 22px; height: 22px; border-radius: 4px; display: flex; align-items: center; justify-content: center; font-size: 11px; flex-shrink: 0; }
  .fn-icon.fn-entry { background: rgba(46,160,67,0.2); color: var(--fn-entry); }
  .fn-icon.fn-call { background: rgba(88,166,255,0.2); color: var(--fn-call); }
  .fn-name { font-family: 'SF Mono', 'Fira Code', monospace; font-size: 13px; font-weight: 500; flex: 1; min-width: 0; }
  .fn-meta { display: flex; align-items: center; gap: 12px; flex-shrink: 0; }
  .fn-meta .fn-duration { font-family: 'SF Mono', monospace; font-size: 12px; color: var(--text-secondary); white-space: nowrap; }
  .fn-meta .fn-duration .ms { color: var(--info); }
  .fn-meta .fn-status { font-size: 14px; }
  .fn-meta .fn-bar { width: 60px; height: 4px; background: rgba(255,255,255,0.06); border-radius: 2px; overflow: hidden; }
  .fn-meta .fn-bar .fill { height: 100%; border-radius: 2px; }
  .fn-meta .fn-bar .fill.success { background: var(--success); }
  .fn-meta .fn-bar .fill.fail { background: var(--fail); }
  .fn-detail { display: none; border-top: 1px solid var(--border); padding: 14px; background: rgba(0,0,0,0.15); border-radius: 0 0 8px 8px; }
  .fn-detail.open { display: block; }
  .fn-detail .info-grid { display: grid; grid-template-columns: 1fr; gap: 10px; margin-bottom: 12px; }
  .fn-detail .info-grid .item { padding: 8px 10px; background: rgba(0,0,0,0.2); border-radius: 6px; }
  .fn-detail .info-grid .item label { font-size: 10px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px; }
  .fn-detail .info-grid .item .val { font-size: 12px; font-family: 'SF Mono', monospace; margin-top: 2px; word-break: break-all; max-height: 120px; overflow-y: auto; }
  .fn-detail .info-grid .item .bytes { font-size: 11px; color: var(--text-secondary); margin-top: 2px; }
  .fn-detail .info-grid .item .mem-diff { font-size: 11px; color: var(--accent); font-style: italic; }
  .fn-logs { background: rgba(0,0,0,0.15); border-radius: 6px; padding: 8px 10px; margin-top: 10px !important; }
  .fn-logs-header { font-size: 10px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 6px; }
  .fn-log-entry { display: flex; gap: 8px; padding: 3px 0; font-size: 12px; font-family: 'SF Mono', monospace; }
  .fn-log-level { font-weight: 700; min-width: 42px; }
  .fn-log-trace .fn-log-level { color: #888; } .fn-log-debug .fn-log-level { color: #5b9bd5; }
  .fn-log-info .fn-log-level { color: #70ad47; } .fn-log-warn .fn-log-level { color: #ffc107; }
  .fn-log-error .fn-log-level { color: #f44747; }
  .section { background: var(--card); border: 1px solid var(--border); border-radius: 12px; margin-bottom: 20px; overflow: hidden; }
  .section-header { padding: 16px 20px; border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center; cursor: pointer; user-select: none; }
  .section-header:hover { background: rgba(255,255,255,0.02); }
  .section-header h3 { font-size: 15px; font-weight: 600; }
  .section-body { padding: 12px 20px; }
  .no-data { color: var(--text-secondary); text-align: center; padding: 40px; font-size: 14px; }
  @media (max-width: 768px) { .header-grid { grid-template-columns: 1fr 1fr; } }
</style>
<script>
function toggleFn(el) {
  const detail = el.nextElementSibling;
  const toggle = el.querySelector('.fn-toggle');
  if (detail && detail.classList.contains('fn-detail')) {
    detail.classList.toggle('open');
    toggle.textContent = detail.classList.contains('open') ? '▼' : '▶';
  }
}
</script>
</head>
<body>
<div class="container">
<div class="header">
  <div class="header-top">
    <div><div class="header-title">函数调用链路报告 <small>Function Call Trace · 执行时间 · 传入传出数据</small></div></div>
  </div>
  <div class="header-grid">
    <div class="header-item"><label>Trace ID</label><div class="value">"#,
    );

    // Trace info
    html.push_str(trace_id);
    if let Some(t) = trace {
        let user_id = t["user_id"].as_str().unwrap_or("");
        let session_id = t["session_id"].as_str().unwrap_or("");
        let input = t["input"].as_str().unwrap_or("");
        let duration = t["total_duration_ms"]
            .as_u64()
            .map(|d| format!("{} ms", d))
            .unwrap_or_else(|| "-".to_string());
        let status = t["status"].as_str().unwrap_or("Unknown");
        let status_class = if status.contains("Success") {
            "badge-success"
        } else {
            "badge-fail"
        };

        html.push_str(&format!("</div></div>
    <div class=\"header-item\"><label>Session ID</label><div class=\"value\">{}</div></div>
    <div class=\"header-item\"><label>User ID</label><div class=\"value\">{}</div></div>
    <div class=\"header-item\"><label>Total Duration</label><div class=\"value\">{}</div></div>
    <div class=\"header-item\"><label>Input</label><div class=\"value\">{}</div></div>
    <div class=\"header-item\"><label>Status</label><div class=\"value\"><span class=\"badge {}\">{}</span></div></div>", session_id, user_id, duration, input, status_class, status));
    } else {
        html.push_str("</div></div>");
    }

    html.push_str("</div></div>");

    // Fn call tree section
    html.push_str("<div class=\"section\"><div class=\"section-header\"><h3>函数调用栈</h3></div><div class=\"section-body\"><div class=\"fn-tree\">");

    if let Some(tree) = fn_tree {
        if let Some(fn_calls) = tree["fn_calls"].as_array() {
            for fn_call in fn_calls {
                render_fn_call_node(&mut html, fn_call, 0, true);
            }
        }
    } else {
        html.push_str("<div class=\"no-data\">暂无函数调用链路数据。<br>请先通过 /subhuti/api/v1/orchestrate 发起一次请求，函数调用链路会在执行过程中自动记录。</div>");
    }

    html.push_str("</div></div></div>");
    html.push_str("</div></body></html>");
    html
}

/// 从函数节点 JSON 中提取 logs 数组并生成 HTML
fn build_logs_html(node: &serde_json::Value) -> String {
    let logs = match node.get("logs").and_then(|l| l.as_array()) {
        Some(logs) if !logs.is_empty() => logs,
        _ => return String::new(),
    };

    let mut html = String::from("<div class=\"fn-logs\" style=\"margin-top: 10px;\"><div class=\"fn-logs-header\">函数执行日志</div>");
    for log in logs {
        let level = log["level"].as_str().unwrap_or("INFO");
        let message = log["message"].as_str().unwrap_or("");
        let level_class = level.to_lowercase();
        html.push_str(&format!(
            "<div class=\"fn-log-entry fn-log-{}\"><span class=\"fn-log-level\">{}</span><span class=\"fn-log-msg\">{}</span></div>",
            level_class, level, message,
        ));
    }
    html.push_str("</div>");
    html
}

/// 递归渲染函数调用节点
fn render_fn_call_node(html: &mut String, node: &serde_json::Value, depth: usize, is_root: bool) {
    let fn_name = node["fn_name"].as_str().unwrap_or("unknown");
    let duration_ms = node["duration_ms"].as_u64().unwrap_or(0);
    let success = node["success"].as_bool().unwrap_or(true);
    let input = node["input"].as_str().unwrap_or("");
    let output = node["output"].as_str().unwrap_or("");
    let input_bytes = node["input_bytes"].as_u64().unwrap_or(0);
    let output_bytes = node["output_bytes"].as_u64().unwrap_or(0);
    let memory_entry = node["memory_entry"].as_u64();
    let memory_exit = node["memory_exit"].as_u64();
    let memory_diff = node["memory_diff"].as_u64().unwrap_or(0);
    let has_children = node
        .get("children")
        .and_then(|c| c.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);

    // 提取日志
    let logs_html = build_logs_html(node);

    // 格式化内存显示
    let mem_entry_str = memory_entry
        .map(|b| format_mem_bytes(b))
        .unwrap_or_else(|| "-".into());
    let mem_exit_str = memory_exit
        .map(|b| format_mem_bytes(b))
        .unwrap_or_else(|| "-".into());
    let mem_diff_str = if memory_diff > 0 {
        format!(" (+{} bytes)", memory_diff)
    } else {
        String::new()
    };

    let icon_class = if is_root { "fn-entry" } else { "fn-call" };
    let icon = if is_root { "E" } else { "S" };
    let status_icon = if success { "✅" } else { "❌" };
    let bar_class = if success { "success" } else { "fail" };
    let bar_width = if duration_ms > 0 {
        if duration_ms > 10000 {
            "100"
        } else {
            &format!(
                "{}",
                (duration_ms as f64 / 10000.0 * 100.0).min(100.0) as u64
            )
        }
    } else {
        "0"
    };
    let indent = if depth > 0 {
        " style=\"margin-left:28px\""
    } else {
        ""
    };

    html.push_str(&format!(
        "<div class=\"fn-node\"{}>{}
        <div class=\"fn-card\" onclick=\"toggleFn(this)\">
          <div class=\"fn-card-header\">
            <span class=\"fn-toggle\">▶</span>
            <span class=\"fn-icon {}\">{}</span>
            <span class=\"fn-name\">{}</span>
            <span class=\"fn-meta\">
              <span class=\"fn-duration\"><span class=\"ms\">{}</span>ms</span>
              <span class=\"fn-status\">{}</span>
              <span class=\"fn-bar\"><div class=\"fill {}\" style=\"width:{}%\"></div></span>
            </span>
          </div>
        </div>
        <div class=\"fn-detail\">
          <div class=\"info-grid\">
            <div class=\"item\"><label>Input</label><div class=\"val\">{}</div><div class=\"bytes\">{} bytes</div></div>
            <div class=\"item\"><label>Output</label><div class=\"val\">{}</div><div class=\"bytes\">{} bytes</div></div>
            <div class=\"item\"><label>Memory</label><div class=\"val\">{} → {} <span class=\"mem-diff\">{}</span></div></div>
          </div>
          {}
        </div>",
        indent,
        if has_children { "" } else { "" },
        icon_class, icon,
        fn_name,
        duration_ms,
        status_icon,
        bar_class, bar_width,
        if input.is_empty() { "-" } else { input },
        input_bytes,
        if output.is_empty() { "-" } else { output },
        output_bytes,
        mem_entry_str,
        mem_exit_str,
        mem_diff_str,
        logs_html,
    ));

    if has_children {
        html.push_str("<div class=\"fn-children\">");
        if let Some(children) = node["children"].as_array() {
            for child in children {
                render_fn_call_node(html, child, depth + 1, false);
            }
        }
        html.push_str("</div>");
    }

    html.push_str("</div>");
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/traces",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/traces", get(traces_list_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/traces/:id",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/traces/:id", get(traces_get_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/traces/:id/tree",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/traces/:id/tree", get(traces_tree_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/traces/:id/fn_call_tree",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/traces/:id/fn_call_tree", get(fn_call_tree_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/traces/:id/fn_call_report",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/traces/:id/fn_call_report", get(fn_call_report_handler)),
    }
}
