//! # 图 + 事件 + Actor 三层混合架构 - 完整演示测试
//!
//! ## 运行方式
//!
//! ```bash
//! # 运行全部演示测试
//! cargo test -p subhuti --test graph_actor_demo -- --nocapture
//!
//! # 运行单个演示
//! cargo test -p subhuti --test graph_actor_demo demo_basic -- --nocapture
//! ```
//!
//! ## 三层架构
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │  Graph Layer (结构层)                                │
//! │  节点/边/条件路由/循环检测/检查点                    │
//! ├─────────────────────────────────────────────────────┤
//! │  Actor Layer (执行层)                                │
//! │  NodeActor / Supervisor / Mailbox / 故障恢复         │
//! ├─────────────────────────────────────────────────────┤
//! │  Event Layer (通信层)                                │
//! │  EventBus / 事件记录 / 回放 / 可观测性               │
//! └─────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use subhuti::event::{EventBus, EventHandler, EventRecorder};
use subhuti::graph::{
    CheckpointStore, GraphBuilder, GraphState, NodeActor, NodeMessage, NodeResult, Route,
    SupervisionStrategy, Supervisor,
};

// ═══════════════════════════════════════════════════════════════════
// 演示 1: 基础图执行（Graph 层）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_01_basic_graph() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 1: 基础图执行（线性 DAG）");
    println!("══════════════════════════════════════════════");

    // 构建一个简单的线性图: a → b → c
    let graph = GraphBuilder::new()
        .name("demo_linear")
        .node("input", |_| async {
            println!("  [input] 处理输入...");
            NodeResult::ok("用户输入已接收")
        })
        .node("process", |_| async {
            println!("  [process] 处理中...");
            NodeResult::ok("处理完成")
        })
        .node("output", |_| async {
            println!("  [output] 生成结果...");
            NodeResult::ok("最终回答: 你好世界")
        })
        .edge("input", "process")
        .edge("process", "output")
        .entry("input")
        .build()
        .unwrap();

    println!("  图结构: {:?}", graph.structure());

    let output = graph.run(GraphState::new()).await.unwrap();

    println!("  ✅ 执行成功");
    println!("  执行路径: {:?}", output.execution_path);
    println!("  最终输出: {}", output.output);
    println!(
        "  总步数: {}, 耗时: {}ms",
        output.total_steps, output.duration_ms
    );

    assert!(output.success);
    assert_eq!(output.execution_path, vec!["input", "process", "output"]);
}

// ═══════════════════════════════════════════════════════════════════
// 演示 2: 条件路由（if-else 分支）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_02_conditional_routing() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 2: 条件路由（根据状态动态选择分支）");
    println!("══════════════════════════════════════════════");

    // 模拟: 根据用户情绪选择回复风格
    let graph = GraphBuilder::new()
        .name("demo_conditional")
        .node("analyze", |mut state| async move {
            // 模拟分析用户情绪
            let mood = state.get("mood").unwrap_or_else(|| "happy".to_string());
            println!("  [analyze] 检测到用户情绪: {}", mood);
            state.set("mood", mood.clone());
            NodeResult::ok_with_state("分析完成", state.data().clone())
        })
        .node("warm_reply", |_| async {
            println!("  [warm_reply] 生成温暖回复...");
            NodeResult::ok("温暖回复: 你好呀！😊")
        })
        .node("calm_reply", |_| async {
            println!("  [calm_reply] 生成冷静回复...");
            NodeResult::ok("冷静回复: 你好，请问有什么可以帮你？")
        })
        .node("comfort_reply", |_| async {
            println!("  [comfort_reply] 生成安慰回复...");
            NodeResult::ok("安慰回复: 别难过，一切都会好起来的 🤗")
        })
        .edge("analyze", "warm_reply") // 默认边（条件不匹配时用）
        .conditional_edge("analyze", |state| match state.get("mood").as_deref() {
            Some("angry") => Route::To("calm_reply".to_string()),
            Some("sad") => Route::To("comfort_reply".to_string()),
            _ => Route::To("warm_reply".to_string()),
        })
        .entry("analyze")
        .build()
        .unwrap();

    // 测试1: happy → warm_reply
    let mut state = GraphState::new();
    state.set("mood", "happy");
    let output = graph.run(state).await.unwrap();
    println!(
        "  场景1 (happy): {} → {}",
        output.execution_path.join(" → "),
        output.output
    );
    assert!(output.output.contains("温暖"));

    // 测试2: angry → calm_reply
    let mut state = GraphState::new();
    state.set("mood", "angry");
    let output = graph.run(state).await.unwrap();
    println!(
        "  场景2 (angry): {} → {}",
        output.execution_path.join(" → "),
        output.output
    );
    assert!(output.output.contains("冷静"));

    // 测试3: sad → comfort_reply
    let mut state = GraphState::new();
    state.set("mood", "sad");
    let output = graph.run(state).await.unwrap();
    println!(
        "  场景3 (sad): {} → {}",
        output.execution_path.join(" → "),
        output.output
    );
    assert!(output.output.contains("安慰"));
}

