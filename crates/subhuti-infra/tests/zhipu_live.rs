//! Live integration tests for Zhipu (智谱) client.
//!
//! 自动从项目根目录的 .env 文件读取 ZHIPU_API_KEY（通过 dotenvy），
//! 也支持显式通过环境变量传入：
//!
//! ```bash
//! cargo test --package subhuti-infra --test zhipu_live -- --nocapture
//! ```

use subhuti_core::runtime::llm::{Message, LLM};
use subhuti_infra::{ZhipuClient, ZhipuConfig};

fn load_dotenv() {
    // dotenvy 自动向上查找 .env，一般能找到 workspace 根目录下的那份；找不到就静默忽略
    let _ = dotenvy::dotenv();
}

fn make_client(model: &str) -> ZhipuClient {
    load_dotenv();
    let api_key = std::env::var("ZHIPU_API_KEY")
        .expect("ZHIPU_API_KEY 未在 .env 或环境变量中设置，跳过 live 测试");
    ZhipuClient::new(ZhipuConfig {
        api_key,
        api_url: "https://open.bigmodel.cn/api/paas/v4".to_string(),
        model: model.to_string(),
        temperature: 0.7,
        max_tokens: 128,
    })
}

#[tokio::test]
#[ignore = "live test：打真实智谱 API，需手动 --include-ignored 运行"]
async fn zhipu_health_check_glm4_flash() {
    load_dotenv();
    if std::env::var("ZHIPU_API_KEY").is_err() {
        eprintln!("⚠️  ZHIPU_API_KEY 未设置，跳过 health_check 测试");
        return;
    }
    let client = make_client("glm-4-flash");
    let ok = client.health_check().await.expect("HTTP 请求不应失败");
    println!("✅ glm-4-flash health_check -> {}", ok);
    assert!(ok, "health_check 应返回 true");
}

#[tokio::test]
#[ignore = "live test：打真实智谱 API，需手动 --include-ignored 运行"]
async fn zhipu_health_check_glm47_flash() {
    load_dotenv();
    if std::env::var("ZHIPU_API_KEY").is_err() {
        eprintln!("⚠️  ZHIPU_API_KEY 未设置，跳过 glm-4.7-flash health_check 测试");
        return;
    }
    let client = make_client("glm-4.7-flash");
    let ok = client.health_check().await.expect("HTTP 请求不应失败");
    println!("✅ glm-4.7-flash health_check -> {}", ok);
    assert!(ok, "health_check 应返回 true");
}

#[tokio::test]
#[ignore = "live test：打真实智谱 API，需手动 --include-ignored 运行"]
async fn zhipu_chat_glm4_flash() {
    load_dotenv();
    if std::env::var("ZHIPU_API_KEY").is_err() {
        eprintln!("⚠️  ZHIPU_API_KEY 未设置，跳过 chat 测试");
        return;
    }
    let client = make_client("glm-4-flash");
    let reply = client
        .chat(vec![Message::user("用一句话介绍 Rust 语言")])
        .await
        .expect("chat 调用不应失败");
    println!(
        "✅ glm-4-flash chat 回复（长度 {}）：{}",
        reply.chars().count(),
        reply
    );
    assert!(!reply.is_empty(), "回复内容不能为空");
}

#[tokio::test]
#[ignore = "live test：打真实智谱 API，需手动 --include-ignored 运行"]
async fn zhipu_chat_glm47_flash_reasoning() {
    load_dotenv();
    if std::env::var("ZHIPU_API_KEY").is_err() {
        eprintln!("⚠️  ZHIPU_API_KEY 未设置，跳过 glm-4.7-flash 深度思考测试");
        return;
    }
    let client = make_client("glm-4.7-flash");
    let reply = client
        .chat(vec![Message::user(
            "2 的 20 次方等于多少？请先一步步思考再给出结果",
        )])
        .await
        .expect("chat 调用不应失败");
    println!(
        "✅ glm-4.7-flash（深度思考模型）回复（长度 {}）：{}",
        reply.chars().count(),
        reply
    );
    assert!(!reply.is_empty(), "回复内容不能为空");
}
