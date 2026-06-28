//! LLM Mock 端到端测试
//!
//! 使用 MockLLM 模拟完整的 Agent 处理链路，无需真实 API 调用
//!
//! 运行: cargo test -p subhuti --test test_llm_mock -- --nocapture

use std::time::Instant;
use subhuti::runtime::llm::ToolCall;
use subhuti::{
    CalculatorSkill, DefaultChatSkill, Message, MockLLM, Subhuti, TestTracker, WeatherSkill, LLM,
};

fn main() {
    run_tests();
}

#[test]
fn test_mock_llm_e2e() {
    run_tests();
}

fn run_tests() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           LLM MOCK E2E TEST - Agent 链路验证                  ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let mut tracker = TestTracker::new();
    let total_start = Instant::now();

    // ── Test 1: MockLLM 基础功能 ──────────────────────────
    print_step(1, "MockLLM 基础功能测试");
    match test_mock_llm_basic() {
        Ok(msg) => {
            tracker.pass("MockLLM 基础");
            println!("  ✅ {} ({})", msg, format_elapsed(0));
        }
        Err(e) => tracker.fail("MockLLM 基础", &e),
    }

    // ── Test 2: MockLLM 预设响应队列 ──────────────────────
    print_step(2, "预设响应队列测试");
    match test_response_queue() {
        Ok(msg) => {
            tracker.pass("响应队列");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("响应队列", &e),
    }

    // ── Test 3: MockLLM 消息捕获 ──────────────────────────
    print_step(3, "消息捕获与 Prompt 验证测试");
    match test_message_capture() {
        Ok(msg) => {
            tracker.pass("消息捕获");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("消息捕获", &e),
    }

    // ── Test 4: MockLLM 工具调用响应 ──────────────────────
    print_step(4, "工具调用响应测试");
    match test_tool_call_response() {
        Ok(msg) => {
            tracker.pass("工具调用");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("工具调用", &e),
    }

    // ── Test 5: 完整 Agent 链路 - 简单聊天 ─────────────────
    print_step(5, "完整 Agent 链路 - 简单聊天");
    match test_full_agent_chat() {
        Ok(msg) => {
            tracker.pass("Agent 聊天");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("Agent 聊天", &e),
    }

    // ── Test 6: 完整 Agent 链路 - 工具调用流程 ─────────────
    print_step(6, "完整 Agent 链路 - 工具调用流程");
    match test_full_agent_tool_call() {
        Ok(msg) => {
            tracker.pass("Agent 工具调用");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("Agent 工具调用", &e),
    }

    // ── Test 7: 流式输出模拟 ──────────────────────────────
    print_step(7, "流式输出模拟测试");
    match test_streaming_mock() {
        Ok(msg) => {
            tracker.pass("流式输出");
            println!("  ✅ {}", msg);
        }
        Err(e) => tracker.fail("流式输出", &e),
    }

    // ── 测试总结 ──────────────────────────────────────────
    println!("\n══════════════════════════════════════════════════════════════");
    println!("{}", tracker.summary());
    println!(
        "总耗时: {:.3}ms",
        total_start.elapsed().as_secs_f64() * 1000.0
    );
    println!("══════════════════════════════════════════════════════════════\n");
}

fn print_step(num: usize, name: &str) {
    println!("\n── Test {}: {} ──", num, name);
}

fn format_elapsed(_: usize) -> String {
    String::new()
}

// ─── 测试函数 ──────────────────────────────────────────────

/// Test 1: MockLLM 基础 - 固定响应和默认回显
fn test_mock_llm_basic() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // 固定响应
    let mock = MockLLM::with_response("Hello, I am Subhuti!");
    let result = rt.block_on(mock.chat(vec![Message::user("Hi")])).unwrap();
    assert_eq!(result, "Hello, I am Subhuti!");

    // 队列为空时默认回显用户消息
    let result2 = rt
        .block_on(mock.chat(vec![Message::user("What is Rust?")]))
        .unwrap();
    assert_eq!(result2, "What is Rust?");

    Ok("固定响应和默认回显均正常".to_string())
}

/// Test 2: 预设响应队列 - 按顺序消费
fn test_response_queue() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mock = MockLLM::new();

    mock.add_responses(vec!["第一个响应", "第二个响应", "第三个响应"]);

    let r1 = rt.block_on(mock.chat(vec![Message::user("Q1")])).unwrap();
    let r2 = rt.block_on(mock.chat(vec![Message::user("Q2")])).unwrap();
    let r3 = rt.block_on(mock.chat(vec![Message::user("Q3")])).unwrap();

    assert_eq!(r1, "第一个响应");
    assert_eq!(r2, "第二个响应");
    assert_eq!(r3, "第三个响应");
    assert_eq!(mock.get_call_count(), 3);

    Ok("3 个响应按顺序消费，调用次数正确".to_string())
}

/// Test 3: 消息捕获 - 验证 Prompt 构建
fn test_message_capture() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mock = MockLLM::with_response("captured!");

    let messages = vec![Message::system("你是一个助手"), Message::user("你好，世界")];

    rt.block_on(mock.chat(messages)).unwrap();

    let captured = mock.get_captured_messages();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].len(), 2);
    assert_eq!(captured[0][0].role, subhuti::Role::System);
    assert_eq!(captured[0][0].content, "你是一个助手");
    assert_eq!(captured[0][1].role, subhuti::Role::User);
    assert_eq!(captured[0][1].content, "你好，世界");

    // 验证 get_last_messages
    let last = mock.get_last_messages().unwrap();
    assert_eq!(last.len(), 2);

    Ok("消息历史完整捕获，角色和内容均正确".to_string())
}