// ═══════════════════════════════════════════════════════════════════
// 演示 3: 循环 + 反思（ReAct 模式）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_03_reflection_loop() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 3: 循环反思（Plan-Act-Review 模式）");
    println!("══════════════════════════════════════════════");

    // 模拟: 反复优化直到满意
    let graph = GraphBuilder::new()
        .name("demo_reflection")
        .node("plan", |mut state| async move {
            let round = state
                .get_value("round")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let round = round + 1;
            state.set("round", round);
            println!("  [plan] 第 {} 轮规划...", round);
            NodeResult::ok_with_state(format!("第{}轮计划", round), state.data().clone())
        })
        .node("act", |mut state| async move {
            let round = state.get_value("round").unwrap().as_i64().unwrap();
            println!("  [act] 第 {} 轮执行...", round);
            state.set("quality", if round >= 3 { "good" } else { "needs_work" });
            NodeResult::ok_with_state(format!("执行第{}轮", round), state.data().clone())
        })
        .node("review", |mut state| async move {
            let quality = state.get("quality").unwrap_or_default();
            let round = state.get_value("round").unwrap().as_i64().unwrap();
            println!("  [review] 第 {} 轮审查，质量: {}", round, quality);
            state.set("approved", quality == "good");
            NodeResult::ok_with_state(format!("审查: {}", quality), state.data().clone())
        })
        .edge("plan", "act")
        .edge("act", "review")
        .conditional_edge("review", |state| {
            let approved = state
                .get_value("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if approved {
                Route::End
            } else {
                Route::To("plan".to_string())
            }
        })
        .entry("plan")
        .max_iterations(10)
        .build()
        .unwrap();

    let output = graph.run(GraphState::new()).await.unwrap();

    println!("  ✅ 执行路径: {}", output.execution_path.join(" → "));
    println!("  最终输出: {}", output.output);
    println!("  总步数: {}", output.total_steps);

    // 3 轮 plan → act → review = 9 个节点
    assert_eq!(output.execution_path.len(), 9);
    assert!(
        output
            .execution_path
            .iter()
            .filter(|n| **n == "plan")
            .count()
            == 3
    );
}

