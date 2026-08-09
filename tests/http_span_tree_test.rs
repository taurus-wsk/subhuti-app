//! # 真实 HTTP 请求测试：验证 /traces/:id/span_tree 返回完整调用链路树
//!
//! 测试流程（端到端验证 Event-Bus Bridge Trace 方案）：
//! 1. 启动真实 axum HTTP server（MockLLM 模式，不打真实 API）
//! 2. POST /subhuti/api/v1/orchestrate 发起编排请求
//! 3. GET /subhuti/api/v1/traces 列表，通过 session_id 找到 trace_id
//! 4. GET /subhuti/api/v1/traces/:id/tree 验证 span 树包含完整调用链路
//!
//! 验证点：
//! - TraceAppService 装饰器生成了 trace_id 并注入 request
//! - 出站适配器把 trace_id 写入 ctx.metadata
//! - 框架 orchestrator/graph emit 事件时携带 trace_id
//! - TraceEventBridge 桥接框架事件 → TraceObserverPort.spans
//! - get_span_tree 组装出可查询的嵌套树

use std::time::Duration;

use serde_json::Value;

use subhuti_app::adapter::inbound::http::adapters::HttpAdapterFactory;
use subhuti_app::adapter::inbound::http::route_adapter::build_router;
use subhuti_app::application::CompositionRoot;
use subhuti_app::infra::config::default_config;

/// 启动真实 axum HTTP server（绑定 ephemeral 端口），返回 base_url
async fn start_test_server() -> String {
    let mut config = default_config();
    // 强制 MockLLM 模式：不打真实 LLM API，走框架 mock 响应
    config.test_mode.enabled = true;
    config.test_mode.mock_delay_ms = 0;
    // 绑定随机端口，避免端口冲突
    config.http.addr = "127.0.0.1:0".to_string();

    let composition = CompositionRoot::build(&config)
        .await
        .expect("CompositionRoot build 失败");

    let factory = HttpAdapterFactory::new(
        composition.chat_port,
        composition.expert_port,
        composition.skill_port,
        composition.trace_observer.clone(),
        composition.session_observer.clone(),
    );
    let app_state = factory.create_app_state();

    // 用 inventory 自动注册的 build_router 构建完整 axum app
    let app = build_router().with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener 失败");
    let addr = listener.local_addr().expect("获取 local_addr 失败");

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{}", addr)
}

