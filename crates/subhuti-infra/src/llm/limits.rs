//! # LLM 上下文裁剪包装器
//!
//! 透明代理任意 `LLM` 实例，在**调用真实模型之前**对 `messages` 做长度裁剪，
//! 防止长会话把整段历史原样塞进 prompt 导致 token 爆炸 / 请求被拒。
//!
//! ## 为什么需要
//! 编排链路会把会话历史拼进 messages，此前没有任何上限。多轮长对话（尤其编码
//! 类任务反复回灌错误、追加文件内容）会让请求体迅速膨胀。这是「先粗暴设上限、
//! 后续再做精确内容选择」的第一版：只保证**不超限**，不判断哪条消息更相关。
//!
//! ## 裁剪策略（crude v1）
//! 1. **最后一条恒保留且不截断**：它代表当前输入，丢了或截了请求就没意义了。
//! 2. **System 尽量全留**：视为「指令头」。若系统提示自己就超预算，则**丢弃靠前的
//!    几条**，并对最后一条按剩余预算**截断内容**（保留开头，通常是角色定义与规则）。
//! 3. **其余按最近优先**：从倒数第二条往前回填，直到触及 `max_messages`
//!    或 `max_chars` 任一预算。
//! 4. **裁剪发生时插入提示**：说明省略了多少条，避免模型误以为对话就此开始。
//! 5. **窗口开头是 Tool 消息则丢弃**：其配对的 Assistant tool_call 多半已被裁掉，
//!    留下会导致协议不合法。
//!
//! ## 已知取舍（后续再精化）
//! - 按**字符数**估算长度，不按 token 计（无需引入 tokenizer）。
//! - 不做消息重要性排序（例如"含文件路径的旧消息"不额外加权）。
//! - 只约束"传入"方向；输出长度由 `LLMConfig::max_tokens` 控制。
//!
//! ## 开启方式
//! - `SUBHUTI_LLM_LIMIT=0` 关闭裁剪（默认开启）
//! - `SUBHUTI_LLM_MAX_MESSAGES` 消息条数上限（默认 20）
//! - `SUBHUTI_LLM_MAX_CHARS` 总字符数上限（默认 32000）

use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::runtime::llm::{
    LLMConfig, LLMProvider, LLMResponse, Message, Role, ToolInfo, LLM,
};
use tracing::{debug, info};

/// 裁剪提示消息预留的字符数
const NOTE_RESERVE_CHARS: usize = 96;
/// 截断系统提示时保留给结束标记的字符数
const TRUNCATE_MARKER_CHARS: usize = 24;

/// 裁剪配置
#[derive(Debug, Clone, Copy)]
pub struct LimitConfig {
    /// 是否启用（`SUBHUTI_LLM_LIMIT=0` 关闭）
    pub enabled: bool,
    /// 消息条数上限（含系统消息、裁剪提示、当前输入）
    pub max_messages: usize,
    /// 总字符数上限
    pub max_chars: usize,
}

impl Default for LimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_messages: 20,
            max_chars: 32_000,
        }
    }
}

impl LimitConfig {
    /// 从环境变量读取配置
    pub fn from_env() -> Self {
        let enabled = std::env::var("SUBHUTI_LLM_LIMIT")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);
        let max_messages = env_usize("SUBHUTI_LLM_MAX_MESSAGES", 20).max(3);
        let max_chars = env_usize("SUBHUTI_LLM_MAX_CHARS", 32_000).max(256);
        Self {
            enabled,
            max_messages,
            max_chars,
        }
    }
}

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

/// 裁剪结果说明
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrimOutcome {
    /// 原始消息条数
    pub original: usize,
    /// 裁剪后消息条数
    pub kept: usize,
    /// 被整条省略的原始消息条数（不含新插入的提示）
    pub omitted: usize,
    /// 被截断内容的系统消息条数
    pub truncated: usize,
}

impl TrimOutcome {
    /// 是否发生了任何形式的裁剪
    pub fn changed(&self) -> bool {
        self.omitted > 0 || self.truncated > 0
    }
}