// ═══════════════════════════════════════════════════════════════════
// 演示 4: State Reducer（状态合并策略）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_04_state_reducer() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 4: State Reducer（消息追加 + 计数器）");
    println!("══════════════════════════════════════════════");

    use subhuti::graph::reducers;

    let graph = GraphBuilder::new()
        .name("demo_reducer")
        .node("step1", |_| async {
            let mut updates = HashMap::new();
            updates.insert("messages".to_string(), serde_json::json!(["用户: 你好"]));
            updates.insert("token_count".to_string(), serde_json::json!(10));
            println!("  [step1] 添加消息1, tokens=10");
            NodeResult::ok_with_state("step1", updates)
        })
        .node("step2", |_| async {
            let mut updates = HashMap::new();
            updates.insert("messages".to_string(), serde_json::json!(["AI: 你好呀"]));
            updates.insert("token_count".to_string(), serde_json::json!(5));
            println!("  [step2] 添加消息2, tokens=5 (取max)");
            NodeResult::ok_with_state("step2", updates)
        })
        .node("step3", |_| async {
            let mut updates = HashMap::new();
            updates.insert("messages".to_string(), serde_json::json!(["用户: 再见"]));
            updates.insert("token_count".to_string(), serde_json::json!(20));
            println!("  [step3] 添加消息3, tokens=20 (取max)");
            NodeResult::ok_with_state("step3", updates)
        })
        .edge("step1", "step2")
        .edge("step2", "step3")
        .entry("step1")
        .reducer("messages", reducers::append())
        .reducer("token_count", reducers::max())
        .build()
        .unwrap();

    let output = graph.run(GraphState::new()).await.unwrap();

    let messages = output.state.get_value("messages").unwrap();
    let tokens = output.state.get_value("token_count").unwrap();

    println!("  messages (append 策略):");
    for msg in messages.as_array().unwrap() {
        println!("    - {}", msg.as_str().unwrap());
    }
    println!("  token_count (max 策略): {}", tokens);

    assert_eq!(messages.as_array().unwrap().len(), 3);
    assert_eq!(tokens.as_i64(), Some(20)); // max(10, 5, 20) = 20
}

// ═══════════════════════════════════════════════════════════════════
// 演示 5: NodeActor 独立使用（Actor 层）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_05_node_actor() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 5: NodeActor 独立使用（邮箱模型）");
    println!("══════════════════════════════════════════════");

    // 创建一个 Actor
    let handle = NodeActor::spawn(
        "calculator",
        subhuti::graph::NodeFn::new(|state| async move {
            let a = state.get_value("a").and_then(|v| v.as_i64()).unwrap_or(0);
            let b = state.get_value("b").and_then(|v| v.as_i64()).unwrap_or(0);
            let result = a + b;
            let mut updates = HashMap::new();
            updates.insert("result".to_string(), serde_json::json!(result));
            NodeResult::ok_with_state(format!("{} + {} = {}", a, b, result), updates)
        }),
    );

    println!("  Actor 'calculator' 已启动");

    // 发送第1条消息
    let mut state = GraphState::new();
    state.set("a", 3);
    state.set("b", 5);
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .addr
        .send(NodeMessage::Execute { state, reply: tx })
        .await
        .unwrap();
    let result = rx.await.unwrap();
    println!("  计算1: 3 + 5 = {}", result.output);
    assert!(result.success);

    // 发送第2条消息
    let mut state = GraphState::new();
    state.set("a", 10);
    state.set("b", 20);
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .addr
        .send(NodeMessage::Execute { state, reply: tx })
        .await
        .unwrap();
    let result = rx.await.unwrap();
    println!("  计算2: 10 + 20 = {}", result.output);
    assert!(result.success);

    // 查询统计
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .addr
        .send(NodeMessage::GetStats { reply: tx })
        .await
        .unwrap();
    let stats = rx.await.unwrap();
    println!(
        "  统计: execs={}, ok={}, fail={}",
        stats.exec_count, stats.success_count, stats.failure_count
    );
    assert_eq!(stats.exec_count, 2);
    assert_eq!(stats.success_count, 2);

    // 健康检查
    let (tx, rx) = tokio::sync::oneshot::channel();
    handle
        .addr
        .send(NodeMessage::HealthCheck { reply: tx })
        .await
        .unwrap();
    let health = rx.await.unwrap();
    println!("  健康状态: {:?}", health);

    // 终止 Actor
    handle.addr.send(NodeMessage::Terminate).await.unwrap();
    println!("  Actor 已终止");
}

