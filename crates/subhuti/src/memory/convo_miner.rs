//! # Convo Miner - 对话挖掘器
//!
//! 增强版对话归档挖掘：在归档到长期记忆时，
//! 从对话历史中提取结构化信息（实体、关键词、摘要），
//! 而非仅存储原始 "User: xxx\nAssistant: xxx" 文本。
//!
//! ## 组成
//!
//! - [`ConvoExchange`][]: 单次用户-助手对话交换
//! - [`MinedMemory`][]: 挖掘后的结构化记忆
//! - [`ConvoMiner`][]: 对话挖掘引擎

use super::entities::EntityExtractor;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use tracing::{debug, info};

// ── 数据模型 ──────────────────────────────────────────

/// 单次用户-助手对话交换
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvoExchange {
    /// 用户消息
    pub user_message: String,
    /// 助手消息
    pub assistant_message: String,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 会话 ID
    pub session_id: String,
}

/// 挖掘后的记忆
///
/// 由 [`ConvoMiner::mine_exchange`] 产出，包含格式化内容、摘要、
/// 实体列表、关键词列表与元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinedMemory {
    /// 格式化后的记忆内容
    pub content: String,
    /// 简要摘要
    pub summary: String,
    /// 抽取的实体名称列表
    pub entities: Vec<String>,
    /// 关键词列表
    pub keywords: Vec<String>,
    /// 元数据
    pub metadata: HashMap<String, String>,
    /// 时间戳
    pub timestamp: DateTime<Utc>,
    /// 会话 ID
    pub session_id: String,
}

// ── 停用词表 ──────────────────────────────────────────
//
// 使用 OnceLock 延迟构建，进程内只初始化一次。

/// 英文停用词集合
fn english_stop_words() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "the", "is", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of",
            "with", "by", "as", "it", "this", "that", "these", "those", "be", "are", "was", "were",
            "been", "being", "have", "has", "had", "do", "does", "did", "will", "would", "could",
            "should", "may", "might", "can", "shall", "i", "you", "he", "she", "we", "they",
            "them", "his", "her", "its", "our", "your", "their", "me", "him", "us", "my", "from",
            "into", "if", "then", "so", "not", "no",
        ]
        .into_iter()
        .collect()
    })
}

/// 中文停用词集合
fn chinese_stop_words() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "的", "了", "是", "在", "我", "你", "他", "她", "它", "们", "个", "这", "那", "都",
            "也", "不", "没", "有", "就", "会", "要", "说", "做",
        ]
        .into_iter()
        .collect()
    })
}

// ── 对话挖掘器 ────────────────────────────────────────

/// 对话挖掘引擎
///
/// 从对话交换中提取结构化记忆：实体、关键词、格式化内容。
/// 实体抽取复用 [`EntityExtractor`]（纯规则、语言中立）。
#[derive(Debug, Clone)]
pub struct ConvoMiner {
    // EntityExtractor::extract 为关联函数（无 self），
    // 此字段保留以符合接口契约与未来扩展，当前通过 EntityExtractor::extract 调用。
    #[allow(dead_code)]
    entity_extractor: EntityExtractor,
}

impl ConvoMiner {
    /// 创建新的挖掘器
    pub fn new() -> Self {
        Self {
            entity_extractor: EntityExtractor::new(),
        }
    }

    /// 挖掘单次对话交换
    ///
    /// 处理流程：
    /// 1. 分别从用户消息与助手消息中抽取实体并合并去重
    /// 2. 合并两段文本提取关键词
    /// 3. 格式化结构化记忆内容
    /// 4. 生成简要摘要与元数据
    pub fn mine_exchange(&self, exchange: &ConvoExchange) -> MinedMemory {
        info!(
            "ConvoMiner: mine_exchange 开始，session_id={}",
            exchange.session_id
        );

        let user_entities = EntityExtractor::extract(&exchange.user_message);
        let assistant_entities = EntityExtractor::extract(&exchange.assistant_message);
        debug!(
            "ConvoMiner: mine_exchange 抽取实体 user={}, assistant={}",
            user_entities.len(),
            assistant_entities.len()
        );

        let mut entities: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for e in user_entities.iter().chain(assistant_entities.iter()) {
            let key = e.to_lowercase();
            if seen.insert(key) {
                entities.push(e.clone());
            }
        }

        let combined = format!("{} {}", exchange.user_message, exchange.assistant_message);
        let keywords = Self::extract_keywords(&combined);
        debug!("ConvoMiner: mine_exchange 提取关键词数={}", keywords.len());

        let content = Self::format_memory(exchange, &entities, &keywords);
        let summary = Self::make_summary(&exchange.user_message, &exchange.assistant_message);

        let mut metadata = HashMap::new();
        metadata.insert("entity_count".to_string(), entities.len().to_string());
        metadata.insert("keyword_count".to_string(), keywords.len().to_string());

        let result = MinedMemory {
            content: content.clone(),
            summary: summary.clone(),
            entities: entities.clone(),
            keywords: keywords.clone(),
            metadata,
            timestamp: exchange.timestamp,
            session_id: exchange.session_id.clone(),
        };

        info!(
            "ConvoMiner: mine_exchange 完成，entities={}, keywords={}, content_len={}",
            entities.len(),
            keywords.len(),
            content.len()
        );
        result
    }

