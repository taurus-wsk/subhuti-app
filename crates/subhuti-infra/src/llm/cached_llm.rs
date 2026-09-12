//! # LLM 调试缓存包装器
//!
//! 透明代理任意 `LLM` 实例：相同输入（messages + model）第二次调用时
//! 直接返回缓存结果，避免每次本地调试都打真实的智谱 / OpenAI API。
//!
//! ## 特性
//! - 文件持久化：进程重启后缓存仍然有效（默认 `.llm-cache.json`）
//! - LRU 淘汰：最多保留 100 条，超出删最旧
//! - 透明代理：所有 `LLM` trait 方法都委托给内部真实 client
//! - 命中/未命中计数：通过 `stats()` 查询，便于调试
//!
//! ## 开启方式
//! 在 `SubhutiFrameworkInitializer` 中，当环境变量 `SUBHUTI_LLM_CACHE=1`
//! 时，自动用 `CachedLLM::wrap(real_llm, path)` 包装。
//!
//! ## 缓存 Key
//! `model + "\n" + 各 message 的 role + content`，做 128 位 hash（两段 u64）后
//! hex 编码。相同输入命中相同 key。
//!
//! ## 不缓存
//! - `health_check`：每次都打真实 API（轻量 ping，不影响调试）
//! - `chat_streaming`：流式响应直接透传

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subhuti_core::runtime::llm::{LLMConfig, LLMProvider, LLMResponse, Message, ToolInfo, LLM};
use tracing::{debug, info, warn};

/// 缓存上限（超出后按 LRU 删除最旧条目）
const DEFAULT_MAX_ENTRIES: usize = 100;

/// 单条缓存记录
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    /// 序号（单调递增），用于 LRU 淘汰
    seq: u64,
    /// `chat` 接口的返回内容
    chat_response: Option<String>,
    /// `chat_with_tools` 接口的返回内容（完整序列化）
    chat_with_tools_response: Option<LLMResponseSerde>,
}

/// `LLMResponse` 的可序列化镜像（`LLMResponse` 本身未实现 Serialize）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LLMResponseSerde {
    content: String,
    tool_call: Option<ToolCallSerde>,
    model: Option<String>,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallSerde {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

impl From<&subhuti_core::runtime::llm::ToolCall> for ToolCallSerde {
    fn from(t: &subhuti_core::runtime::llm::ToolCall) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            arguments: t.arguments.clone(),
        }
    }
}

impl From<ToolCallSerde> for subhuti_core::runtime::llm::ToolCall {
    fn from(t: ToolCallSerde) -> Self {
        Self {
            id: t.id,
            name: t.name,
            arguments: t.arguments,
        }
    }
}

impl From<&LLMResponse> for LLMResponseSerde {
    fn from(r: &LLMResponse) -> Self {
        Self {
            content: r.content.clone(),
            tool_call: r.tool_call.as_ref().map(Into::into),
            model: r.model.clone(),
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
        }
    }
}

impl From<LLMResponseSerde> for LLMResponse {
    fn from(r: LLMResponseSerde) -> Self {
        Self {
            content: r.content,
            tool_call: r.tool_call.map(Into::into),
            model: r.model,
            prompt_tokens: r.prompt_tokens,
            completion_tokens: r.completion_tokens,
            total_tokens: r.total_tokens,
        }
    }
}

/// 缓存命中/未命中统计
#[derive(Debug, Default, Clone, Copy)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub saved_calls: u64,
}

/// LLM 调试缓存包装器
///
/// 用法：
/// ```ignore
/// let real = ZhipuClient::new(cfg);
/// let cached = CachedLLM::wrap(Arc::new(real), ".llm-cache.json");
/// subhuti.set_llm(cached);
/// ```
pub struct CachedLLM {
    inner: Arc<dyn LLM>,
    state: Mutex<CacheState>,
}

struct CacheState {
    /// 磁盘文件路径（每次写入都覆盖）
    file_path: PathBuf,
    /// key -> entry
    entries: HashMap<String, CacheEntry>,
    /// 下一个 seq 号（用于 LRU）
    next_seq: u64,
    /// 最大条目数
    max_entries: usize,
    /// 命中/未命中统计
    stats: CacheStats,
}

impl CachedLLM {
    /// 用默认上限（100）和给定路径包装一个真实 LLM
    pub fn wrap(inner: Arc<dyn LLM>, file_path: impl Into<PathBuf>) -> Arc<Self> {
        Self::with_capacity(inner, file_path, DEFAULT_MAX_ENTRIES)
    }

