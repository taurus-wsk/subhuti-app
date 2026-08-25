//! # 主动提问（挂起 - 恢复）
//!
//! 专家在规划阶段信息不足时，可向用户发起**单选提问**：
//! 1. 通过 `progress_tx` 推送 `{"type":"ask",...}` 事件 → 前端渲染提问卡片
//! 2. 在本注册表登记一个挂起通道，阻塞等待 `/ask-resolve` 投递回答
//! 3. 收到回答后返回给调用方，专家带着回答继续执行
//!
//! 超时（TTL）未回答时返回空字符串，避免挂起任务泄漏。
//! 采用一次性注册表（全局静态），无需注入 DI——与 `progress_tx` 全局注册表同款轻量模式。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tokio::sync::mpsc;

/// 挂起中提问的回答投递通道表：ask_id -> sender
static PENDING: OnceLock<Mutex<HashMap<String, mpsc::UnboundedSender<String>>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

fn map() -> &'static Mutex<HashMap<String, mpsc::UnboundedSender<String>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> String {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("ask-{}-{}", ms, n)
}

/// 提问超时（未在 TTL 内回答则视为取消，返回空字符串继续）。
/// 取一个「用户来得及看一眼并点击，但又不至于长时间挂起」的折中值；
/// 前端若没接到 Ask 事件/未渲染卡片，本 TTL 过后调用方会返回空回答继续执行（而非永久阻塞）。
const TTL: Duration = Duration::from_secs(60);

/// 向用户发起一次单选提问并等待回答。
///
/// 返回用户的选择文本；超时未回答返回空字符串。
pub async fn ask_user(
    progress_tx: &Option<mpsc::Sender<String>>,
    question: &str,
    options: Vec<String>,
) -> String {
    let ask_id = next_id();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    map().lock().unwrap().insert(ask_id.clone(), tx);

    // 推送 Ask 事件（前端据此渲染提问卡片）
    let evt = serde_json::json!({
        "type": "ask",
        "ask_id": ask_id,
        "question": question,
        "options": options,
    })
    .to_string();
    if let Some(sender) = progress_tx {
        let _ = sender.try_send(evt);
    }

    // 阻塞等待回答（带超时）
    let answer = match tokio::time::timeout(TTL, rx.recv()).await {
        Ok(Some(a)) => a,
        Ok(None) => String::new(),
        Err(_) => String::new(), // 超时
    };

    map().lock().unwrap().remove(&ask_id);
    answer
}

/// 由恢复接口调用：把用户回答投递给挂起的提问。
/// 返回是否找到对应挂起提问。
pub fn resolve(ask_id: &str, answer: String) -> bool {
    let sender = match map().lock().unwrap().get(ask_id) {
        Some(s) => s.clone(),
        None => return false,
    };
    let _ = sender.send(answer);
    true
}

/// 测试辅助：当前挂起中的提问数
#[cfg(test)]
pub fn pending_count() -> usize {
    map().lock().unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ask_and_resolve_roundtrip() {
        let (tx, mut rx) = mpsc::channel::<String>(8);
        // 注册进去，等待回答
        let ask = tokio::spawn(async move {
            ask_user(
                &Some(tx),
                "选择语言?",
                vec!["Rust".to_string(), "Go".to_string()],
            )
            .await
        });
        // 等待 Ask 事件 + 拿 ask_id
        let evt_json = rx.recv().await.unwrap();
        let v: serde_json::Value = serde_json::from_str(&evt_json).unwrap();
        assert_eq!(v["type"], "ask");
        let ask_id = v["ask_id"].as_str().unwrap();
        assert_eq!(pending_count(), 1);
        // 投递回答
        assert!(resolve(ask_id, "Rust".to_string()));
        let answer = ask.await.unwrap();
        assert_eq!(answer, "Rust");
        assert_eq!(pending_count(), 0);
    }

    #[tokio::test]
    async fn test_resolve_unknown_returns_false() {
        assert!(!resolve("ask-no-such".to_string().leak(), "x".to_string()));
    }
}