    /// 挖掘完整会话
    ///
    /// 对每个对话交换独立挖掘，返回结构化记忆列表（保持输入顺序）。
    pub fn mine_session(&self, exchanges: &[ConvoExchange]) -> Vec<MinedMemory> {
        info!(
            "ConvoMiner: mine_session 开始，对话交换数={}",
            exchanges.len()
        );
        let results: Vec<MinedMemory> = exchanges
            .iter()
            .enumerate()
            .map(|(i, exchange)| {
                debug!("ConvoMiner: mine_session 处理第 {} 个交换", i + 1);
                self.mine_exchange(exchange)
            })
            .collect();
        info!(
            "ConvoMiner: mine_session 完成，生成 {} 条挖掘记忆",
            results.len()
        );
        results
    }

    /// 提取关键词
    ///
    /// 处理流程：
    /// 1. 按非字母数字字符分词（unicode 感知）
    /// 2. 小写归一化
    /// 3. 过滤英文停用词与中文停用词
    /// 4. 过滤长度小于 2 的 token
    /// 5. 统计词频，按频率降序排序（相同频率按字典序升序）
    /// 6. 取前 10 个
    pub fn extract_keywords(text: &str) -> Vec<String> {
        debug!("ConvoMiner: extract_keywords 开始，文本长度={}", text.len());
        const MIN_LEN: usize = 2;
        const TOP_N: usize = 10;

        let en_stop = english_stop_words();
        let zh_stop = chinese_stop_words();

        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut current = String::new();
        for ch in text.chars() {
            if ch.is_alphanumeric() {
                current.push(ch);
            } else if !current.is_empty() {
                Self::count_token(&mut counts, &current, en_stop, zh_stop, MIN_LEN);
                current.clear();
            }
        }
        if !current.is_empty() {
            Self::count_token(&mut counts, &current, en_stop, zh_stop, MIN_LEN);
        }

        debug!(
            "ConvoMiner: extract_keywords 词频统计完成，候选词数={}",
            counts.len()
        );

        let mut entries: Vec<(String, usize)> = counts.into_iter().collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let result: Vec<String> = entries.into_iter().take(TOP_N).map(|(k, _)| k).collect();

        debug!(
            "ConvoMiner: extract_keywords 完成，关键词数={}",
            result.len()
        );
        result
    }

    /// 统计单个 token：长度过滤、停用词过滤、小写归一化
    fn count_token(
        counts: &mut HashMap<String, usize>,
        token: &str,
        en_stop: &HashSet<&str>,
        zh_stop: &HashSet<&str>,
        min_len: usize,
    ) {
        // 长度过滤（按字符数计）
        if token.chars().count() < min_len {
            return;
        }
        // 小写归一化
        let lower = token.to_lowercase();
        // 停用词过滤
        if en_stop.contains(lower.as_str()) || zh_stop.contains(lower.as_str()) {
            return;
        }
        *counts.entry(lower).or_insert(0) += 1;
    }

    /// 格式化结构化记忆内容
    ///
    /// 输出格式：
    /// ```text
    /// [Session: {session_id}] [{timestamp}]
    /// User: {user_message}
    /// Assistant: {assistant_message}
    /// Entities: {entities joined by ", "}
    /// Keywords: {keywords joined by ", "}
    /// ```
    pub fn format_memory(
        exchange: &ConvoExchange,
        entities: &[String],
        keywords: &[String],
    ) -> String {
        debug!(
            "ConvoMiner: format_memory 开始，entities={}, keywords={}",
            entities.len(),
            keywords.len()
        );
        let entities_str = entities.join(", ");
        let keywords_str = keywords.join(", ");
        let result = format!(
            "[Session: {}] [{}]\nUser: {}\nAssistant: {}\nEntities: {}\nKeywords: {}",
            exchange.session_id,
            exchange.timestamp,
            exchange.user_message,
            exchange.assistant_message,
            entities_str,
            keywords_str
        );
        debug!("ConvoMiner: format_memory 完成，长度={}", result.len());
        result
    }

