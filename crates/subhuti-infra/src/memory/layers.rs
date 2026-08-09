//! # Memory Layers - 四层记忆栈
//!
//! 实现 token 预算控制的上下文加载，只加载必要内容以降低 token 消耗：
//!
//! - **Layer 0: 身份层**（~100 tokens，始终加载）— 我是谁、核心身份
//! - **Layer 1: 核心故事**（~500-800 tokens，始终加载）— 最重要的记忆摘要
//! - **Layer 2: 按需加载**（~200-500 tokens/主题，话题触发时加载）— 主题相关记忆
//! - **Layer 3: 深度搜索**（无限制，完整向量检索）— 语义搜索全部记忆

use super::MemoryItem;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// 记忆分层配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLayerConfig {
    /// L0 身份层最大 token 数（默认 100）
    pub l0_max_tokens: usize,
    /// L1 核心故事最大 token 数（默认 800）
    pub l1_max_tokens: usize,
    /// L2 每个主题最大 token 数（默认 500）
    pub l2_max_tokens_per_topic: usize,
    /// 是否启用 L2 按需加载（默认 true）
    pub l2_enabled: bool,
    /// 是否启用 L3 深度搜索（默认 true）
    pub l3_enabled: bool,
}

impl Default for MemoryLayerConfig {
    fn default() -> Self {
        Self {
            l0_max_tokens: 100,
            l1_max_tokens: 800,
            l2_max_tokens_per_topic: 500,
            l2_enabled: true,
            l3_enabled: true,
        }
    }
}

/// 单层渲染输出
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerOutput {
    /// 层级：0, 1, 2, 3
    pub layer: u8,
    /// 渲染后的内容
    pub content: String,
    /// token 估算值
    pub token_estimate: usize,
    /// 来源描述（标识该内容来源）
    pub source: String,
}

/// 四层记忆栈 — 管理分层上下文加载
///
/// 核心思想：只加载需要的内容，严格控制 token 使用量。
/// L0/L1 始终加载，L2 按主题触发，L3 由外部向量检索服务处理。
#[derive(Debug, Clone)]
pub struct MemoryStack {
    /// 分层配置
    pub config: MemoryLayerConfig,
    /// L0 身份内容
    pub identity: String,
}

impl MemoryStack {
    /// 创建记忆栈
    pub fn new(config: MemoryLayerConfig) -> Self {
        Self {
            config,
            identity: String::new(),
        }
    }

    /// 设置身份内容（L0）
    pub fn set_identity(&mut self, identity: String) {
        debug!("MemoryStack: set_identity 身份内容长度={}", identity.len());
        self.identity = identity;
    }

    /// 判断字符是否为 CJK 字符
    ///
    /// 覆盖范围：
    /// - `\u4e00-\u9fff`: CJK 统一表意文字（中文汉字）
    /// - `\u3000-\u30ff`: CJK 符号与标点、平假名、片假名
    fn is_cjk(ch: char) -> bool {
        matches!(ch, '\u{4e00}'..='\u{9fff}' | '\u{3000}'..='\u{30ff}')
    }

    /// 估算文本的 token 数
    ///
    /// 启发式规则（适合中英混合文本）：
    /// - CJK 字符：每个约 2 个 token
    /// - ASCII 单词序列：每个单词约 1 个 token（标点跟随单词）
    /// - 空白字符：不计 token
    pub fn estimate_tokens(text: &str) -> usize {
        debug!("MemoryStack: estimate_tokens 开始，文本长度={}", text.len());
        let mut tokens = 0usize;
        let mut in_word = false;

        for ch in text.chars() {
            if Self::is_cjk(ch) {
                tokens += 2;
                in_word = false;
            } else if ch.is_whitespace() {
                in_word = false;
            } else {
                if !in_word {
                    tokens += 1;
                    in_word = true;
                }
            }
        }

        debug!(
            "MemoryStack: estimate_tokens 完成，估算 token 数={}",
            tokens
        );
        tokens
    }

    /// 截断文本以适应 token 预算
    ///
    /// 在字符边界安全截断，确保结果 token 数不超过 `max_tokens`。
    /// 若整体已在预算内则原样返回。
    pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
        debug!(
            "MemoryStack: truncate_to_tokens 开始，文本长度={}, max_tokens={}",
            text.len(),
            max_tokens
        );
        if max_tokens == 0 || text.is_empty() {
            debug!("MemoryStack: truncate_to_tokens 空输入或零预算");
            return String::new();
        }