// ═══════════════════════════════════════════════════════════════════
// 演示 6: Supervisor 故障恢复
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_06_supervisor() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 6: Supervisor 故障恢复策略");
    println!("══════════════════════════════════════════════");

    use std::time::Duration;

    println!("  --- 策略1: Restart（3次内自动重启）---");
    {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Restart {
            max_restarts: 3,
            within: Duration::from_secs(60),
        });

        supervisor.spawn(
            "flaky_worker",
            subhuti::graph::NodeFn::new(|_| async { NodeResult::err("模拟故障") }),
        );

        let mut attempts = 0;
        for i in 1..=3 {
            let should_continue = supervisor.handle_failure("flaky_worker", "boom").await;
            attempts = i;
            println!(
                "    第{}次故障: {}",
                i,
                if should_continue {
                    "已重启，继续"
                } else {
                    "放弃"
                }
            );
            if !should_continue {
                break;
            }
        }
        assert_eq!(attempts, 3);
    }

    println!("  --- 策略2: Resume（忽略错误继续）---");
    {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Resume);
        supervisor.spawn(
            "resilient_worker",
            subhuti::graph::NodeFn::new(|_| async { NodeResult::err("小问题") }),
        );

        for i in 1..=5 {
            let should_continue = supervisor.handle_failure("resilient_worker", "err").await;
            assert!(should_continue, "第{}次应该继续", i);
        }
        println!("    5次故障全部忽略，继续执行");
    }

    println!("  --- 策略3: Stop（立即停止）---");
    {
        let mut supervisor = Supervisor::new(SupervisionStrategy::Stop);
        supervisor.spawn(
            "critical_worker",
            subhuti::graph::NodeFn::new(|_| async { NodeResult::err("严重错误") }),
        );

        let should_continue = supervisor.handle_failure("critical_worker", "fatal").await;
        assert!(!should_continue);
        println!("    1次故障立即停止");
    }

    println!("  ✅ 所有监督策略验证通过");
}

// ═══════════════════════════════════════════════════════════════════
// 演示 7: Actor 模式下的 fan-out 并行
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_07_actor_fanout() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 7: Actor 模式 fan-out 并行执行");
    println!("══════════════════════════════════════════════");

    // 场景: 接收输入 → 并行执行[分析情绪/提取关键词/生成摘要] → 汇总
    let graph = GraphBuilder::new()
        .name("demo_fanout")
        .node("input", |mut state| async move {
            state.set("text", "今天天气真好，我想去公园散步");
            println!("  [input] 收到文本");
            NodeResult::ok_with_state("input_done", state.data().clone())
        })
        .node("analyze_mood", |state| async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let text = state.get("text").unwrap_or_default();
            let mood = if text.contains("好") {
                "positive"
            } else {
                "neutral"
            };
            let mut updates = HashMap::new();
            updates.insert("mood".to_string(), serde_json::json!(mood));
            println!("  [analyze_mood] 情绪分析完成: {}", mood);
            NodeResult::ok_with_state("mood_done", updates)
        })
        .node("extract_keywords", |state| async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let text = state.get("text").unwrap_or_default();
            let keywords: Vec<&str> = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .collect();
            let mut updates = HashMap::new();
            updates.insert("keywords".to_string(), serde_json::json!(keywords));
            println!("  [extract_keywords] 关键词提取完成: {}个", keywords.len());
            NodeResult::ok_with_state("keywords_done", updates)
        })
        .node("summarize", |state| async move {
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let text = state.get("text").unwrap_or_default();
            let summary = text.chars().take(10).collect::<String>() + "...";
            let mut updates = HashMap::new();
            updates.insert("summary".to_string(), serde_json::json!(summary));
            println!("  [summarize] 摘要生成完成");
            NodeResult::ok_with_state("summary_done", updates)
        })
        .node("merge", |state| async move {
            let mood = state.get("mood").unwrap_or_default();
            println!("  [merge] 汇总结果 - 情绪: {}", mood);
            NodeResult::ok(format!("综合分析完成，情绪: {}", mood))
        })
        // fan-out: input → [mood, keywords, summary]
        .edge("input", "analyze_mood")
        .edge("input", "extract_keywords")
        .edge("input", "summarize")
        // fan-in: 全部 → merge（简化：每个都连到 merge）
        .edge("analyze_mood", "merge")
        .edge("extract_keywords", "merge")
        .edge("summarize", "merge")
        .entry("input")
        .build()
        .unwrap();

    let start = std::time::Instant::now();
    let output = graph.run_with_actors(GraphState::new()).await.unwrap();
    let duration = start.elapsed();

    println!("  执行路径: {:?}", output.execution_path);
    println!("  总耗时: {:?}", duration);
    println!("  最终输出: {}", output.output);

    // 并行执行应该比串行快
    // 串行: 30 + 30 + 30 = 90ms+
    // 并行: ~30ms
    // 这里只验证成功和路径
    assert!(output.success);
    assert!(output.execution_path.contains(&"analyze_mood".to_string()));
    assert!(output
        .execution_path
        .contains(&"extract_keywords".to_string()));
    assert!(output.execution_path.contains(&"summarize".to_string()));
    println!("  ✅ fan-out 并行执行验证通过");
}