    /// 指定上限
    pub fn with_capacity(
        inner: Arc<dyn LLM>,
        file_path: impl Into<PathBuf>,
        max_entries: usize,
    ) -> Arc<Self> {
        let file_path = file_path.into();
        let (entries, next_seq) = load_from_file(&file_path);
        let loaded = entries.len();
        info!(
            "[CachedLLM] 已加载缓存：path={:?}, 条目数={}, max_entries={}, next_seq={}",
            file_path, loaded, max_entries, next_seq
        );

        Arc::new(Self {
            inner,
            state: Mutex::new(CacheState {
                file_path,
                entries,
                next_seq,
                max_entries,
                stats: CacheStats::default(),
            }),
        })
    }

    /// 查询命中统计
    pub fn stats(&self) -> CacheStats {
        self.state.lock().unwrap().stats
    }

    /// 清空内存 + 磁盘缓存
    pub fn clear(&self) {
        let mut st = self.state.lock().unwrap();
        st.entries.clear();
        st.next_seq = 0;
        st.stats = CacheStats::default();
        let _ = fs::write(&st.file_path, b"{}");
        info!("[CachedLLM] 缓存已清空: {:?}", st.file_path);
    }

    /// 计算 cache key：`model + messages(role+content)` 的 128-bit hash hex
    ///
    /// 必须使用**确定性**哈希：此前用 `DefaultHasher`（SipHash，每进程随机种子），
    /// 导致进程重启后同一输入算出的 key 完全不同，磁盘缓存 100% 失效——
    /// 表现为「缓存文件有内容，但重启后一条都命中不了」。
    /// 这里改用 SHA-256（前 128 bit），跨进程、跨机器结果稳定。
    fn make_key(model: &str, messages: &[Message]) -> String {
        let mut hasher = Sha256::new();
        // 0x1f 作字段分隔符，避免 "ab"+"c" 与 "a"+"bc" 拼出相同摘要
        hasher.update(model.as_bytes());
        hasher.update([0x1f]);
        for m in messages {
            hasher.update(format!("{:?}", m.role).as_bytes());
            hasher.update([0x1f]);
            hasher.update(m.content.as_bytes());
            hasher.update([0x1f]);
            hasher.update(format!("{:?}", m.tool_call_id).as_bytes());
            hasher.update([0x1f]);
        }
        let digest = hasher.finalize();
        // 取前 16 字节（128 bit），与原先两段 u64 hex 的长度保持一致
        format!("{:x}", digest)[..32].to_string()
    }

    /// 命中缓存时取出，返回 Some(...)
    fn lookup_chat(&self, key: &str) -> Option<String> {
        let mut st = self.state.lock().unwrap();
        // 先用 immutable 读出 next_seq（Copy），避免后面 get_mut 与 stats 同时 mutable 借用
        let seq = st.next_seq;
        let mut hit = false;
        let result = if let Some(e) = st.entries.get_mut(key) {
            hit = true;
            e.seq = seq;
            e.chat_response.clone()
        } else {
            None
        };
        // 块外更新统计（e 已离开作用域，mutable borrow 释放）
        if hit {
            st.next_seq += 1;
            st.stats.hits += 1;
            st.stats.saved_calls += 1;
            debug!("[CachedLLM] HIT chat: key={}..", &key[..8.min(key.len())]);
        } else {
            st.stats.misses += 1;
        }
        result
    }

    fn lookup_chat_with_tools(&self, key: &str) -> Option<LLMResponse> {
        let mut st = self.state.lock().unwrap();
        let seq = st.next_seq;
        let mut hit = false;
        let result = if let Some(e) = st.entries.get_mut(key) {
            hit = true;
            e.seq = seq;
            e.chat_with_tools_response.clone()
        } else {
            None
        };
        if hit {
            st.next_seq += 1;
            st.stats.hits += 1;
            st.stats.saved_calls += 1;
            debug!(
                "[CachedLLM] HIT chat_with_tools: key={}..",
                &key[..8.min(key.len())]
            );
        } else {
            st.stats.misses += 1;
        }
        result.map(LLMResponse::from)
    }

    /// 写入缓存（含 LRU 淘汰 + 落盘）
    fn store_chat(&self, key: String, response: String) {
        let mut st = self.state.lock().unwrap();
        let seq = st.next_seq;
        st.next_seq += 1;
        st.entries.insert(
            key.clone(),
            CacheEntry {
                seq,
                chat_response: Some(response),
                chat_with_tools_response: None,
            },
        );
        evict_if_needed(&mut st);
        persist(&st);
    }