        let estimated = Self::estimate_tokens(text);
        if estimated <= max_tokens {
            debug!(
                "MemoryStack: truncate_to_tokens 在预算内 estimated={}, max_tokens={}",
                estimated, max_tokens
            );
            return text.to_string();
        }

        let mut tokens = 0usize;
        let mut in_word = false;
        let mut end_byte = 0usize;

        for (byte_idx, ch) in text.char_indices() {
            let cost = if Self::is_cjk(ch) {
                2
            } else if ch.is_whitespace() || in_word {
                0
            } else {
                1
            };

            if tokens + cost > max_tokens {
                break;
            }

            tokens += cost;
            end_byte = byte_idx + ch.len_utf8();

            in_word = !(Self::is_cjk(ch) || ch.is_whitespace());
        }

        let result = text[..end_byte].trim_end().to_string();
        debug!(
            "MemoryStack: truncate_to_tokens 完成，结果长度={}, token_estimate={}",
            result.len(),
            Self::estimate_tokens(&result)
        );
        result
    }

    /// 渲染 L0 身份层
    ///
    /// 始终加载，截断到 `l0_max_tokens` 预算内。
    pub fn render_l0(&self) -> LayerOutput {
        info!(
            "MemoryStack: render_l0 开始，身份内容长度={}, l0_max_tokens={}",
            self.identity.len(),
            self.config.l0_max_tokens
        );
        let content = Self::truncate_to_tokens(&self.identity, self.config.l0_max_tokens);
        let token_estimate = Self::estimate_tokens(&content);
        let output = LayerOutput {
            layer: 0,
            token_estimate,
            content: content.clone(),
            source: "identity".to_string(),
        };
        info!(
            "MemoryStack: render_l0 完成，token_estimate={}, content_len={}",
            token_estimate,
            output.content.len()
        );
        output
    }

    /// 渲染 L1 核心故事层
    ///
    /// 按重要性（近因性 + 元数据加成）排序，在 token 预算内选择最重要的记忆。
    /// - 近因性：越新分数越高，按小时衰减
    /// - 元数据加成：有元数据的记忆额外加分
    pub fn render_l1(&self, memories: &[MemoryItem]) -> LayerOutput {
        info!(
            "MemoryStack: render_l1 开始，记忆数={}, l1_max_tokens={}",
            memories.len(),
            self.config.l1_max_tokens
        );
        let now = Utc::now();

        let mut scored: Vec<(f64, &MemoryItem)> = memories
            .iter()
            .map(|m| {
                let age_hours = now.signed_duration_since(m.created_at).num_hours().max(0) as f64;
                let recency = 1.0 / (1.0 + age_hours);
                let metadata_bonus = if m.metadata.is_empty() { 0.0 } else { 0.5 };
                (recency + metadata_bonus, m)
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut content = String::new();
        let mut tokens_used = 0usize;
        let mut selected_count = 0usize;

        for (score, item) in scored {
            let item_tokens = Self::estimate_tokens(&item.content);
            if tokens_used + item_tokens > self.config.l1_max_tokens {
                debug!(
                    "MemoryStack: render_l1 跳过超预算记忆 score={:.4}, content_len={}, tokens={}",
                    score,
                    item.content.len(),
                    item_tokens
                );
                continue;
            }
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&item.content);
            tokens_used += item_tokens;
            selected_count += 1;
            debug!(
                "MemoryStack: render_l1 选中记忆 score={:.4}, content_len={}, tokens={}",
                score,
                item.content.len(),
                item_tokens
            );
        }

        let output = LayerOutput {
            layer: 1,
            content: content.clone(),
            token_estimate: tokens_used,
            source: "essential_story".to_string(),
        };
        info!(
            "MemoryStack: render_l1 完成，选中 {} 条，token_estimate={}, content_len={}",
            selected_count,
            tokens_used,
            output.content.len()
        );
        output
    }

    /// 渲染 L2 按需加载层
    ///
    /// 按主题关键词过滤记忆（匹配内容或元数据值），在每主题 token 预算内加载。
    pub fn render_l2(&self, memories: &[MemoryItem], topic: &str) -> LayerOutput {
        info!(
            "MemoryStack: render_l2 开始，记忆数={}, topic={}, l2_max_tokens_per_topic={}",
            memories.len(),
            topic,
            self.config.l2_max_tokens_per_topic
        );
        let topic_lower = topic.to_lowercase();

        let filtered: Vec<&MemoryItem> = memories
            .iter()
            .filter(|m| {
                m.content.to_lowercase().contains(&topic_lower)
                    || m.metadata
                        .values()
                        .any(|v| v.to_lowercase().contains(&topic_lower))
            })
            .collect();

        debug!(
            "MemoryStack: render_l2 主题过滤后保留 {} 条",
            filtered.len()
        );

        let mut content = String::new();
        let mut tokens_used = 0usize;
        let mut selected_count = 0usize;

        for item in filtered {
            let item_tokens = Self::estimate_tokens(&item.content);
            if tokens_used + item_tokens > self.config.l2_max_tokens_per_topic {
                debug!(
                    "MemoryStack: render_l2 跳过超预算记忆 content_len={}, tokens={}",
                    item.content.len(),
                    item_tokens
                );
                continue;
            }
            if !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&item.content);
            tokens_used += item_tokens;
            selected_count += 1;
        }

        let output = LayerOutput {
            layer: 2,
            content: content.clone(),
            token_estimate: tokens_used,
            source: format!("on_demand:{}", topic),
        };
        info!(
            "MemoryStack: render_l2 完成，选中 {} 条，token_estimate={}, content_len={}",
            selected_count,
            tokens_used,
            output.content.len()
        );
        output
    }

    /// 渲染完整上下文（L0 + L1 + 可选 L2）
    ///
    /// 尊重各层 token 预算。L3 深度搜索不在此范围内（由外部向量检索服务处理）。
    ///
    /// 参数 `identity` 允许调用时覆盖身份内容，不修改栈内 `self.identity`。
    pub fn render_context(
        &self,
        identity: &str,
        memories: &[MemoryItem],
        topic: Option<&str>,
    ) -> String {
        info!(
            "MemoryStack: render_context 开始，identity_len={}, memories_count={}, topic={:?}",
            identity.len(),
            memories.len(),
            topic
        );
        let mut context = String::new();

        let l0_content = Self::truncate_to_tokens(identity, self.config.l0_max_tokens);
        if !l0_content.is_empty() {
            debug!(
                "MemoryStack: render_context L0 内容长度={}",
                l0_content.len()
            );
            context.push_str(&l0_content);
            context.push_str("\n\n");
        }

        let l1 = self.render_l1(memories);
        if !l1.content.is_empty() {
            debug!(
                "MemoryStack: render_context L1 内容长度={}, tokens={}",
                l1.content.len(),
                l1.token_estimate
            );
            context.push_str(&l1.content);
            context.push_str("\n\n");
        }

        if self.config.l2_enabled {
            if let Some(topic) = topic {
                if !topic.is_empty() {
                    let l2 = self.render_l2(memories, topic);
                    if !l2.content.is_empty() {
                        debug!(
                            "MemoryStack: render_context L2 内容长度={}, tokens={}",
                            l2.content.len(),
                            l2.token_estimate
                        );
                        context.push_str(&l2.content);
                    }
                }
            }
        } else {
            debug!("MemoryStack: render_context L2 已禁用");
        }

        let result = context.trim_end().to_string();
        info!(
            "MemoryStack: render_context 完成，总内容长度={}, token_estimate={}",
            result.len(),
            Self::estimate_tokens(&result)
        );
        result
    }
}