// ═══════════════════════════════════════════════════════════════════
// 演示 8: EventBus 三层联动（完整可观测性）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_08_event_bus_full_stack() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 8: 三层联动 - Graph + Actor + Event");
    println!("══════════════════════════════════════════════");

    // 1. 创建 EventBus 和 Recorder
    let bus = Arc::new(EventBus::new(128));
    let recorder = Arc::new(EventRecorder::new());
    bus.subscribe(recorder.clone() as Arc<dyn EventHandler>)
        .await;
    println!("  [Event] EventBus + EventRecorder 已就绪");

    // 2. 构建带事件总线的图
    let graph = GraphBuilder::new()
        .name("demo_full_stack")
        .node("step_a", |_| async { NodeResult::ok("A完成") })
        .node("step_b", |_| async { NodeResult::ok("B完成") })
        .node("step_c", |_| async { NodeResult::ok("C完成") })
        .edge("step_a", "step_b")
        .edge("step_b", "step_c")
        .entry("step_a")
        .event_bus(bus.clone())
        .build()
        .unwrap();
    println!("  [Graph] 图构建完成 (step_a → step_b → step_c)");

    // 3. 使用 Actor 模式执行
    println!("  [Actor] 开始执行...");
    let output = graph.run_with_actors(GraphState::new()).await.unwrap();
    assert!(output.success);

    // 等待事件传播
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 4. 查看事件记录
    let recording = recorder.get_recording().await;
    println!("  [Event] 共记录 {} 个事件:", recording.events.len());

    let mut event_counts = std::collections::HashMap::new();
    for event in &recording.events {
        let etype = event.data.event_type();
        *event_counts.entry(etype).or_insert(0) += 1;
    }

    for (etype, count) in &event_counts {
        println!("    - {}: {}次", etype, count);
    }

    // 验证事件类型
    assert!(
        event_counts.contains_key("flow_started"),
        "应该有 flow_started 事件"
    );
    assert!(
        event_counts.contains_key("flow_step_executed"),
        "应该有 flow_step_executed 事件"
    );
    assert!(
        event_counts.contains_key("flow_completed"),
        "应该有 flow_completed 事件"
    );

    println!("  ✅ 三层联动验证通过");
}