/// Test 4: 工具调用响应
fn test_tool_call_response() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mock = MockLLM::new();

    // 添加工具调用预设
    mock.add_tool_call_response(ToolCall {
        id: "call_001".to_string(),
        name: "calculate".to_string(),
        arguments: serde_json::json!({"expression": "2 + 3"}),
    });

    let result = rt
        .block_on(mock.chat_with_tools(vec![Message::user("计算 2+3")], vec![]))
        .unwrap();

    assert!(result.tool_call.is_some());
    let tc = result.tool_call.unwrap();
    assert_eq!(tc.name, "calculate");
    assert_eq!(tc.arguments, serde_json::json!({"expression": "2 + 3"}));
    assert_eq!(result.total_tokens, Some(15));

    Ok("工具调用响应正确解析，参数匹配".to_string())
}

/// Test 5: 完整 Agent 链路 - 简单聊天
fn test_full_agent_chat() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let subhuti = Subhuti::new();

        // 注入 MockLLM
        let mock = MockLLM::with_response("你好！我是 Subhuti，很高兴为你服务。");
        subhuti.set_mock_llm(mock);

        // 注册 Skill
        subhuti.register_skill(DefaultChatSkill);

        assert!(subhuti.runtime().has_llm());

        // 调用 Agent（简单模式）
        let result = subhuti.run_simple("user1", "你好").await;
        match result {
            Ok((response, skill_used, tokens)) => {
                println!("  ├─ 响应: {}", response);
                println!("  ├─ 使用 Skill: {:?}", skill_used);
                println!(
                    "  ├─ Token: prompt={}, completion={}, total={}",
                    tokens.prompt_tokens, tokens.completion_tokens, tokens.total_tokens
                );
                Ok(format!("Agent 链路完整：响应正常，Skill={:?}", skill_used))
            }
            Err(e) => Err(format!("Agent 调用失败: {}", e)),
        }
    })
}

/// Test 6: 完整 Agent 链路 - 工具调用流程
fn test_full_agent_tool_call() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let subhuti = Subhuti::new();

        // MockLLM 预设工具调用 → 然后正常回复
        let mock = MockLLM::new();
        mock.add_tool_call_response(ToolCall {
            id: "call_calc_001".to_string(),
            name: "calculate".to_string(),
            arguments: serde_json::json!({"expression": "42 * 2"}),
        });
        mock.add_response("计算结果是 84");
        subhuti.set_mock_llm(mock);

        subhuti.register_skill(CalculatorSkill);

        // 注册计算器工具
        use async_trait::async_trait;
        use subhuti::runtime::tools::{Tool, ToolInfo, ToolResult};

        struct MockCalcTool;

        #[async_trait]
        impl Tool for MockCalcTool {
            fn info(&self) -> ToolInfo {
                ToolInfo {
                    name: "calculate".to_string(),
                    description: "计算表达式".to_string(),
                    parameters: serde_json::json!({"expression": "string"}),
                }
            }
            async fn run(&self, _params: serde_json::Value) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    success: true,
                    content: "84".to_string(),
                    error: None,
                })
            }
        }

        subhuti.runtime().register_tool(MockCalcTool);

        // 调用
        let result = subhuti.run_simple("user1", "计算 42 * 2").await;
        match result {
            Ok((response, skill_used, _tokens)) => {
                println!("  ├─ 响应: {}", response);
                println!("  ├─ 使用 Skill: {:?}", skill_used);
                Ok(format!("工具调用链路完整，Skill={:?}", skill_used))
            }
            Err(e) => Err(format!("工具调用链路失败: {}", e)),
        }
    })
}

/// Test 7: 流式输出模拟
fn test_streaming_mock() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let subhuti = Subhuti::new();
        let mock = MockLLM::with_response("这是 流式 输出 测试");
        subhuti.set_mock_llm(mock);
        subhuti.register_skill(DefaultChatSkill);

        let chunks: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let chunks_clone = chunks.clone();
        let callback = move |chunk: String| {
            chunks_clone.lock().unwrap().push(chunk);
        };

        let result = subhuti
            .run_simple_streaming("user1", "流式测试", Box::new(callback))
            .await;

        match result {
            Ok(response) => {
                let chunk_list = chunks.lock().unwrap();
                println!("  ├─ 完整响应: {}", response);
                println!("  ├─ 收到 {} 个流式块", chunk_list.len());
                Ok(format!("流式输出正常，共 {} 个块", chunk_list.len()))
            }
            Err(e) => Err(format!("流式调用失败: {}", e)),
        }
    })
}
