//! # LLM 重试 / 超时包装器
//!
//! 透明代理任意 `LLM` 实例，对**临时性失败**做指数退避重试，并给每次调用套上
//! 整体超时。这是 P0 韧性改造的核心：此前任何一次限流或网络抖动都会让整条
//! 编排链失败，前面已经跑完的 LLM 调用全部白费。
//!
//! ## 为什么单独立一个装饰器
//! 复用已有的 `CachedLLM` 装饰器形状，对调用方零侵入；`LLM` trait 本身不变，
//! 任何 provider（OpenAI / Ollama / Doubao / Zhipu）自动获得重试能力。
//!
//! ## 重试规则
//! 只重试**可重试**的错误（由 `subhuti_core::Error::Llm` 在产生处标注）：
//! - 网络超时、连接失败、请求未发出、响应体中断
//! - HTTP 408 / 429 / 5xx
//!
//! **不重试**：400 / 401 / 403 / 404 / 422 等（请求本身有问题，重试无意义）、
//! 响应解析失败（确定性错误）。
//!
//! ## 退避策略
//! `delay = min(base * 2^(n-1), max)`，再叠加 [0, delay/2] 的**抖动**，避免
//! 多路并发同时重试形成「惊群」。命中限流（429）时保底延迟不低于 `base` 且
//! 至少 1s，给服务端喘息时间。
//!
//! ## 流式特例
//! `chat_streaming` **只在尚未吐出任何 delta 时**才重试。一旦已经有内容推给
//! 用户，重试会导致重复输出——此时直接失败。
//!
//! ## 开启方式
//! 环境变量：
//! - `SUBHUTI_LLM_RETRY=0` 关闭（默认开启）
//! - `SUBHUTI_LLM_MAX_ATTEMPTS` 总尝试次数（默认 3，含首次）
//! - `SUBHUTI_LLM_RETRY_BASE_MS` 退避基数（默认 500）
//! - `SUBHUTI_LLM_RETRY_MAX_MS` 退避上限（默认 8000）
//! - `SUBHUTI_LLM_TIMEOUT_SECS` 单次调用整体超时（默认 120）

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use subhuti_core::runtime::llm::{LLMConfig, LLMProvider, LLMResponse, Message, ToolInfo, LLM};
use subhuti_core::{ClassifiableError, Error, ErrorKind};
use tracing::{info, warn};

/// 重试配置
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    /// 是否启用
    pub enabled: bool,
    /// 总尝试次数（含首次）
    pub max_attempts: u32,
    /// 退避基数（毫秒）
    pub base_delay_ms: u64,
    /// 退避上限（毫秒）
    pub max_delay_ms: u64,
    /// 单次调用整体超时（秒）
    pub timeout_secs: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            base_delay_ms: 500,
            max_delay_ms: 8_000,
            timeout_secs: 120,
        }
    }
}