fn char_len(m: &Message) -> usize {
    m.content.chars().count()
}

/// 纯函数：按预算裁剪消息列表。
///
/// 返回裁剪后的列表与结果说明，便于调用方打日志与测试。
pub fn trim_messages(messages: Vec<Message>, cfg: &LimitConfig) -> (Vec<Message>, TrimOutcome) {
    let original = messages.len();
    let unchanged = |msgs: Vec<Message>| {
        (
            msgs,
            TrimOutcome {
                original,
                kept: original,
                omitted: 0,
                truncated: 0,
            },
        )
    };

    if !cfg.enabled || messages.is_empty() {
        return unchanged(messages);
    }

    let total_chars: usize = messages.iter().map(char_len).sum();
    if messages.len() <= cfg.max_messages && total_chars <= cfg.max_chars {
        return unchanged(messages);
    }

    let mut omitted = 0usize;
    let mut truncated = 0usize;

    // 1) 拆分：System 为「指令头」，其余为可裁剪正文
    let mut pinned: Vec<Message> = Vec::new();
    let mut body: Vec<Message> = Vec::new();
    for m in messages {
        if m.role == Role::System {
            pinned.push(m);
        } else {
            body.push(m);
        }
    }

    // 2) 当前输入（最后一条）恒保留且不截断
    let last = body.pop();
    let last_chars = last.as_ref().map(char_len).unwrap_or(0);

    // 预留：当前输入 + 一条裁剪提示
    let mut remaining = cfg
        .max_chars
        .saturating_sub(last_chars)
        .saturating_sub(NOTE_RESERVE_CHARS);

    // 3) 系统消息：从最后一条往前塞；塞不下则截断当前这条并丢弃更早的
    let mut pinned_kept: Vec<Message> = Vec::new();
    while let Some(mut sys) = pinned.pop() {
        let cost = char_len(&sys);
        if cost <= remaining {
            remaining -= cost;
            pinned_kept.push(sys);
        } else if remaining > TRUNCATE_MARKER_CHARS {
            let keep = remaining.saturating_sub(TRUNCATE_MARKER_CHARS);
            let head: String = sys.content.chars().take(keep).collect();
            sys.content = format!("{head}\n…（系统提示已截断）");
            remaining = 0;
            truncated += 1;
            pinned_kept.push(sys);
            break;
        } else {
            // 剩余预算不足以容纳有效内容：整条丢弃，更早的一并丢弃
            omitted += 1;
            break;
        }
    }
    pinned_kept.reverse();

    // 4) 正文按最近优先回填
    let count_budget = cfg
        .max_messages
        .saturating_sub(pinned_kept.len())
        .saturating_sub(if last.is_some() { 1 } else { 0 })
        .saturating_sub(1); // 裁剪提示

    let mut kept_rev: Vec<Message> = Vec::new();
    while let Some(m) = body.pop() {
        if kept_rev.len() >= count_budget {
            body.push(m);
            break;
        }
        let cost = char_len(&m);
        if cost > remaining {
            body.push(m);
            break;
        }
        remaining -= cost;
        kept_rev.push(m);
    }

    // 5) 丢弃窗口开头孤立的 Tool 消息（其配对的 tool_call 多半已被裁掉）
    while kept_rev.last().map(|m| m.role) == Some(Role::Tool) {
        kept_rev.pop();
        omitted += 1;
    }
    kept_rev.reverse();

    // body 中剩下的就是被省略的历史
    omitted += body.len();

    let mut out = pinned_kept;
    if omitted > 0 {
        out.push(Message::system(format!(
            "[上下文已裁剪] 为控制长度，已省略更早的 {omitted} 条对话记录，仅保留最近 {} 条。",
            kept_rev.len() + usize::from(last.is_some())
        )));
    }
    out.extend(kept_rev);
    if let Some(l) = last {
        out.push(l);
    }

    let kept = out.len();
    (
        out,
        TrimOutcome {
            original,
            kept,
            omitted,
            truncated,
        },
    )
}

