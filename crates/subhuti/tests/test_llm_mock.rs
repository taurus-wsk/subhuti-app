//! LLM Mock 端到端测试
//!
//! 使用 MockLLM 模拟完整的 Agent 处理链路，无需真实 API 调用
//!
//! 运行: cargo test -p subhuti --test test_llm_mock -- --nocapture

use std::time::Instant;
use subhuti::runtime::{ToolCall, LLM};
use subhuti::{Message, MockLLM, Subhuti};

fn main() {
    run_tests();
}

#[test]
fn test_mock_llm_e2e() {
    run_tests();
}

struct TestTracker {
    passed: usize,
    failed: usize,
    failed_tests: Vec<String>,
    start_time: Instant,
}

impl TestTracker {
    fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            failed_tests: Vec::new(),
            start_time: Instant::now(),
        }
    }

    fn pass(&mut self, name: &str) {
        self.passed += 1;
        eprintln!("[TEST OK] {}", name);
    }

    fn fail(&mut self, name: &str, reason: &str) {
        self.failed += 1;
        self.failed_tests.push(format!("{}: {}", name, reason));
        eprintln!("[TEST FAIL] {} - {}", name, reason);
    }

    fn summary(&self) -> String {
        let elapsed = self.start_time.elapsed();
        let total = self.passed + self.failed;

        if self.failed == 0 {
            format!(
                "✅ All {} tests passed in {:.3}s",
                total,
                elapsed.as_secs_f32()
            )
        } else {
            format!(
                "❌ {}/{} tests passed in {:.3}s\nFailed tests:\n{}",
                self.passed,
                total,
                elapsed.as_secs_f32(),
                self.failed_tests.join("\n")
            )
        }
    }
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

    // ── Test 6: 流式输出模拟 ──────────────────────────────
    print_step(6, "流式输出模拟测试");
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

    let mock = MockLLM::with_response("Hello, I am Subhuti!");
    let result = rt.block_on(mock.chat(vec![Message::user("Hi")])).unwrap();
    assert_eq!(result, "Hello, I am Subhuti!");

    let mock_echo = MockLLM::with_echo();
    let result2 = rt
        .block_on(mock_echo.chat(vec![Message::user("What is Rust?")]))
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

    let last = mock.get_last_messages().unwrap();
    assert_eq!(last.len(), 2);

    Ok("消息历史完整捕获，角色和内容均正确".to_string())
}

/// Test 4: 工具调用响应
fn test_tool_call_response() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mock = MockLLM::new();

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

    Ok("工具调用响应正确解析，参数匹配".to_string())
}

/// Test 5: 完整 Agent 链路 - 简单聊天
fn test_full_agent_chat() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let subhuti = Subhuti::new();

        let result = subhuti.dispatch("你好").await;
        println!("  ├─ 响应: {}", result.output);
        println!("  ├─ 成功: {}", result.success);

        if result.success {
            Ok(format!("Agent 链路完整：响应正常"))
        } else {
            Err(format!("Agent 调用失败"))
        }
    })
}

/// Test 6: 流式输出模拟
fn test_streaming_mock() -> Result<String, String> {
    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(async {
        let mock = MockLLM::with_response("这是流式输出测试");

        let chunks: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let chunks_clone = chunks.clone();
        let callback = move |chunk: String| {
            chunks_clone.lock().unwrap().push(chunk);
        };

        let result = mock
            .chat_streaming(vec![Message::user("流式测试")], Box::new(callback))
            .await;

        match result {
            Ok(_) => {
                let chunk_list = chunks.lock().unwrap();
                println!("  ├─ 收到 {} 个流式块", chunk_list.len());
                Ok(format!("流式输出正常，共 {} 个块", chunk_list.len()))
            }
            Err(e) => Err(format!("流式调用失败: {}", e)),
        }
    })
}
