//! # 增强实体提取器
//!
//! 从 MemoryNode 中提取高质量实体，作为图谱和空间排序的语义锚点。
//! 支持中文（2-4字复合词）和英文（单词/短语）混合文本，无需外部 NLP 依赖。

use crate::sutra_library::models::MemoryNode;
use crate::sutra_library::recall::EntityUuid;
use std::collections::HashSet;

/// 实体提取器
pub struct EntityExtractor {
    /// 停用词集合
    stop_words: HashSet<String>,
    /// 中文 N-gram 最小长度
    chinese_ngram_min: usize,
    /// 中文 N-gram 最大长度
    chinese_ngram_max: usize,
    /// 内容提取的最大字符数（避免全量处理）
    content_sample_len: usize,
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self {
            stop_words: default_stop_words(),
            chinese_ngram_min: 2,
            chinese_ngram_max: 4,
            content_sample_len: 500,
        }
    }
}

impl EntityExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 自定义停用词
    pub fn with_stop_words(mut self, words: Vec<String>) -> Self {
        for w in words {
            self.stop_words.insert(w);
        }
        self
    }

    /// 从 MemoryNode 提取实体列表
    pub fn extract_from_node(&self, node: &MemoryNode) -> Vec<EntityUuid> {
        let mut seen = HashSet::new();
        let mut entities = Vec::new();

        // 1. 节点自身 ID 作为实体
        self.add_if_new(&node.node_id, &mut seen, &mut entities);

        // 2. 从标题提取
        self.extract_from_text(&node.title, &mut seen, &mut entities);

        // 3. 从摘要提取
        self.extract_from_text(&node.summary, &mut seen, &mut entities);

        // 4. 从内容头部提取（受限长度）
        let content_sample = if node.content.len() > self.content_sample_len {
            &node.content[..self.content_sample_len]
        } else {
            &node.content
        };
        self.extract_from_text(content_sample, &mut seen, &mut entities);

        // 5. 领域作为实体
        if !node.domain.is_empty() {
            self.add_if_new(&format!("domain:{}", node.domain), &mut seen, &mut entities);
        }

        // 6. 从 metadata 中提取
        if let Some(tags) = node.metadata.get("tags").and_then(|v| v.as_array()) {
            for tag in tags {
                if let Some(s) = tag.as_str() {
                    self.add_if_new(&format!("tag:{}", s), &mut seen, &mut entities);
                }
            }
        }
        if let Some(keywords) = node.metadata.get("keywords").and_then(|v| v.as_array()) {
            for kw in keywords {
                if let Some(s) = kw.as_str() {
                    self.add_if_new(s, &mut seen, &mut entities);
                }
            }
        }

        entities
    }

    /// 从多个节点收集实体集合
    pub fn collect_entities(&self, nodes: &[MemoryNode]) -> HashSet<EntityUuid> {
        let mut set = HashSet::new();
        for node in nodes {
            for entity in self.extract_from_node(node) {
                set.insert(entity);
            }
        }
        set
    }

    // ─── 内部方法 ───────────────────────────────────────────

    /// 从文本中提取实体
    fn extract_from_text(&self, text: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        if text.is_empty() {
            return;
        }

        // 提取中文 N-gram
        let cn_ngrams = self.extract_chinese_ngrams(text);
        for ngram in cn_ngrams {
            if !self.stop_words.contains(&ngram) {
                self.add_if_new(&ngram, seen, out);
            }
        }

        // 提取英文/数字词
        let en_tokens = self.extract_english_tokens(text);
        for token in en_tokens {
            let lower = token.to_lowercase();
            if !self.stop_words.contains(&lower) && token.len() > 1 {
                self.add_if_new(&token, seen, out);
            }
        }

        // 提取混合短语（连续中文+英文，如 "Rust编译器"）
        let mixed = self.extract_mixed_phrases(text);
        for phrase in mixed {
            if !self.stop_words.contains(&phrase) {
                self.add_if_new(&phrase, seen, out);
            }
        }
    }

    /// 提取中文 N-gram（2-4 字连续中文序列）
    fn extract_chinese_ngrams(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut results = Vec::new();

        // 找到连续的中文字符段
        let mut i = 0;
        while i < chars.len() {
            if is_chinese_char(chars[i]) {
                let start = i;
                while i < chars.len() && is_chinese_char(chars[i]) {
                    i += 1;
                }
                let cn_segment: String = chars[start..i].iter().collect();

                // 从该段中提取所有 2-4 ngram
                let seg_chars: Vec<char> = cn_segment.chars().collect();
                for j in 0..seg_chars.len() {
                    for len in self.chinese_ngram_min..=self.chinese_ngram_max {
                        if j + len <= seg_chars.len() {
                            let ngram: String = seg_chars[j..j + len].iter().collect();
                            results.push(ngram);
                        }
                    }
                }
            } else {
                i += 1;
            }
        }

        // 去重
        results.sort();
        results.dedup();
        results
    }

    /// 提取英文/数字词（按空白和标点分割）
    fn extract_english_tokens(&self, text: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();

        for c in text.chars() {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                current.push(c);
            } else {
                if !current.is_empty() && current.len() > 1 {
                    // 检查是否包含英文或数字
                    if current
                        .chars()
                        .any(|ch| ch.is_ascii_alphabetic() || ch.is_ascii_digit())
                    {
                        tokens.push(current.clone());
                    }
                }
                current.clear();
            }
        }
        if !current.is_empty() && current.len() > 1 {
            if current
                .chars()
                .any(|ch| ch.is_ascii_alphabetic() || ch.is_ascii_digit())
            {
                tokens.push(current);
            }
        }

        tokens.sort();
        tokens.dedup();
        tokens
    }

    /// 提取混合短语（如 "Rust编译器"、"API接口"）
    fn extract_mixed_phrases(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut phrases = Vec::new();

        let mut i = 0;
        while i < chars.len() {
            // 找到一个英文/数字段后紧跟中文段
            if is_ascii_word_char(chars[i]) {
                let en_start = i;
                while i < chars.len() && is_ascii_word_char(chars[i]) {
                    i += 1;
                }
                let en_part: String = chars[en_start..i].iter().collect();
                if en_part.len() < 20 {
                    // 跳过空白
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    // 看后面是否紧跟中文
                    if i < chars.len() && is_chinese_char(chars[i]) {
                        let cn_start = i;
                        while i < chars.len() && is_chinese_char(chars[i]) {
                            i += 1;
                        }
                        let cn_part: String = chars[cn_start..i].iter().collect();
                        phrases.push(format!("{}{}", en_part, cn_part));
                    }
                }
            } else {
                i += 1;
            }
        }

        phrases.sort();
        phrases.dedup();
        phrases
    }

    fn add_if_new(&self, entity: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
        if seen.insert(entity.to_string()) {
            out.push(entity.to_string());
        }
    }
}