impl RetryConfig {
    /// 从环境变量读取配置
    pub fn from_env() -> Self {
        let enabled = std::env::var("SUBHUTI_LLM_RETRY")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        Self {
            enabled,
            max_attempts: env_u64("SUBHUTI_LLM_MAX_ATTEMPTS", 3).clamp(1, 10) as u32,
            base_delay_ms: env_u64("SUBHUTI_LLM_RETRY_BASE_MS", 500).max(1),
            max_delay_ms: env_u64("SUBHUTI_LLM_RETRY_MAX_MS", 8_000).max(1),
            timeout_secs: env_u64("SUBHUTI_LLM_TIMEOUT_SECS", 120).max(1),
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

/// 重试决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryDecision {
    /// 值得重试（`rate_limited` 表示来自 429，退避需更保守）
    Retry { rate_limited: bool },
    /// 不值得重试，直接失败
    Stop,
}

/// 纯函数：判断某个错误是否值得重试。
///
/// 优先信任 `Error::Llm` 里由 HTTP 层标注的 `retryable`；对未类型化的错误，
/// 退回到 `ErrorKind` 兜底并嗅探常见的限流标记。
fn decide(err: &Error) -> RetryDecision {
    if let Error::Llm {
        retryable,
        status,
        message,
    } = err
    {
        if *retryable {
            return RetryDecision::Retry {
                rate_limited: *status == Some(429) || message.contains("429"),
            };
        }
        return RetryDecision::Stop;
    }
    if err.kind() == ErrorKind::Retryable {
        return RetryDecision::Retry {
            rate_limited: err.to_string().contains("429"),
        };
    }
    RetryDecision::Stop
}

/// 纯函数：计算第 `attempt` 次重试前的退避时长。
///
/// - `attempt` 从 1 开始（第一次重试）
/// - `rate_limited` 为真时保底延迟至少 1s，且不低于 `base_delay_ms`
/// - `jitter_seed` 由调用方提供，便于测试注入确定性抖动
pub fn compute_backoff(
    attempt: u32,
    cfg: &RetryConfig,
    rate_limited: bool,
    jitter_seed: u64,
) -> Duration {
    let shift = attempt.saturating_sub(1).min(16);
    let exp = cfg.base_delay_ms.saturating_mul(1u64 << shift);
    let mut delay = exp.min(cfg.max_delay_ms);
    if rate_limited {
        delay = delay.max(cfg.base_delay_ms.max(1_000));
    }
    // 抖动：叠加 [0, delay/2]
    let jitter = jitter_seed % (delay / 2 + 1);
    Duration::from_millis(delay + jitter)
}

fn jitter_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u64).rotate_left(17)
}

/// LLM 重试 / 超时包装器
///
/// 用法：
/// ```ignore
/// let real = ZhipuClient::new(cfg);
/// let retrying = RetryLLM::from_env(Arc::new(real));
/// ```
pub struct RetryLLM {
    inner: Arc<dyn LLM>,
    cfg: RetryConfig,
}

impl RetryLLM {
    /// 用显式配置包装
    pub fn wrap(inner: Arc<dyn LLM>, cfg: RetryConfig) -> Arc<Self> {
        info!(
            "[RetryLLM] 已启用：enabled={}, max_attempts={}, timeout={}s, backoff={}ms..{}ms",
            cfg.enabled, cfg.max_attempts, cfg.timeout_secs, cfg.base_delay_ms, cfg.max_delay_ms
        );
        Arc::new(Self { inner, cfg })
    }

    /// 从环境变量读取配置并包装
    pub fn from_env(inner: Arc<dyn LLM>) -> Arc<Self> {
        Self::wrap(inner, RetryConfig::from_env())
    }

    /// 当前配置
    pub fn config_retry(&self) -> RetryConfig {
        self.cfg
    }

    /// 计算下一次重试的等待时长
    fn backoff(&self, attempt: u32, err: &Error) -> Duration {
        let rate_limited = matches!(decide(err), RetryDecision::Retry { rate_limited: true });
        compute_backoff(attempt, &self.cfg, rate_limited, jitter_seed())
    }
}

/// 把一次调用包成「整体超时」的 future
async fn with_timeout<F, T>(fut: F, timeout: Duration, label: &str) -> subhuti_core::Result<T>
where
    F: std::future::Future<Output = subhuti_core::Result<T>>,
{
    match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r,
        Err(_) => Err(Error::llm(
            None,
            true,
            format!("LLM {label}调用超时（{}s）", timeout.as_secs()),
        )),
    }
}

#[async_trait]
impl LLM for RetryLLM {
    fn provider(&self) -> LLMProvider {
        self.inner.provider()
    }