    /// 生成简要摘要
    ///
    /// 取用户消息首个非空行，截断至 80 字符；
    /// 若用户消息为空则回退到助手消息首行。
    fn make_summary(user_message: &str, assistant_message: &str) -> String {
        debug!(
            "ConvoMiner: make_summary 开始，user_len={}, assistant_len={}",
            user_message.len(),
            assistant_message.len()
        );
        const MAX_LEN: usize = 80;
        let source = user_message
            .lines()
            .find(|l| !l.trim().is_empty())
            .or_else(|| assistant_message.lines().find(|l| !l.trim().is_empty()))
            .unwrap_or("")
            .trim();
        let result = if source.chars().count() > MAX_LEN {
            let truncated: String = source.chars().take(MAX_LEN).collect();
            format!("{}...", truncated)
        } else {
            source.to_string()
        };
        debug!("ConvoMiner: make_summary 完成，摘要长度={}", result.len());
        result
    }
}

impl Default for ConvoMiner {
    fn default() -> Self {
        Self::new()
    }
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── 关键词提取测试 ────────────────────────────────

    #[test]
    fn test_extract_keywords_english() {
        let text = "the rust programming language is great and rust is fast";
        let keywords = ConvoMiner::extract_keywords(text);
        // 停用词应被过滤
        assert!(!keywords.contains(&"the".to_string()));
        assert!(!keywords.contains(&"is".to_string()));
        assert!(!keywords.contains(&"and".to_string()));
        // rust 出现两次，应排首位
        assert_eq!(keywords.first(), Some(&"rust".to_string()));
        assert!(keywords.contains(&"programming".to_string()));
        assert!(keywords.contains(&"language".to_string()));
        assert!(keywords.contains(&"great".to_string()));
        assert!(keywords.contains(&"fast".to_string()));
    }

    #[test]
    fn test_extract_keywords_chinese() {
        let text = "学习 编程 是 有趣 的 编程 语言";
        let keywords = ConvoMiner::extract_keywords(text);
        // 中文停用词应被过滤
        assert!(!keywords.contains(&"是".to_string()));
        assert!(!keywords.contains(&"的".to_string()));
        // 编程 出现两次，应排首位
        assert_eq!(keywords.first(), Some(&"编程".to_string()));
        assert!(keywords.contains(&"学习".to_string()));
        assert!(keywords.contains(&"语言".to_string()));
        assert!(keywords.contains(&"有趣".to_string()));
    }

    #[test]
    fn test_extract_keywords_mixed() {
        // 中英混合，Rust 与 rust 应归一化为同一词
        let text = "使用 Rust 开发 编程 是 有趣 rust";
        let keywords = ConvoMiner::extract_keywords(text);
        // rust 出现两次（大小写归一），应排首位
        assert_eq!(keywords.first(), Some(&"rust".to_string()));
        assert!(keywords.contains(&"编程".to_string()));
        assert!(keywords.contains(&"使用".to_string()));
        assert!(keywords.contains(&"开发".to_string()));
        // 停用词应被过滤
        assert!(!keywords.contains(&"是".to_string()));
    }

    #[test]
    fn test_extract_keywords_top_10_limit() {
        // 生成 15 个不同词，词频递增，应截断至 10
        let mut text = String::new();
        for i in 0..15 {
            for _ in 0..(i + 1) {
                text.push_str(&format!(" word{}", i));
            }
        }
        let keywords = ConvoMiner::extract_keywords(&text);
        assert_eq!(keywords.len(), 10);
        // 词频最高的 word14 应排首位
        assert_eq!(keywords.first(), Some(&"word14".to_string()));
    }

    #[test]
    fn test_extract_keywords_min_length() {
        // 单字符与停用词应被过滤
        let keywords = ConvoMiner::extract_keywords("a I 是 的 ok");
        assert!(keywords.contains(&"ok".to_string()));
        assert!(!keywords.contains(&"a".to_string()));
        assert!(!keywords.contains(&"i".to_string()));
        assert!(!keywords.contains(&"是".to_string()));
        assert!(!keywords.contains(&"的".to_string()));
    }

    #[test]
    fn test_extract_keywords_empty() {
        let keywords = ConvoMiner::extract_keywords("");
        assert!(keywords.is_empty());
    }

    #[test]
    fn test_extract_keywords_case_insensitive_merge() {
        // Rust / RUST / rust 应合并计数
        let keywords = ConvoMiner::extract_keywords("Rust RUST rust other");
        assert_eq!(keywords.first(), Some(&"rust".to_string()));
        assert!(keywords.contains(&"other".to_string()));
    }

    // ── 实体抽取集成测试 ──────────────────────────────