/// 等待 server 就绪（重试 health 检查）
async fn wait_server_ready(base_url: &str) {
    let client = reqwest::Client::new();
    let health_url = format!("{}/subhuti/api/v1/health", base_url);
    for _ in 0..20 {
        if client
            .get(&health_url)
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // health 路由可能不存在，直接返回（server 已 spawn，listener 已 bind 即可接受请求）
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_span_tree_returns_full_trace() {
    let base_url = start_test_server().await;
    wait_server_ready(&base_url).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("构建 reqwest client 失败");

    // 用唯一 session_id 便于后续从 /traces 列表中定位本次请求的 trace
    let session_id = format!(
        "span-tree-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );

    println!("═══════════════════════════════════════════════════════════");
    println!("📋 测试 session_id: {}", session_id);
    println!("🌐 测试 server: {}", base_url);
    println!("═══════════════════════════════════════════════════════════");

    // ── 步骤 1：POST /orchestrate 发起编排请求 ──
    println!("\n📤 [步骤1] POST /subhuti/api/v1/orchestrate");
    let orchestrate_url = format!("{}/subhuti/api/v1/orchestrate", base_url);
    let resp = client
        .post(&orchestrate_url)
        .json(&serde_json::json!({
            "message": "帮我用 Blender 做一个弹跳球动画",
            "user_id": "span-tree-test-user",
            "session_id": session_id,
        }))
        .send()
        .await
        .expect("orchestrate 请求发送失败");

    let status = resp.status();
    let body: Value = resp.json().await.expect("解析 orchestrate 响应失败");
    println!("  HTTP 状态: {}", status);
    println!("  响应体: {}", serde_json::to_string_pretty(&body).unwrap());

    assert!(
        body["success"].as_bool().unwrap_or(false),
        "orchestrate 应返回 success=true，实际: {}",
        serde_json::to_string(&body).unwrap()
    );

    // ── 步骤 2：等待 TraceEventBridge 异步处理框架事件 ──
    // EventBus broadcast 是异步的，handler 在独立 task 执行，
    // orchestrate 返回后事件可能还在处理中，需要等待一下让 span 写入完成
    println!("\n⏳ [步骤2] 等待 TraceEventBridge 处理框架事件（500ms）...");
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ── 步骤 3：GET /traces 列表，通过 session_id 找到 trace_id ──
    println!("\n📋 [步骤3] GET /subhuti/api/v1/traces 查找 trace_id");
    let traces_url = format!("{}/subhuti/api/v1/traces", base_url);
    let traces_resp: Value = client
        .get(&traces_url)
        .send()
        .await
        .expect("traces 列表请求失败")
        .json()
        .await
        .expect("解析 traces 列表失败");

    println!(
        "  traces 列表: {}",
        serde_json::to_string_pretty(&traces_resp).unwrap()
    );

    let traces = traces_resp["data"]
        .as_array()
        .expect("traces 响应 data 应为数组");

    let trace_id = traces
        .iter()
        .find(|t| t["session_id"].as_str() == Some(session_id.as_str()))
        .map(|t| t["id"].as_str().expect("trace 应有 id 字段").to_string())
        .unwrap_or_else(|| {
            panic!(
                "未找到 session_id={} 的 trace，现有 traces: {:?}",
                session_id,
                traces
                    .iter()
                    .map(|t| t["session_id"].as_str())
                    .collect::<Vec<_>>()
            )
        });

    println!("  ✅ 找到 trace_id: {}", trace_id);

    // ── 步骤 4：GET /traces/:id/tree 验证 span 树 ──
    println!("\n🌳 [步骤4] GET /subhuti/api/v1/traces/{}/tree", trace_id);
    let tree_url = format!("{}/subhuti/api/v1/traces/{}/tree", base_url, trace_id);
    let tree_resp: Value = client
        .get(&tree_url)
        .send()
        .await
        .expect("span tree 请求失败")
        .json()
        .await
        .expect("解析 span tree 失败");

    println!(
        "  span tree: {}",
        serde_json::to_string_pretty(&tree_resp).unwrap()
    );

    assert!(
        tree_resp["success"].as_bool().unwrap_or(false),
        "span tree 应返回 success=true，实际: {}",
        serde_json::to_string(&tree_resp).unwrap()
    );

    let tree = &tree_resp["data"];
    assert!(
        tree["span_type"].is_string(),
        "root span 应有 span_type 字段"
    );

    println!("\n📊 span 树统计:");
    println!(
        "  root span_type: {}",
        tree["span_type"].as_str().unwrap_or("?")
    );
    println!("  root name: {}", tree["name"].as_str().unwrap_or("?"));

    // ── 验证点 1：root span 应是 user_message（编排入口事件） ──
    let root_span_type = tree["span_type"].as_str().unwrap_or("");
    assert_eq!(
        root_span_type, "user_message",
        "root span 应是 user_message（编排入口），实际: {}",
        root_span_type
    );
    assert_eq!(
        tree["input"].as_str(),
        Some("帮我用 Blender 做一个弹跳球动画"),
        "user_message span 的 input 应是用户消息原文"
    );

    // ── 验证点 2：root 下应有 chain_selected（编排策略选择） ──
    let root_children = tree["children"].as_array().expect("root children 应为数组");
    assert!(!root_children.is_empty(), "root 下应有子事件，实际为空");

    let chain_selected = root_children
        .iter()
        .find(|c| c["span_type"].as_str() == Some("chain_selected"))
        .expect("root 下应包含 chain_selected span");

    // ── 验证点 3：chain_selected 下应嵌套 graph_started（不是平铺在 root 下） ──
    let chain_children = chain_selected["children"]
        .as_array()
        .expect("chain_selected children 应为数组");

    let graph_started = chain_children
        .iter()
        .find(|c| c["span_type"].as_str() == Some("graph_started"))
        .expect("chain_selected 下应嵌套 graph_started span（验证嵌套结构）");

    // ── 验证点 4：graph_started 下应嵌套 node_execute_requested / node_completed / graph_completed ──
    let graph_children = graph_started["children"]
        .as_array()
        .expect("graph_started children 应为数组");

    let graph_child_types: Vec<&str> = graph_children
        .iter()
        .map(|c| c["span_type"].as_str().unwrap_or(""))
        .collect();
    println!("  graph_started 下嵌套的 span: {:?}", graph_child_types);

    assert!(
        graph_child_types
            .iter()
            .any(|&s| s == "node_execute_requested"),
        "graph_started 下应嵌套 node_execute_requested，实际: {:?}",
        graph_child_types
    );
    assert!(
        graph_child_types.iter().any(|&s| s == "node_completed"),
        "graph_started 下应嵌套 node_completed，实际: {:?}",
        graph_child_types
    );
    assert!(
        graph_child_types.iter().any(|&s| s == "graph_completed"),
        "graph_started 下应嵌套 graph_completed，实际: {:?}",
        graph_child_types
    );

    // ── 验证点 5：node_completed 应有 output 和 duration_ms ──
    let node_completed = graph_children
        .iter()
        .find(|c| c["span_type"].as_str() == Some("node_completed"))
        .expect("应有 node_completed span");
    assert!(
        node_completed["output"].as_str().is_some(),
        "node_completed 应有 output 字段"
    );
    assert!(
        node_completed["duration_ms"].as_u64().is_some(),
        "node_completed 应有 duration_ms 字段"
    );

    // ── 验证点 6：无重复事件（graph_started / graph_completed 各只出现一次） ──
    fn count_span_type(node: &serde_json::Value, target: &str) -> usize {
        let mut count = 0;
        if node["span_type"].as_str() == Some(target) {
            count += 1;
        }
        if let Some(children) = node["children"].as_array() {
            for child in children {
                count += count_span_type(child, target);
            }
        }
        count
    }
    assert_eq!(
        count_span_type(tree, "graph_started"),
        1,
        "graph_started 应只出现一次（无重复）"
    );
    assert_eq!(
        count_span_type(tree, "graph_completed"),
        1,
        "graph_completed 应只出现一次（无重复）"
    );

    // ── 验证点 7：session_id 应非空（SubhutiOrchestrationEngine 注入了 ctx.metadata） ──
    let root_extra = &tree["extra"];
    if root_extra.is_object() {
        let sid = root_extra["session_id"].as_str().unwrap_or("");
        assert!(
            !sid.is_empty(),
            "root span 的 extra.session_id 应非空（TraceAppService → ctx.metadata 传递链），实际: {:?}",
            sid
        );
    }

    println!("\n═══════════════════════════════════════════════════════════");
    println!("✅ 测试通过：/traces/:id/tree 返回了完整的嵌套调用链路树");
    println!("   - trace_id: {}", trace_id);
    println!(
        "   - 嵌套结构: user_message → chain_selected → graph_started → [node_*, graph_completed]"
    );
    println!("═══════════════════════════════════════════════════════════");
}
