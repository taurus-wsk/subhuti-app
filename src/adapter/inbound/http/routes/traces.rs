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