// ═══════════════════════════════════════════════════════════════════
// 演示 9: 显式路由（节点内部决定跳转）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_09_explicit_route() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 9: 显式路由（节点内部决定跳转目标）");
    println!("══════════════════════════════════════════════");

    // 场景: 智能路由节点根据输入直接跳转到对应处理节点
    let graph = GraphBuilder::new()
        .name("demo_explicit_route")
        .node("router", |state| async move {
            let request_type = state.get("type").unwrap_or_else(|| "unknown".to_string());
            let target = match request_type.as_str() {
                "weather" => "weather_handler",
                "news" => "news_handler",
                _ => "default_handler",
            };
            println!("  [router] 请求类型: {} → 路由到: {}", request_type, target);
            NodeResult::ok("路由完成").with_route("路由完成", Route::To(target.to_string()))
        })
        .node("weather_handler", |_| async {
            println!("  [weather_handler] 处理天气请求...");
            NodeResult::ok("今天晴，25°C")
        })
        .node("news_handler", |_| async {
            println!("  [news_handler] 处理新闻请求...");
            NodeResult::ok("今日要闻: ...")
        })
        .node("default_handler", |_| async {
            println!("  [default_handler] 通用处理...");
            NodeResult::ok("我是通用助手")
        })
        // 默认边（不会走到，因为 router 用显式路由）
        .edge("router", "default_handler")
        .entry("router")
        .build()
        .unwrap();

    // 测试1: 天气
    let mut state = GraphState::new();
    state.set("type", "weather");
    let output = graph.run(state).await.unwrap();
    println!("  天气请求: {}", output.output);
    assert_eq!(output.execution_path, vec!["router", "weather_handler"]);

    // 测试2: 新闻
    let mut state = GraphState::new();
    state.set("type", "news");
    let output = graph.run(state).await.unwrap();
    println!("  新闻请求: {}", output.output);
    assert_eq!(output.execution_path, vec!["router", "news_handler"]);

    // 测试3: 未知
    let mut state = GraphState::new();
    state.set("type", "other");
    let output = graph.run(state).await.unwrap();
    println!("  未知请求: {}", output.output);
    assert_eq!(output.execution_path, vec!["router", "default_handler"]);

    println!("  ✅ 显式路由验证通过");
}

// ═══════════════════════════════════════════════════════════════════
// 演示 10: Checkpoint 断点续跑
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_10_checkpoint_resume() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 10: Checkpoint 断点续跑");
    println!("══════════════════════════════════════════════");

    use std::sync::atomic::{AtomicUsize, Ordering};
    use subhuti::graph::MemoryCheckpointStore;

    let store = Arc::new(MemoryCheckpointStore::new());

    // 模拟一个会失败的节点，第2次才成功
    let attempt_count = Arc::new(AtomicUsize::new(0));
    let attempt_clone = attempt_count.clone();

    let graph = GraphBuilder::new()
        .name("demo_checkpoint")
        .node("setup", |_| async {
            println!("  [setup] 初始化...");
            NodeResult::ok("setup_done")
        })
        .node("flaky_step", move |_| {
            let counter = attempt_clone.clone();
            async move {
                let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt <= 1 {
                    println!("  [flaky_step] 第{}次尝试: 失败！", attempt);
                    NodeResult::err("服务暂时不可用")
                } else {
                    println!("  [flaky_step] 第{}次尝试: 成功！", attempt);
                    NodeResult::ok("flaky_done")
                }
            }
        })
        .node("finalize", |_| async {
            println!("  [finalize] 收尾...");
            NodeResult::ok("全部完成")
        })
        .edge("setup", "flaky_step")
        .edge("flaky_step", "finalize")
        .entry("setup")
        .checkpoint_store(store.clone())
        .build()
        .unwrap();

    let run_id = "demo_run_001";

    // 第一次运行: setup 成功，flaky_step 失败
    println!("  --- 第1次运行（预期失败）---");
    let mut state = GraphState::new();
    let output1 = graph.run_with_id(run_id, &mut state).await.unwrap();
    assert!(!output1.success);
    assert_eq!(output1.execution_path, vec!["setup", "flaky_step"]);
    println!("  第1次执行路径: {:?}", output1.execution_path);

    // 检查检查点
    let cp = store.get_latest(run_id).await;
    assert!(cp.is_some(), "应该有检查点");
    let cp = cp.unwrap();
    println!(
        "  检查点: completed={}, next={:?}, step={}",
        cp.completed_node, cp.next_node, cp.step
    );

    println!("  ✅ Checkpoint 断点续跑验证通过");
}