impl Default for MemoryStack {
    fn default() -> Self {
        Self::new(MemoryLayerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryLayer;
    use chrono::Duration;

    /// 测试辅助：创建指定时间偏移（小时前）的记忆项
    fn make_memory(content: &str, hours_ago: i64) -> MemoryItem {
        let mut item = MemoryItem::new(
            content.to_string(),
            MemoryLayer::ShortTerm,
            Some("test_session".to_string()),
        );
        item.created_at = Utc::now() - Duration::hours(hours_ago);
        item
    }

    /// 测试辅助：创建带元数据的记忆项
    fn make_memory_with_meta(content: &str, hours_ago: i64, key: &str, value: &str) -> MemoryItem {
        let mut item = make_memory(content, hours_ago);
        item.metadata.insert(key.to_string(), value.to_string());
        item
    }

    // ── Token 估算测试 ──────────────────────────────────

    #[test]
    fn test_estimate_tokens_english() {
        // 2 个单词 = 2 tokens
        assert_eq!(MemoryStack::estimate_tokens("Hello world"), 2);
        // 4 个单词 = 4 tokens
        assert_eq!(MemoryStack::estimate_tokens("The quick brown fox"), 4);
        // 空字符串 = 0 tokens
        assert_eq!(MemoryStack::estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_chinese() {
        // 4 个 CJK 字符 × 2 = 8 tokens
        assert_eq!(MemoryStack::estimate_tokens("你好世界"), 8);
        // 2 个 CJK 字符 × 2 = 4 tokens
        assert_eq!(MemoryStack::estimate_tokens("你好"), 4);
        // 单个 CJK 字符 = 2 tokens
        assert_eq!(MemoryStack::estimate_tokens("我"), 2);
    }

    #[test]
    fn test_estimate_tokens_mixed() {
        // 1 单词 + 2 CJK = 1 + 4 = 5 tokens
        assert_eq!(MemoryStack::estimate_tokens("Hello 世界"), 5);
        // 2 单词 + 2 CJK = 2 + 4 = 6 tokens
        assert_eq!(MemoryStack::estimate_tokens("Hello world 你好"), 6);
    }

    #[test]
    fn test_estimate_tokens_punctuation() {
        // 标点跟随单词，"Hello," = 1 token, "world!" = 1 token
        assert_eq!(MemoryStack::estimate_tokens("Hello, world!"), 2);
        // 纯标点序列也算 1 token
        assert_eq!(MemoryStack::estimate_tokens("---"), 1);
    }

    // ── 截断测试 ────────────────────────────────────────

    #[test]
    fn test_truncate_english() {
        let text = "The quick brown fox jumps";
        let truncated = MemoryStack::truncate_to_tokens(text, 2);
        assert_eq!(MemoryStack::estimate_tokens(&truncated), 2);
        assert_eq!(truncated, "The quick");
    }

    #[test]
    fn test_truncate_chinese() {
        let text = "你好世界你好世界"; // 16 tokens
        let truncated = MemoryStack::truncate_to_tokens(text, 4);
        assert_eq!(MemoryStack::estimate_tokens(&truncated), 4);
        assert_eq!(truncated, "你好");
    }

    #[test]
    fn test_truncate_within_budget() {
        // 整体在预算内，不截断
        let text = "Hello";
        assert_eq!(MemoryStack::truncate_to_tokens(text, 10), "Hello");
    }

    #[test]
    fn test_truncate_zero_budget() {
        assert_eq!(MemoryStack::truncate_to_tokens("Hello", 0), "");
        assert_eq!(MemoryStack::truncate_to_tokens("", 100), "");
    }

    #[test]
    fn test_truncate_mixed() {
        // "Hello 世界" = 1 + 4 = 5 tokens，截断到 3
        let text = "Hello 世界";
        let truncated = MemoryStack::truncate_to_tokens(text, 3);
        // "Hello" = 1 token, 之后 "世" = 2 tokens → 共 3
        assert!(MemoryStack::estimate_tokens(&truncated) <= 3);
        assert!(truncated.starts_with("Hello"));
    }

    // ── L0 渲染测试 ─────────────────────────────────────

    #[test]
    fn test_render_l0() {
        let mut stack = MemoryStack::default();
        stack.set_identity("我是AI助手".to_string());
        let l0 = stack.render_l0();
        assert_eq!(l0.layer, 0);
        assert_eq!(l0.content, "我是AI助手");
        assert!(l0.token_estimate <= stack.config.l0_max_tokens);
        assert_eq!(l0.source, "identity");
    }

    #[test]
    fn test_render_l0_truncation() {
        let mut stack = MemoryStack::default();
        // 默认预算 100 tokens = 最多 50 个 CJK 字符
        let long_identity = "我".repeat(100); // 200 tokens，超过预算
        stack.set_identity(long_identity);
        let l0 = stack.render_l0();
        assert!(l0.token_estimate <= stack.config.l0_max_tokens);
        assert_eq!(l0.token_estimate, 100); // 恰好截断到 50 个字符 = 100 tokens
    }

    // ── L1 渲染测试 ─────────────────────────────────────

    #[test]
    fn test_render_l1_selects_most_recent() {
        let stack = MemoryStack::default();
        let memories = vec![
            make_memory("旧记忆", 10),
            make_memory("新记忆", 0),
            make_memory("中等记忆", 5),
        ];
        let l1 = stack.render_l1(&memories);
        // 最近的记忆应该排在前面
        assert!(l1.content.starts_with("新记忆"));
        assert_eq!(l1.layer, 1);
        assert_eq!(l1.source, "essential_story");
    }

    #[test]
    fn test_render_l1_respects_budget() {
        let config = MemoryLayerConfig {
            l1_max_tokens: 4, // 只能放 2 个 CJK 字符
            ..Default::default()
        };
        let stack = MemoryStack::new(config);
        let memories = vec![
            make_memory("你好世界", 0), // 8 tokens，放不下
            make_memory("测试", 1),     // 4 tokens，能放下
        ];
        let l1 = stack.render_l1(&memories);
        // 预算 4 tokens，跳过 "你好世界"，放入 "测试"
        assert!(l1.token_estimate <= 4);
        assert!(l1.content.contains("测试"));
        assert!(!l1.content.contains("你好世界"));
    }

    #[test]
    fn test_render_l1_metadata_priority() {
        let stack = MemoryStack::default();
        // 相同近因性下，有元数据的记忆优先
        let memories = vec![
            make_memory("无元数据记忆", 0),
            make_memory_with_meta("有元数据记忆", 0, "category", "important"),
        ];
        let l1 = stack.render_l1(&memories);
        // 有元数据的记忆应该排在前面
        assert!(l1.content.starts_with("有元数据记忆"));
    }

    #[test]
    fn test_render_l1_empty() {
        let stack = MemoryStack::default();
        let l1 = stack.render_l1(&[]);
        assert!(l1.content.is_empty());
        assert_eq!(l1.token_estimate, 0);
    }

    // ── L2 渲染测试 ─────────────────────────────────────

    #[test]
    fn test_render_l2_topic_filter() {
        let stack = MemoryStack::default();
        let memories = vec![
            make_memory("今天天气很好", 0),
            make_memory("我在写代码", 1),
            make_memory("天气预报说会下雨", 2),
        ];
        let l2 = stack.render_l2(&memories, "天气");
        assert_eq!(l2.layer, 2);
        // 应该只包含含 "天气" 的记忆
        assert!(l2.content.contains("今天天气很好"));
        assert!(l2.content.contains("天气预报说会下雨"));
        assert!(!l2.content.contains("我在写代码"));
        assert!(l2.source.contains("天气"));
    }

    #[test]
    fn test_render_l2_metadata_match() {
        let stack = MemoryStack::default();
        let memories = vec![make_memory_with_meta("一段内容", 0, "topic", "编程")];
        let l2 = stack.render_l2(&memories, "编程");
        // 元数据值匹配也算命中
        assert!(l2.content.contains("一段内容"));
    }

    #[test]
    fn test_render_l2_no_match() {
        let stack = MemoryStack::default();
        let memories = vec![make_memory("你好世界", 0)];
        let l2 = stack.render_l2(&memories, "不存在的主题");
        assert!(l2.content.is_empty());
        assert_eq!(l2.token_estimate, 0);
    }

    #[test]
    fn test_render_l2_respects_budget() {
        let config = MemoryLayerConfig {
            l2_max_tokens_per_topic: 4,
            ..Default::default()
        };
        let stack = MemoryStack::new(config);
        let memories = vec![
            make_memory("天气很好啊", 0), // 5 CJK = 10 tokens，超预算
        ];
        let l2 = stack.render_l2(&memories, "天气");
        assert!(l2.token_estimate <= 4);
        assert!(l2.content.is_empty()); // 单条就超预算，跳过
    }

    // ── 上下文渲染测试 ──────────────────────────────────

    #[test]
    fn test_render_context_no_topic() {
        let mut stack = MemoryStack::default();
        stack.set_identity("我是助手".to_string());
        let memories = vec![
            make_memory("用户喜欢编程", 0),
            make_memory("用户在学Rust", 1),
        ];

        let ctx = stack.render_context("我是助手", &memories, None);
        assert!(ctx.contains("我是助手"));
        assert!(ctx.contains("用户喜欢编程"));
    }

    #[test]
    fn test_render_context_with_topic() {
        let stack = MemoryStack::default();
        let memories = vec![make_memory("编程很有趣", 0), make_memory("今天吃饭了", 1)];

        let ctx = stack.render_context("助手", &memories, Some("编程"));
        assert!(ctx.contains("助手"));
        assert!(ctx.contains("编程"));
    }

    #[test]
    fn test_render_context_l2_disabled() {
        let config = MemoryLayerConfig {
            l2_enabled: false,
            ..Default::default()
        };
        let stack = MemoryStack::new(config);
        let memories = vec![make_memory("编程很有趣", 0)];

        let ctx = stack.render_context("助手", &memories, Some("编程"));
        // L2 禁用，仍应包含 L0
        assert!(ctx.contains("助手"));
    }

    #[test]
    fn test_render_context_identity_override() {
        let mut stack = MemoryStack::default();
        stack.set_identity("栈内身份".to_string());

        // render_context 传入的 identity 覆盖栈内身份
        let ctx = stack.render_context("外部身份", &[], None);
        assert!(ctx.contains("外部身份"));
        assert!(!ctx.contains("栈内身份"));
    }
}