/// 上下文裁剪包装器
///
/// 用法：
/// ```ignore
/// let real = ZhipuClient::new(cfg);
/// let limited = ContextLimitLLM::from_env(Arc::new(real));
/// ```
pub struct ContextLimitLLM {
    inner: Arc<dyn LLM>,
    cfg: LimitConfig,
}

impl ContextLimitLLM {
    /// 用显式配置包装
    pub fn wrap(inner: Arc<dyn LLM>, cfg: LimitConfig) -> Arc<Self> {
        info!(
            "[ContextLimitLLM] 已启用：enabled={}, max_messages={}, max_chars={}",
            cfg.enabled, cfg.max_messages, cfg.max_chars
        );
        Arc::new(Self { inner, cfg })
    }

    /// 从环境变量读取配置并包装
    pub fn from_env(inner: Arc<dyn LLM>) -> Arc<Self> {
        Self::wrap(inner, LimitConfig::from_env())
    }

    /// 当前配置
    pub fn config_limit(&self) -> LimitConfig {
        self.cfg
    }

    fn apply(&self, messages: Vec<Message>) -> Vec<Message> {
        let (trimmed, outcome) = trim_messages(messages, &self.cfg);
        if outcome.changed() {
            debug!(
                "[ContextLimitLLM] 裁剪上下文：{} → {} 条（省略 {} 条，截断 {} 条系统消息）",
                outcome.original, outcome.kept, outcome.omitted, outcome.truncated
            );
        }
        trimmed
    }
}

#[async_trait]
impl LLM for ContextLimitLLM {
    fn provider(&self) -> LLMProvider {
        self.inner.provider()
    }

    fn config(&self) -> &LLMConfig {
        self.inner.config()
    }