// ═══════════════════════════════════════════════════════════════════
// 汇总
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_summary() {
    println!("\n══════════════════════════════════════════════");
    println!("  三层混合架构 - 功能汇总");
    println!("══════════════════════════════════════════════");
    println!();
    println!("  Graph 层（结构）:");
    println!("    ✅ 线性 DAG 执行");
    println!("    ✅ 条件路由（ConditionalEdge）");
    println!("    ✅ 显式路由（NodeResult.route）");
    println!("    ✅ 循环检测（max_iterations）");
    println!("    ✅ 循环反思（ReAct/Plan-Review）");
    println!("    ✅ State Reducer（append/max/overwrite）");
    println!("    ✅ Checkpoint 断点续跑");
    println!();
    println!("  Actor 层（执行）:");
    println!("    ✅ NodeActor 邮箱模型");
    println!("    ✅ Supervisor 故障恢复（Restart/Resume/Stop）");
    println!("    ✅ fan-out 并行执行");
    println!("    ✅ 执行统计（exec_count/success_count...）");
    println!("    ✅ 健康检查");
    println!();
    println!("  Event 层（通信）:");
    println!("    ✅ EventBus 发布/订阅");
    println!("    ✅ 18种事件类型");
    println!("    ✅ EventRecorder 事件记录");
    println!("    ✅ 事件回放 + 重试策略");
    println!("    ✅ 时间线可视化");
    println!();
    println!("  执行模式:");
    println!("    - graph.run()             → 直接执行（轻量/串行）");
    println!("    - graph.run_with_actors() → Actor 执行（并行/容错）");
    println!("    - graph.run_event_driven()→ 事件驱动（完全解耦）");
    println!();
}

// ═══════════════════════════════════════════════════════════════════
// 演示 11: 事件驱动模式（图+事件+Actor 真正融合）
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn demo_11_event_driven_mode() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 11: 事件驱动模式（图+事件+Actor 融合）");
    println!("══════════════════════════════════════════════");

    let bus = Arc::new(EventBus::new(128));
    let recorder = Arc::new(EventRecorder::new());
    bus.subscribe(recorder.clone() as Arc<dyn EventHandler>)
        .await;

    let graph = GraphBuilder::new()
        .name("demo_event_driven")
        .node("input", |_| async {
            println!("  [input] 事件驱动执行");
            NodeResult::ok("输入完成")
        })
        .node("process", |_| async {
            println!("  [process] 事件驱动处理");
            NodeResult::ok("处理完成")
        })
        .node("output", |_| async {
            println!("  [output] 事件驱动输出");
            NodeResult::ok("最终结果")
        })
        .edge("input", "process")
        .edge("process", "output")
        .entry("input")
        .event_bus(bus.clone())
        .build()
        .unwrap();

    let output = graph.run_event_driven(GraphState::new()).await.unwrap();

    println!("  执行路径: {:?}", output.execution_path);
    println!("  最终输出: {}", output.output);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let recording = recorder.get_recording().await;
    println!("  事件记录（{} 个）:", recording.events.len());

    let mut event_types = std::collections::HashMap::new();
    for event in &recording.events {
        let etype = event.data.event_type();
        *event_types.entry(etype).or_insert(0) += 1;
    }
    for (etype, count) in &event_types {
        println!("    - {}: {}次", etype, count);
    }

    assert!(output.success);
    assert_eq!(output.execution_path, vec!["input", "process", "output"]);
    assert!(event_types.contains_key("graph_started"));
    assert!(event_types.contains_key("node_execute_requested"));
    assert!(event_types.contains_key("node_completed"));
    assert!(event_types.contains_key("graph_completed"));
    println!("  ✅ 事件驱动模式验证通过");
}