    fn store_chat_with_tools(&self, key: String, response: LLMResponse) {
        let mut st = self.state.lock().unwrap();
        let seq = st.next_seq;
        st.next_seq += 1;
        st.entries.insert(
            key.clone(),
            CacheEntry {
                seq,
                chat_response: None,
                chat_with_tools_response: Some(LLMResponseSerde::from(&response)),
            },
        );
        evict_if_needed(&mut st);
        persist(&st);
    }
}

#[async_trait]
impl LLM for CachedLLM {
    fn provider(&self) -> LLMProvider {
        self.inner.provider()
    }

    fn config(&self) -> &LLMConfig {
        self.inner.config()
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        let key = Self::make_key(&self.inner.config().model, &messages);
        if let Some(cached) = self.lookup_chat(&key) {
            info!(
                "[CachedLLM] 命中缓存，跳过 LLM 调用 (key={}.., resp_len={})",
                &key[..8.min(key.len())],
                cached.len()
            );
            return Ok(cached);
        }
        info!("[CachedLLM] 未命中，调用真实 LLM ...");
        let resp = self.inner.chat(messages).await?;
        self.store_chat(key, resp.clone());
        Ok(resp)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        // 注意：tools 不参与 cache key（同一个 prompt 不同 tools 的情况较少，
        // 而且调试时一般 tools 不变）。如果未来需要支持，可把 tools 序列化拼到 key 里。
        let key = Self::make_key(&self.inner.config().model, &messages);
        if let Some(cached) = self.lookup_chat_with_tools(&key) {
            info!(
                "[CachedLLM] 命中缓存 (chat_with_tools)，跳过 LLM 调用 (key={}..)",
                &key[..8.min(key.len())]
            );
            return Ok(cached);
        }
        info!("[CachedLLM] 未命中 (chat_with_tools)，调用真实 LLM ...");
        let resp = self.inner.chat_with_tools(messages, tools).await?;
        self.store_chat_with_tools(key, resp.clone());
        Ok(resp)
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        // 流式不缓存，直接透传
        self.inner.chat_streaming(messages, callback).await
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        // 健康检查每次都打真实 API（轻量 ping）
        self.inner.health_check().await
    }
}

// ─── 私有辅助函数 ───────────────────────────────────────────────

fn load_from_file(path: &Path) -> (HashMap<String, CacheEntry>, u64) {
    let mut entries = HashMap::new();
    let mut next_seq = 1u64;

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (entries, next_seq), // 文件不存在，正常
    };

    if content.trim().is_empty() || content.trim() == "{}" {
        return (entries, next_seq);
    }

    match serde_json::from_str::<HashMap<String, CacheEntry>>(&content) {
        Ok(map) => {
            for (k, v) in map {
                if v.seq >= next_seq {
                    next_seq = v.seq + 1;
                }
                entries.insert(k, v);
            }
        }
        Err(e) => {
            warn!(
                "[CachedLLM] 缓存文件解析失败，将忽略已有缓存: {:?}, err={}",
                path, e
            );
        }
    }
    (entries, next_seq)
}

fn evict_if_needed(st: &mut CacheState) {
    if st.entries.len() <= st.max_entries {
        return;
    }
    // 找到 seq 最小（最旧）的 key，删除
    while st.entries.len() > st.max_entries {
        if let Some((oldest_key, _)) = st
            .entries
            .iter()
            .min_by_key(|(_, v)| v.seq)
            .map(|(k, v)| (k.clone(), v.seq))
        {
            st.entries.remove(&oldest_key);
            debug!(
                "[CachedLLM] LRU 淘汰: key={}..",
                &oldest_key[..8.min(oldest_key.len())]
            );
        } else {
            break;
        }
    }
}