    async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
        self.inner.chat(self.apply(messages)).await
    }

    /// 转发带用量的对话（漏了这一步 token 采集就会在装饰器链上被默认实现截断）
    async fn chat_counted(
        &self,
        messages: Vec<Message>,
    ) -> subhuti_core::Result<(String, Option<u64>)> {
        self.inner.chat_counted(self.apply(messages)).await
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolInfo>,
    ) -> subhuti_core::Result<LLMResponse> {
        self.inner
            .chat_with_tools(self.apply(messages), tools)
            .await
    }

    async fn chat_streaming(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<()> {
        self.inner
            .chat_streaming(self.apply(messages), callback)
            .await
    }

    /// 转发带用量的流式对话（漏了这一步 token 采集就会在装饰器链上被默认实现截断）
    async fn chat_streaming_counted(
        &self,
        messages: Vec<Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> subhuti_core::Result<Option<u64>> {
        self.inner
            .chat_streaming_counted(self.apply(messages), callback)
            .await
    }

    async fn health_check(&self) -> subhuti_core::Result<bool> {
        // 健康检查是轻量 ping，无需裁剪
        self.inner.health_check().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(max_messages: usize, max_chars: usize) -> LimitConfig {
        LimitConfig {
            enabled: true,
            max_messages,
            max_chars,
        }
    }

    fn turn(i: usize) -> Vec<Message> {
        vec![
            Message::assistant(format!("答{i}")),
            Message::user(format!("问{i}")),
        ]
    }

    #[test]
    fn under_budget_is_untouched() {
        let msgs = vec![
            Message::system("你是助手"),
            Message::user("你好"),
            Message::assistant("你好呀"),
        ];
        let (out, oc) = trim_messages(msgs.clone(), &cfg(20, 32_000));
        assert_eq!(out.len(), msgs.len());
        assert!(!oc.changed());
    }

    #[test]
    fn disabled_is_untouched() {
        let msgs: Vec<Message> = (0..100)
            .map(|i| Message::user(format!("很长的消息{i}")))
            .collect();
        let c = LimitConfig {
            enabled: false,
            ..cfg(5, 100)
        };
        let (out, oc) = trim_messages(msgs.clone(), &c);
        assert_eq!(out.len(), msgs.len());
        assert!(!oc.changed());
    }

    #[test]
    fn system_and_last_are_always_kept() {
        let mut msgs = vec![Message::system("系统指令")];
        for i in 0..10 {
            msgs.extend(turn(i));
        }
        msgs.push(Message::user("当前问题"));
        let (out, oc) = trim_messages(msgs, &cfg(5, 100_000));

        assert_eq!(out.first().unwrap().role, Role::System);
        assert_eq!(out.first().unwrap().content, "系统指令");
        assert_eq!(out.last().unwrap().content, "当前问题");
        assert_eq!(out.len(), 5, "消息数不应超过 max_messages");
        assert!(oc.omitted > 0);
    }

    #[test]
    fn keeps_most_recent_window() {
        let mut msgs = vec![Message::system("s")];
        for i in 0..10 {
            msgs.extend(turn(i));
        }
        msgs.push(Message::user("当前问题"));
        let (out, _) = trim_messages(msgs, &cfg(6, 100_000));

        let joined: String = out.iter().map(|m| m.content.clone()).collect();
        assert!(joined.contains("问9"), "最近的对话必须保留: {joined}");
        assert!(!joined.contains("问0"), "最早的对话应被省略: {joined}");
    }

    #[test]
    fn inserts_omit_notice_when_trimmed() {
        let mut msgs = vec![Message::system("s")];
        for i in 0..10 {
            msgs.extend(turn(i));
        }
        msgs.push(Message::user("当前问题"));
        let (out, oc) = trim_messages(msgs, &cfg(4, 100_000));
        assert!(oc.omitted > 0);
        assert!(
            out.iter().any(|m| m.content.contains("上下文已裁剪")),
            "裁剪后应插入提示: {:?}",
            out.iter().map(|m| &m.content).collect::<Vec<_>>()
        );
    }

    #[test]
    fn char_budget_is_respected() {
        let mut msgs = vec![Message::system("s")];
        for _ in 0..50 {
            msgs.push(Message::user("x".repeat(100)));
            msgs.push(Message::assistant("y".repeat(100)));
        }
        msgs.push(Message::user("当前问题"));
        let (out, _) = trim_messages(msgs, &cfg(100, 2_000));

        let total: usize = out.iter().map(char_len).sum();
        assert!(total <= 2_000, "字符预算未生效：total={total}");
    }

    #[test]
    fn oversized_last_message_is_still_kept() {
        let big = "z".repeat(5_000);
        let msgs = vec![
            Message::system("s"),
            Message::user("旧问题"),
            Message::user(big.clone()),
        ];
        let (out, _) = trim_messages(msgs, &cfg(10, 1_000));
        assert_eq!(out.last().unwrap().content, big);
    }

    #[test]
    fn only_system_messages_get_truncated_from_head() {
        let msgs: Vec<Message> = (0..10)
            .map(|i| Message::system(format!("sys{i}:{}", "a".repeat(200))))
            .collect();
        let (out, oc) = trim_messages(msgs, &cfg(100, 1_000));
        assert!(out.len() < 10, "全 system 超预算应被截断");
        assert!(oc.changed());
        assert!(out.last().unwrap().content.starts_with("sys9"));
    }

    #[test]
    fn oversized_system_prompt_is_truncated_not_ignored() {
        // 系统提示本身就远超预算：必须被截断，否则"限制"形同虚设
        let msgs = vec![Message::system("S".repeat(10_000)), Message::user("你好")];
        let (out, oc) = trim_messages(msgs, &cfg(20, 1_000));
        let total: usize = out.iter().map(char_len).sum();
        assert!(oc.truncated >= 1, "应发生系统提示截断");
        assert!(
            total <= 1_000 + NOTE_RESERVE_CHARS,
            "截断后总长仍超预算：{total}"
        );
        assert_eq!(out.last().unwrap().content, "你好");
    }

    #[test]
    fn leading_orphan_tool_messages_dropped() {
        let mut msgs = vec![Message::system("s")];
        for i in 0..8 {
            msgs.push(Message::user(format!("问{i}")));
            msgs.push(Message::assistant(format!("答{i}")));
        }
        msgs.push(Message::user("问X"));
        msgs.push(Message::assistant("答X"));
        msgs.push(Message::tool("工具结果", "call-1"));
        msgs.push(Message::user("当前问题"));

        let (out, _) = trim_messages(msgs, &cfg(4, 100_000));
        let first_body = out.iter().find(|m| m.role != Role::System);
        assert_ne!(first_body.map(|m| m.role), Some(Role::Tool));
    }

    #[test]
    fn outcome_changed_reflects_any_trim() {
        let oc = TrimOutcome {
            original: 10,
            kept: 10,
            omitted: 0,
            truncated: 0,
        };
        assert!(!oc.changed());
        let oc2 = TrimOutcome { truncated: 1, ..oc };
        assert!(oc2.changed());
    }

    /// 记录「实际收到的消息条数」的替身，用于验证裁剪确实发生在内层之前。
    struct CountingLLM {
        config: LLMConfig,
        seen: std::sync::Mutex<usize>,
    }

    impl CountingLLM {
        fn new() -> Self {
            Self {
                config: LLMConfig::default(),
                seen: std::sync::Mutex::new(0),
            }
        }
        fn seen(&self) -> usize {
            *self.seen.lock().unwrap()
        }
    }

    #[async_trait]
    impl LLM for CountingLLM {
        fn provider(&self) -> LLMProvider {
            LLMProvider::Custom
        }
        fn config(&self) -> &LLMConfig {
            &self.config
        }
        async fn chat(&self, messages: Vec<Message>) -> subhuti_core::Result<String> {
            *self.seen.lock().unwrap() = messages.len();
            Ok("raw".to_string())
        }
        async fn chat_counted(
            &self,
            messages: Vec<Message>,
        ) -> subhuti_core::Result<(String, Option<u64>)> {
            *self.seen.lock().unwrap() = messages.len();
            Ok(("raw".to_string(), Some(7)))
        }
        async fn chat_with_tools(
            &self,
            _messages: Vec<Message>,
            _tools: Vec<ToolInfo>,
        ) -> subhuti_core::Result<LLMResponse> {
            unreachable!("本用例不走 tools 路径")
        }
        async fn chat_streaming(
            &self,
            _messages: Vec<Message>,
            _callback: Box<dyn Fn(String) + Send>,
        ) -> subhuti_core::Result<()> {
            unreachable!("本用例不走流式路径")
        }
        async fn chat_streaming_counted(
            &self,
            _messages: Vec<Message>,
            callback: Box<dyn Fn(String) + Send>,
        ) -> subhuti_core::Result<Option<u64>> {
            // 模拟：转发 callback 一次 + 返回真实用量
            callback("delta".to_string());
            Ok(Some(42))
        }
        async fn health_check(&self) -> subhuti_core::Result<bool> {
            Ok(true)
        }
    }

    /// 关键回归：装饰器必须**转发** `chat_counted`。
    ///
    /// 若不转发，trait 默认实现会退化成 `chat()` 并返回 `None`，
    /// token 用量就在装饰器链上被静默吞掉——这正是「成本恒为 0」的成因类别。
    #[tokio::test]
    async fn forwards_chat_counted_and_still_trims() {
        let inner = Arc::new(CountingLLM::new());
        let llm = ContextLimitLLM::wrap(inner.clone(), cfg(3, 32_000));

        let mut msgs = vec![Message::system("你是助手")];
        for i in 0..10 {
            msgs.push(Message::user(format!("问{i}")));
        }
        let (out, tokens) = llm.chat_counted(msgs).await.unwrap();

        assert_eq!(out, "raw");
        assert_eq!(tokens, Some(7), "用量必须被透传，不能被装饰器吞掉");
        assert!(
            inner.seen() < 11,
            "裁剪必须生效：内层实收 {} 条，期望少于 11 条",
            inner.seen()
        );
    }
}