    #[test]
    fn test_mine_exchange_entities() {
        let miner = ConvoMiner::new();
        let exchange = ConvoExchange {
            user_message: "如何使用 `ChromaBackend` 存储？".to_string(),
            assistant_message: "调用 `store.add(doc)` 即可，参见 https://docs.example.com"
                .to_string(),
            timestamp: Utc::now(),
            session_id: "sess_1".to_string(),
        };
        let mined = miner.mine_exchange(&exchange);
        // 实体应来自用户与助手两侧
        assert!(mined.entities.contains(&"ChromaBackend".to_string()));
        assert!(mined.entities.contains(&"store.add(doc)".to_string()));
        assert_eq!(mined.session_id, "sess_1");
        // 元数据应记录实体与关键词数量
        let expected_entity_count = mined.entities.len().to_string();
        assert_eq!(
            mined.metadata.get("entity_count").map(String::as_str),
            Some(expected_entity_count.as_str())
        );
    }

    #[test]
    fn test_mine_exchange_dedup_entities() {
        let miner = ConvoMiner::new();
        // 用户与助手都提到 `ChromaBackend`，合并后应只出现一次
        let exchange = ConvoExchange {
            user_message: "用 `ChromaBackend`".to_string(),
            assistant_message: "`chromabackend` 已就绪".to_string(),
            timestamp: Utc::now(),
            session_id: "sess_2".to_string(),
        };
        let mined = miner.mine_exchange(&exchange);
        let count = mined
            .entities
            .iter()
            .filter(|e| e.eq_ignore_ascii_case("ChromaBackend"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_mine_session() {
        let miner = ConvoMiner::new();
        let exchanges = vec![
            ConvoExchange {
                user_message: "第一轮 `foo`".to_string(),
                assistant_message: "回复".to_string(),
                timestamp: Utc::now(),
                session_id: "s".to_string(),
            },
            ConvoExchange {
                user_message: "第二轮 `bar`".to_string(),
                assistant_message: "回复".to_string(),
                timestamp: Utc::now(),
                session_id: "s".to_string(),
            },
        ];
        let results = miner.mine_session(&exchanges);
        assert_eq!(results.len(), 2);
        assert!(results[0].entities.contains(&"foo".to_string()));
        assert!(results[1].entities.contains(&"bar".to_string()));
    }

    // ── 记忆格式化测试 ────────────────────────────────

    #[test]
    fn test_format_memory_basic() {
        let ts = Utc::now();
        let exchange = ConvoExchange {
            user_message: "你好".to_string(),
            assistant_message: "你好，有什么可以帮助你？".to_string(),
            timestamp: ts,
            session_id: "sess_42".to_string(),
        };
        let entities = vec!["Rust".to_string()];
        let keywords = vec!["编程".to_string()];
        let formatted = ConvoMiner::format_memory(&exchange, &entities, &keywords);
        assert!(formatted.contains("[Session: sess_42]"));
        assert!(formatted.contains("User: 你好"));
        assert!(formatted.contains("Assistant: 你好，有什么可以帮助你？"));
        assert!(formatted.contains("Entities: Rust"));
        assert!(formatted.contains("Keywords: 编程"));
    }

    #[test]
    fn test_format_memory_empty_fields() {
        let ts = Utc::now();
        let exchange = ConvoExchange {
            user_message: "hi".to_string(),
            assistant_message: "hello".to_string(),
            timestamp: ts,
            session_id: "s".to_string(),
        };
        let formatted = ConvoMiner::format_memory(&exchange, &[], &[]);
        // 空列表应输出为空字符串（标签后无内容）
        assert!(formatted.contains("Entities:"));
        assert!(formatted.contains("Keywords:"));
        assert!(!formatted.contains("Entities: ,"));
    }

    #[test]
    fn test_format_memory_multiple_items() {
        let ts = Utc::now();
        let exchange = ConvoExchange {
            user_message: "问".to_string(),
            assistant_message: "答".to_string(),
            timestamp: ts,
            session_id: "s9".to_string(),
        };
        let entities = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let keywords = vec!["x".to_string(), "y".to_string()];
        let formatted = ConvoMiner::format_memory(&exchange, &entities, &keywords);
        assert!(formatted.contains("Entities: A, B, C"));
        assert!(formatted.contains("Keywords: x, y"));
    }

    // ── 摘要测试 ──────────────────────────────────────

    #[test]
    fn test_summary_truncation() {
        // 超过 80 字符应截断并加省略号
        let long: String = "a".repeat(100);
        let summary = ConvoMiner::make_summary(&long, "");
        assert!(summary.ends_with("..."));
        // 截断后长度 = 80 字符 + "..."
        assert_eq!(summary.chars().count(), 83);
    }

    #[test]
    fn test_summary_fallback_to_assistant() {
        // 用户消息为空时回退到助手消息
        let summary = ConvoMiner::make_summary("", "助手回复内容");
        assert_eq!(summary, "助手回复内容");
    }
}