fn persist(st: &CacheState) {
    let json = match serde_json::to_string_pretty(&st.entries) {
        Ok(s) => s,
        Err(e) => {
            warn!("[CachedLLM] 缓存序列化失败: {}", e);
            return;
        }
    };
    if let Err(e) = fs::write(&st.file_path, json) {
        warn!(
            "[CachedLLM] 缓存写入磁盘失败: {:?}, err={}",
            st.file_path, e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use subhuti_core::runtime::llm::{LLMConfig, LLMProvider, LLMResponse, Message};

    struct EchoLLM {
        config: LLMConfig,
    }

    #[async_trait]
    impl LLM for EchoLLM {
        fn provider(&self) -> LLMProvider {
            LLMProvider::Custom
        }
        fn config(&self) -> &LLMConfig {
            &self.config
        }
        async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
            Ok(format!("echo:{}", messages.last().unwrap().content))
        }
        async fn chat_with_tools(
            &self,
            messages: Vec<Message>,
            _tools: Vec<ToolInfo>,
        ) -> subhuti_core::Result<LLMResponse> {
            Ok(LLMResponse {
                content: format!("echo_tool:{}", messages.last().unwrap().content),
                tool_call: None,
                model: Some(self.config.model.clone()),
                prompt_tokens: Some(10),
                completion_tokens: Some(5),
                total_tokens: Some(15),
            })
        }
        async fn chat_streaming(
            &self,
            _messages: Vec<Message>,
            _callback: Box<dyn Fn(String) + Send>,
        ) -> subhuti_core::Result<()> {
            Ok(())
        }
        async fn health_check(&self) -> subhuti_core::Result<bool> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn cache_hit_skips_inner_call() {
        let tmp = tempfile_path();
        let _ = std::fs::remove_file(&tmp);

        let inner = Arc::new(EchoLLM {
            config: LLMConfig::default(),
        });
        let cached = CachedLLM::wrap(inner.clone(), &tmp);

        let m = vec![Message::user("hello")];
        let r1 = cached.chat(m.clone()).await.unwrap();
        assert_eq!(r1, "echo:hello");
        let r2 = cached.chat(m.clone()).await.unwrap();
        assert_eq!(r2, "echo:hello");
        assert_eq!(cached.stats().hits, 1);
        assert_eq!(cached.stats().misses, 1);

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn cache_persists_across_instances() {
        let tmp = tempfile_path();
        let _ = std::fs::remove_file(&tmp);

        let inner = Arc::new(EchoLLM {
            config: LLMConfig::default(),
        });
        {
            let cached = CachedLLM::wrap(inner.clone(), &tmp);
            let _ = cached.chat(vec![Message::user("ping")]).await.unwrap();
        }
        // 新实例应当从磁盘加载并命中
        let cached2 = CachedLLM::wrap(inner.clone(), &tmp);
        let r = cached2.chat(vec![Message::user("ping")]).await.unwrap();
        assert_eq!(r, "echo:ping");
        assert_eq!(cached2.stats().hits, 1);
        assert_eq!(cached2.stats().misses, 0);

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn lru_evicts_oldest() {
        let tmp = tempfile_path();
        let _ = std::fs::remove_file(&tmp);

        let inner = Arc::new(EchoLLM {
            config: LLMConfig::default(),
        });
        let cached = CachedLLM::with_capacity(inner.clone(), &tmp, 3);

        // 写 4 条，应当保留最后 3 条
        for i in 0..4 {
            let _ = cached
                .chat(vec![Message::user(format!("msg{}", i))])
                .await
                .unwrap();
        }
        let st = cached.state.lock().unwrap();
        assert_eq!(st.entries.len(), 3);
        // msg0 应当被淘汰
        let key0 = CachedLLM::make_key("gpt-4", &[Message::user("msg0")]);
        assert!(!st.entries.contains_key(&key0));
        drop(st);

        let _ = std::fs::remove_file(&tmp);
    }

    /// 回归防护：cache key 必须是**确定性**的（跨进程稳定）
    ///
    /// 此前用 `DefaultHasher`（SipHash，每进程随机种子），重启后同一输入的 key
    /// 全部改变，磁盘缓存 100% 失效。这里锁定三条不变量。
    #[test]
    fn make_key_is_deterministic_and_unambiguous() {
        let msgs = vec![Message::user("hello"), Message::user("world")];
        let k1 = CachedLLM::make_key("glm-4-flash", &msgs);
        let k2 = CachedLLM::make_key("glm-4-flash", &msgs);

        assert_eq!(k1, k2, "相同输入必须产生相同 key");
        assert_eq!(k1.len(), 32, "128-bit hex 应为 32 个字符");
        assert!(
            k1.chars().all(|c| c.is_ascii_hexdigit()),
            "key 应为纯 hex: {}",
            k1
        );

        // 字段边界不能混淆：拆分/合并消息必须得到不同 key
        let joined = vec![Message::user("helloworld")];
        assert_ne!(
            CachedLLM::make_key("m", &joined),
            CachedLLM::make_key("m", &msgs),
            "消息之间必须有分隔符，否则 'ab'+'c' 与 'a'+'bc' 会碰撞"
        );

        // 模型名参与哈希
        assert_ne!(
            CachedLLM::make_key("model-a", &msgs),
            CachedLLM::make_key("model-b", &msgs),
            "换模型必须换 key"
        );
    }

    fn tempfile_path() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "subhuti-cached-llm-test-{}-{}.json",
            std::process::id(),
            n
        ));
        p
    }
}