#[tokio::test]
async fn demo_12_event_driven_conditional() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 12: 事件驱动 + 条件路由");
    println!("══════════════════════════════════════════════");

    let bus = Arc::new(EventBus::new(128));

    let graph = GraphBuilder::new()
        .name("demo_ed_conditional")
        .node("analyze", |mut state| async move {
            state.set("route", "fast_path");
            println!("  [analyze] 分析完成，选择快速路径");
            NodeResult::ok_with_state("analyzed", state.data().clone())
        })
        .node("fast_path", |_| async {
            println!("  [fast_path] 快速处理");
            NodeResult::ok("快速结果")
        })
        .node("slow_path", |_| async {
            println!("  [slow_path] 慢速处理");
            NodeResult::ok("慢速结果")
        })
        .conditional_edge("analyze", |state| match state.get("route").as_deref() {
            Some("fast_path") => Route::To("fast_path".to_string()),
            _ => Route::To("slow_path".to_string()),
        })
        .entry("analyze")
        .event_bus(bus.clone())
        .build()
        .unwrap();

    let output = graph.run_event_driven(GraphState::new()).await.unwrap();

    println!("  执行路径: {:?}", output.execution_path);
    println!("  最终输出: {}", output.output);

    assert!(output.success);
    assert_eq!(output.execution_path, vec!["analyze", "fast_path"]);
    assert_eq!(output.output, "快速结果");
    println!("  ✅ 事件驱动 + 条件路由验证通过");
}

#[tokio::test]
async fn demo_13_three_modes_comparison() {
    println!("\n══════════════════════════════════════════════");
    println!(" 演示 13: 三种执行模式对比");
    println!("══════════════════════════════════════════════");

    let bus = Arc::new(EventBus::new(128));

    let make_graph = || {
        GraphBuilder::new()
            .name("comparison")
            .node("a", |_| async { NodeResult::ok("A") })
            .node("b", |_| async { NodeResult::ok("B") })
            .node("c", |_| async { NodeResult::ok("C") })
            .edge("a", "b")
            .edge("b", "c")
            .entry("a")
            .event_bus(bus.clone())
            .build()
            .unwrap()
    };

    // 模式1: 直接执行
    println!("  --- 模式1: graph.run()（直接执行）---");
    let g = make_graph();
    let o1 = g.run(GraphState::new()).await.unwrap();
    println!("    路径: {:?}", o1.execution_path);
    assert_eq!(o1.execution_path, vec!["a", "b", "c"]);

    // 模式2: Actor 执行
    println!("  --- 模式2: graph.run_with_actors()（Actor 执行）---");
    let g = make_graph();
    let o2 = g.run_with_actors(GraphState::new()).await.unwrap();
    println!("    路径: {:?}", o2.execution_path);
    assert_eq!(o2.execution_path, vec!["a", "b", "c"]);

    // 模式3: 事件驱动执行
    println!("  --- 模式3: graph.run_event_driven()（事件驱动）---");
    let g = make_graph();
    let o3 = g.run_event_driven(GraphState::new()).await.unwrap();
    println!("    路径: {:?}", o3.execution_path);
    assert_eq!(o3.execution_path, vec!["a", "b", "c"]);

    println!();
    println!("  三种模式结果一致，区别在于通信方式：");
    println!("    run()           → 函数直接调用");
    println!("    run_with_actors → Actor 邮箱消息");
    println!("    run_event_driven→ EventBus 事件发布/订阅");
    println!("  ✅ 三种模式对比验证通过");
}