// ─── 辅助函数 ───────────────────────────────────────────────

fn is_chinese_char(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}' |   // CJK Unified Ideographs
        '\u{3400}'..='\u{4dbf}' |   // CJK Extension A
        '\u{f900}'..='\u{faff}'     // CJK Compatibility
    )
}

fn is_ascii_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// 默认停用词集合
fn default_stop_words() -> HashSet<String> {
    let words = vec![
        "的",
        "了",
        "在",
        "是",
        "我",
        "有",
        "和",
        "就",
        "不",
        "人",
        "都",
        "一",
        "一个",
        "上",
        "也",
        "很",
        "到",
        "说",
        "要",
        "去",
        "你",
        "会",
        "着",
        "没有",
        "看",
        "好",
        "自己",
        "这",
        "他",
        "她",
        "它",
        "们",
        "那",
        "些",
        "什么",
        "怎么",
        "为什么",
        "如何",
        "可以",
        "这个",
        "那个",
        "因为",
        "所以",
        "但是",
        "而且",
        "如果",
        "虽然",
        "然后",
        "之后",
        "之前",
        "现在",
        "时候",
        "the",
        "a",
        "an",
        "is",
        "are",
        "was",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "will",
        "would",
        "shall",
        "should",
        "may",
        "might",
        "can",
        "could",
        "this",
        "that",
        "these",
        "those",
        "it",
        "its",
        "they",
        "them",
        "their",
        "we",
        "us",
        "our",
        "you",
        "your",
        "he",
        "she",
        "him",
        "her",
        "his",
        "not",
        "no",
        "nor",
        "and",
        "or",
        "but",
        "if",
        "because",
        "so",
        "than",
        "also",
        "very",
        "just",
        "about",
        "above",
        "after",
        "again",
        "all",
        "am",
        "any",
        "are",
        "at",
        "be",
        "been",
        "being",
        "by",
        "did",
        "each",
        "for",
        "from",
        "has",
        "how",
        "into",
        "more",
        "of",
        "on",
        "only",
        "other",
        "out",
        "over",
        "same",
        "some",
        "such",
        "than",
        "too",
        "under",
        "up",
        "what",
        "when",
        "where",
        "which",
        "who",
        "why",
        "with",
    ];
    words.into_iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_chinese_ngrams() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract_chinese_ngrams("机器学习算法");
        // 2-grams: 机器, 器学, 学习, 习算, 算法
        // 3-grams: 机器学, 器学习, 学习算, 习算法
        // 4-grams: 机器学习, 器学习算, 学习算法
        assert!(result.contains(&"机器".to_string()));
        assert!(result.contains(&"学习".to_string()));
        assert!(result.contains(&"算法".to_string()));
        assert!(result.contains(&"机器学习".to_string()));
        assert!(result.contains(&"学习算法".to_string()));
    }

    #[test]
    fn test_extract_english_tokens() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract_english_tokens("Hello World from Rust");
        assert!(result.contains(&"Hello".to_string()));
        assert!(result.contains(&"World".to_string()));
        assert!(result.contains(&"Rust".to_string()));
        // "from" 应该被过滤，因为它是停用词... 不对，停用词过滤是在 extract_from_text 中
        // extract_english_tokens 只做分词，不做停用词过滤
        assert!(result.contains(&"from".to_string()));
    }

    #[test]
    fn test_extract_mixed_phrases() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract_mixed_phrases("Rust编译器 API接口");
        assert!(result.contains(&"Rust编译器".to_string()));
        assert!(result.contains(&"API接口".to_string()));
    }

    #[test]
    fn test_stop_word_filtering() {
        let extractor = EntityExtractor::default();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        extractor.extract_from_text("的 了 在 机器学习 算法", &mut seen, &mut out);
        // "的", "了", "在" 是停用词，不应该出现在结果中
        // 但 "机器学习" 和 "算法" 应该出现
        assert!(out.contains(&"机器学习".to_string()));
        assert!(out.contains(&"算法".to_string()));
    }
}