    fn config(&self) -> &LLMConfig {
        self.inner.config()
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        if !self.cfg.enabled || self.cfg.max_attempts <= 1 {
            return with_timeout(self.inner.chat(messages), self.cfg.timeout(), "").await;
        }

        let mut attempt = 1u32;
        loop {
            let res = with_timeout(self.inner.chat(messages.clone()), self.cfg.timeout(), "").await;

            match res {
                Ok(v) => {
                    if attempt > 1 {
                        info!("[RetryLLM] chat 第 {} 次尝试成功", attempt);
                    }
                    return Ok(v);
                }
                Err(e) => {
                    let decision = decide(&e);
                    if matches!(decision, RetryDecision::Stop) || attempt >= self.cfg.max_attempts {
                        if attempt > 1 {
                            warn!("[RetryLLM] chat 重试 {} 次后仍失败：{}", attempt - 1, e);
                        }
                        return Err(e);
                    }
                    let delay = self.backoff(attempt, &e);
                    warn!(
                        "[RetryLLM] chat 第 {} 次失败（{}ms 后重试，共 {} 次）：{}",
                        attempt,
                        delay.as_millis(),
                        self.cfg.max_attempts,
                        e
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        if !self.cfg.enabled || self.cfg.max_attempts <= 1 {
            return with_timeout(
                self.inner.chat_with_tools(messages, tools),
                self.cfg.timeout(),
                "chat_with_tools ",
            )
            .await;
        }

        let mut attempt = 1u32;
        loop {
            let res = with_timeout(
                self.inner.chat_with_tools(messages.clone(), tools.clone()),
                self.cfg.timeout(),
                "chat_with_tools ",
            )
            .await;

            match res {
                Ok(v) => {
                    if attempt > 1 {
                        info!("[RetryLLM] chat_with_tools 第 {} 次尝试成功", attempt);
                    }
                    return Ok(v);
                }
                Err(e) => {
                    let decision = decide(&e);
                    if matches!(decision, RetryDecision::Stop) || attempt >= self.cfg.max_attempts {
                        return Err(e);
                    }
                    let delay = self.backoff(attempt, &e);
                    warn!(
                        "[RetryLLM] chat_with_tools 第 {} 次失败（{}ms 后重试）：{}",
                        attempt,
                        delay.as_millis(),
                        e
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        if !self.cfg.enabled || self.cfg.max_attempts <= 1 {
            return self.inner.chat_streaming(messages, callback).await;
        }

        // 是否已经吐出过 delta。一旦吐过就绝不再重试，否则会在用户侧重复输出。
        let emitted = Arc::new(AtomicBool::new(false));
        // 回调是 `Fn`（非 `FnMut`）且 trait object 隐含 'static：用 Mutex 包住以便
        // 在多次尝试之间共享；`Mutex<Box<dyn Fn + Send>>` 是 Send + Sync，可跨 await。
        let holder: Arc<Mutex<Box<dyn Fn(String) + Send>>> = Arc::new(Mutex::new(callback));

        let mut attempt = 1u32;
        loop {
            let emitted_this = emitted.clone();
            let holder = holder.clone();
            let cb: Box<dyn Fn(String) + Send> = Box::new(move |delta: String| {
                emitted_this.store(true, Ordering::SeqCst);
                if let Ok(f) = holder.lock() {
                    f(delta);
                }
            });

            let res = with_timeout(
                self.inner.chat_streaming(messages.clone(), cb),
                self.cfg.timeout(),
                "流式 ",
            )
            .await;

            match res {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if emitted.load(Ordering::SeqCst) {
                        warn!("[RetryLLM] 流式已输出部分内容，不再重试：{}", e);
                        return Err(e);
                    }
                    let decision = decide(&e);
                    if matches!(decision, RetryDecision::Stop) || attempt >= self.cfg.max_attempts {
                        return Err(e);
                    }
                    let delay = self.backoff(attempt, &e);
                    warn!(
                        "[RetryLLM] 流式第 {} 次失败（尚未输出内容，{}ms 后重试）：{}",
                        attempt,
                        delay.as_millis(),
                        e
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        // 健康检查本身就是探测，不做重试（避免探活被拖长）
        self.inner.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn fast_cfg() -> RetryConfig {
        RetryConfig {
            enabled: true,
            max_attempts: 3,
            base_delay_ms: 1,
            max_delay_ms: 4,
            timeout_secs: 5,
        }
    }

    // ─── 纯函数 ───────────────────────────────────────────────

    #[test]
    fn backoff_grows_exponentially_and_caps() {
        let cfg = RetryConfig {
            base_delay_ms: 100,
            max_delay_ms: 1_000,
            ..fast_cfg()
        };
        // 抖动注入 0，便于精确断言
        assert_eq!(
            compute_backoff(1, &cfg, false, 0),
            Duration::from_millis(100)
        );
        assert_eq!(
            compute_backoff(2, &cfg, false, 0),
            Duration::from_millis(200)
        );
        assert_eq!(
            compute_backoff(3, &cfg, false, 0),
            Duration::from_millis(400)
        );
        // 超过上限后被截断
        assert_eq!(
            compute_backoff(9, &cfg, false, 0),
            Duration::from_millis(1_000)
        );
    }

    #[test]
    fn backoff_rate_limited_has_floor() {
        let cfg = RetryConfig {
            base_delay_ms: 100,
            max_delay_ms: 1_000,
            ..fast_cfg()
        };
        // 429 保底 1s
        assert_eq!(
            compute_backoff(1, &cfg, true, 0),
            Duration::from_millis(1_000)
        );
    }

    #[test]
    fn backoff_jitter_stays_within_half() {
        let cfg = RetryConfig {
            base_delay_ms: 1_000,
            max_delay_ms: 10_000,
            ..fast_cfg()
        };
        for seed in [0u64, 1, 7, 499, 500, 10_000] {
            let d = compute_backoff(1, &cfg, false, seed).as_millis();
            assert!(
                (1_000..=1_500).contains(&d),
                "抖动应落在 [base, base*1.5]：seed={seed} d={d}"
            );
        }
    }

    #[test]
    fn decide_retries_transient_and_429() {
        let timeout = Error::llm(None, true, "网络超时");
        assert_eq!(
            decide(&timeout),
            RetryDecision::Retry {
                rate_limited: false
            }
        );

        let limited = Error::llm(Some(429), true, "Zhipu API HTTP 429: 频率超限");
        assert_eq!(
            decide(&limited),
            RetryDecision::Retry { rate_limited: true }
        );

        let server_err = Error::llm(Some(503), true, "服务不可用");
        assert_eq!(
            decide(&server_err),
            RetryDecision::Retry {
                rate_limited: false
            }
        );
    }

    #[test]
    fn decide_stops_on_client_errors() {
        for code in [400u16, 401, 403, 404, 422] {
            let e = Error::llm(Some(code), false, "请求有误");
            assert_eq!(decide(&e), RetryDecision::Stop, "HTTP {code} 不应重试");
        }
        // 解析失败（确定性）
        let parse = Error::llm(None, false, "响应解析失败");
        assert_eq!(decide(&parse), RetryDecision::Stop);
    }

    // ─── 组合行为 ─────────────────────────────────────────────

    struct FlakyLLM {
        config: LLMConfig,
        /// 前 N 次调用返回可重试错误
        fail_times: u32,
        calls: Arc<AtomicU32>,
        retryable: bool,
        /// 模拟「已输出 delta 后才失败」
        emit_before_fail: bool,
    }

    impl FlakyLLM {
        fn new(fail_times: u32, retryable: bool) -> (Arc<Self>, Arc<AtomicU32>) {
            let calls = Arc::new(AtomicU32::new(0));
            (
                Arc::new(Self {
                    config: LLMConfig::default(),
                    fail_times,
                    calls: calls.clone(),
                    retryable,
                    emit_before_fail: false,
                }),
                calls,
            )
        }

        fn e(&self) -> Error {
            Error::llm(
                if self.retryable { Some(503) } else { Some(400) },
                self.retryable,
                "模拟失败",
            )
        }
    }

    #[async_trait]
    impl LLM for FlakyLLM {
        fn provider(&self) -> LLMProvider {
            LLMProvider::Custom
        }
        fn config(&self) -> &LLMConfig {
            &self.config
        }
        async fn chat(&self, _messages: Vec<Message>) -> subhuti_core::Result<String> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_times {
                return Err(self.e());
            }
            Ok("ok".to_string())
        }
        async fn chat_with_tools(
            &self,
            _messages: Vec<Message>,
            _tools: Vec<ToolInfo>,
        ) -> subhuti_core::Result<LLMResponse> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_times {
                return Err(self.e());
            }
            Ok(LLMResponse {
                content: "ok".to_string(),
                tool_call: None,
                model: None,
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: None,
            })
        }
        async fn chat_streaming(
            &self,
            _messages: Vec<Message>,
            callback: Box<dyn Fn(String) + Send>,
        ) -> subhuti_core::Result<()> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if self.emit_before_fail && n < self.fail_times {
                callback("部分内容".to_string());
            }
            if n < self.fail_times {
                return Err(self.e());
            }
            callback("完整内容".to_string());
            Ok(())
        }
        async fn health_check(&self) -> subhuti_core::Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn retries_transient_then_succeeds() {
        let (inner, calls) = FlakyLLM::new(2, true);
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let out = llm.chat(vec![Message::user("hi")]).await.unwrap();
        assert_eq!(out, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 3, "应重试到第 3 次成功");
    }

    #[tokio::test]
    async fn does_not_retry_client_error() {
        let (inner, calls) = FlakyLLM::new(2, false);
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let err = llm.chat(vec![Message::user("hi")]).await.unwrap_err();
        assert!(!err.is_retryable());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "4xx 不应重试");
    }

    #[tokio::test]
    async fn exhausts_attempts_and_returns_error() {
        let (inner, calls) = FlakyLLM::new(99, true);
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let err = llm.chat(vec![Message::user("hi")]).await.unwrap_err();
        assert!(err.is_retryable());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "应恰好尝试 max_attempts 次"
        );
    }

    #[tokio::test]
    async fn retries_streaming_only_when_nothing_emitted() {
        let (inner, calls) = FlakyLLM::new(2, true);
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let chunks = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = chunks.clone();
        let cb: Box<dyn Fn(String) + Send> =
            Box::new(move |s: String| sink.lock().unwrap().push(s));
        llm.chat_streaming(vec![Message::user("hi")], cb)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn streaming_stops_retrying_after_partial_output() {
        let (_, base_calls) = FlakyLLM::new(99, true);
        // 手动构造「会先吐 delta 再失败」的实例
        let inner = Arc::new(FlakyLLM {
            config: LLMConfig::default(),
            fail_times: 99,
            calls: base_calls.clone(),
            retryable: true,
            emit_before_fail: true,
        });
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let chunks = Arc::new(Mutex::new(Vec::<String>::new()));
        let sink = chunks.clone();
        let cb: Box<dyn Fn(String) + Send> =
            Box::new(move |s: String| sink.lock().unwrap().push(s));
        let err = llm
            .chat_streaming(vec![Message::user("hi")], cb)
            .await
            .unwrap_err();
        assert!(err.is_retryable());
        assert_eq!(
            base_calls.load(Ordering::SeqCst),
            1,
            "已输出内容后不得重试（否则用户会看到重复）"
        );
        assert_eq!(chunks.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn disabled_passes_through_without_retry() {
        let (inner, calls) = FlakyLLM::new(99, true);
        let cfg = RetryConfig {
            enabled: false,
            ..fast_cfg()
        };
        let llm = RetryLLM::wrap(inner, cfg);
        let _ = llm.chat(vec![Message::user("hi")]).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn chat_with_tools_retries() {
        let (inner, calls) = FlakyLLM::new(1, true);
        let llm = RetryLLM::wrap(inner, fast_cfg());
        let out = llm
            .chat_with_tools(vec![Message::user("hi")], vec![])
            .await
            .unwrap();
        assert_eq!(out.content, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
