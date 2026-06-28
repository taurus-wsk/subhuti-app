//! 同步测试二进制 - 测试 chat_sync
//! 运行: cargo run --bin sync_test

use anyhow::Result;
use subhuti::runtime::llm::{DoubaoClient, LLMConfig, Message};

fn main() -> Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    println!("=== Doubao Sync Test ===\n");

    let config = LLMConfig {
        model: "doubao-seed-2-0-lite-260215".to_string(),
        api_url: "https://ark.cn-beijing.volces.com/api/v3/responses".to_string(),
        api_key: None,
        temperature: 0.7,
        max_tokens: 2048,
    };

    let doubao = DoubaoClient::new(config)?;
    println!("✓ Doubao 客户端创建成功\n");

    println!("测试同步调用...");
    match doubao.chat_sync(vec![Message::user("你好")]) {
        Ok(response) => println!("✓ 响应: {}", response),
        Err(e) => println!("✗ 失败: {}", e),
    }

    Ok(())
}
