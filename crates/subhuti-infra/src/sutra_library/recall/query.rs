//! # 查询理解
//!
//! 查询扩展、同义词/别名词典、查询分类。
//! 弥补无向量场景下的语义鸿沟。

use std::collections::HashMap;

/// 查询意图
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryIntent {
    /// 精准查找（如 API 名称、函数名）
    Precise,
    /// 标准检索
    Standard,
    /// 深度问答（需要扩展上下文）
    Deep,
}

/// 分析后的查询
#[derive(Debug, Clone)]
pub struct AnalyzedQuery {
    /// 原始查询文本
    pub raw: String,
    /// 查询意图
    pub intent: QueryIntent,
    /// 扩展后的查询列表（输入 BM25 的多个变体）
    pub expansions: Vec<String>,
    /// 提取的关键词
    pub keywords: Vec<String>,
}

/// 同义词/别名词典
#[derive(Debug, Clone)]
pub struct SynonymDict {
    /// 同义词映射: 词 -> 同义词列表
    synonyms: HashMap<String, Vec<String>>,
    /// 别名/缩写映射: 缩写 -> 全称
    aliases: HashMap<String, String>,
}

impl Default for SynonymDict {
    fn default() -> Self {
        let mut dict = Self {
            synonyms: HashMap::new(),
            aliases: HashMap::new(),
        };
        // 默认同义词
        dict.add_synonyms(
            "api",
            &["接口", "API接口", "application programming interface"],
        );
        dict.add_synonyms("search", &["搜索", "检索", "查找", "查询"]);
        dict.add_synonyms("config", &["配置", "设置", "configuration", "configure"]);
        dict.add_synonyms("error", &["错误", "异常", "故障", "bug", "issue"]);
        dict.add_synonyms("create", &["创建", "新建", "生成", "add", "new"]);
        dict.add_synonyms("delete", &["删除", "移除", "remove", "del"]);
        dict.add_synonyms(
            "update",
            &["更新", "修改", "编辑", "edit", "modify", "change"],
        );
        dict.add_synonyms("query", &["查询", "检索", "搜索", "search"]);
        dict.add_synonyms("memory", &["记忆", "存储", "缓存", "cache"]);
        dict.add_synonyms("graph", &["图谱", "图", "关系", "graph"]);
        dict.add_synonyms("feedback", &["反馈", "评价", "评分", "review"]);
        dict.add_synonyms("entity", &["实体", "对象", "概念", "entity"]);
        dict.add_synonyms("learn", &["学习", "训练", "共现", "co-occurrence"]);
        dict.add_synonyms("test", &["测试", "单元测试", "验证", "integration test"]);

        // 默认别名
        dict.add_alias("bm25", "BM25 检索算法");
        dict.add_alias("pg", "PostgreSQL 数据库");
        dict.add_alias("tsvector", "全文检索索引");
        dict.add_alias("bfs", "广度优先搜索");
        dict.add_alias("llm", "大语言模型");

        dict
    }
}

impl SynonymDict {
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加同义词组
    pub fn add_synonyms(&mut self, word: &str, synonyms: &[&str]) {
        let entry = self.synonyms.entry(word.to_lowercase()).or_default();
        for s in synonyms {
            if !entry.contains(&s.to_string()) {
                entry.push(s.to_string());
            }
        }
    }

    /// 添加别名
    pub fn add_alias(&mut self, short: &str, full: &str) {
        self.aliases.insert(short.to_lowercase(), full.to_string());
    }

    /// 获取同义词
    pub fn get_synonyms(&self, word: &str) -> Option<&[String]> {
        self.synonyms
            .get(&word.to_lowercase())
            .map(|v| v.as_slice())
    }

    /// 展开别名
    pub fn expand_alias(&self, word: &str) -> Option<&str> {
        self.aliases.get(&word.to_lowercase()).map(|s| s.as_str())
    }

    /// 注册领域特定同义词
    pub fn register_domain_synonyms(&mut self, _domain: &str, pairs: &[(&str, &[&str])]) {
        for (word, syns) in pairs {
            self.add_synonyms(word, syns);
        }
    }
}

/// 查询分析器
pub struct QueryAnalyzer {
    dict: SynonymDict,
}

impl Default for QueryAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryAnalyzer {
    pub fn new() -> Self {
        Self {
            dict: SynonymDict::default(),
        }
    }

    /// 获取同义词词典引用（便于注册领域词）
    pub fn dict_mut(&mut self) -> &mut SynonymDict {
        &mut self.dict
    }

    /// 分析查询
    pub fn analyze(&self, query: &str) -> AnalyzedQuery {
        let raw = query.to_string();
        let intent = self.classify(query);
        let keywords = self.extract_keywords(query);
        let expansions = self.expand_query(query);

        AnalyzedQuery {
            raw,
            intent,
            expansions,
            keywords,
        }
    }

    /// 查询分类
    pub fn classify(&self, query: &str) -> QueryIntent {
        let lower = query.to_lowercase();

        // 精准检索模式：看起来像代码/API/路径
        if query.contains("::")       // Rust 路径
            || query.contains(".") && query.chars().any(|c| c.is_ascii_uppercase()) // 类名.方法
            || query.starts_with('/') // 路径
            || query.starts_with("fn ") // 函数
            || query.starts_with("struct ")
            || query.starts_with("impl ")
            || query.starts_with("trait ")
            || query.starts_with("enum ")
        {
            return QueryIntent::Precise;
        }

        // 深度问答模式：需要扩展上下文
        let deep_indicators = [
            "为什么",
            "如何",
            "怎么",
            "有什么区别",
            "是什么",
            "原理",
            "how",
            "why",
            "what is",
            "explain",
            "difference between",
            "compare",
            "vs",
            "versus",
            "原理",
        ];
        if deep_indicators.iter().any(|w| lower.contains(w)) {
            return QueryIntent::Deep;
        }

        // 短查询倾向精准（区分中英文）
        let word_count = query.split_whitespace().count();
        let chinese_chars: usize = query.chars().filter(|c| is_chinese(*c)).count();
        let has_english = query.chars().any(|c| c.is_ascii_alphabetic());

        // 单字/词：英文 1 词或中文 ≤4 字为精准
        if word_count <= 1 && (has_english || chinese_chars <= 4) {
            return QueryIntent::Precise;
        }
        // 双中文词且总字 ≤4 为精准
        if word_count == 2 && !has_english && chinese_chars <= 4 {
            return QueryIntent::Precise;
        }

        QueryIntent::Standard
    }

    /// 提取关键词
    pub fn extract_keywords(&self, query: &str) -> Vec<String> {
        let mut keywords: Vec<String> = Vec::new();

        // 分割并过滤
        for token in query.split_whitespace() {
            let cleaned: String = token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ':')
                .collect();
            if !cleaned.is_empty() && cleaned.len() > 1 {
                // 检查是否包含中文或英文
                if cleaned.chars().any(|c| c.is_alphabetic()) {
                    keywords.push(cleaned);
                }
            }
        }

        // 提取中文短语：从连续中文字段中提取 2-4 字滑动窗口
        let chars: Vec<char> = query.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if is_chinese(chars[i]) {
                let start = i;
                while i < chars.len() && is_chinese(chars[i]) {
                    i += 1;
                }
                let segment: String = chars[start..i].iter().collect();
                if segment.len() >= 2 && segment.len() <= 8 {
                    keywords.push(segment.clone());
                }
                // 从长段中提取 2-4 字子短语
                if segment.len() > 4 {
                    let seg_chars: Vec<char> = segment.chars().collect();
                    for j in 0..seg_chars.len() {
                        for len in 2..=4.min(seg_chars.len() - j) {
                            let sub: String = seg_chars[j..j + len].iter().collect();
                            if sub.len() >= 2 {
                                keywords.push(sub);
                            }
                        }
                    }
                }
            } else {
                i += 1;
            }
        }

        keywords.sort();
        keywords.dedup();
        keywords
    }

    /// 查询扩展
    pub fn expand_query(&self, query: &str) -> Vec<String> {
        let mut expansions = vec![query.to_string()];

        // 别名展开
        let mut expanded = query.to_string();
        let mut has_alias = false;
        for word in query.split_whitespace() {
            if let Some(full) = self.dict.expand_alias(word) {
                expanded = expanded.replace(word, full);
                has_alias = true;
            }
        }
        if has_alias {
            expansions.push(expanded);
        }

        // 同义词扩展：为每个词生成同义词变体
        let words: Vec<&str> = query.split_whitespace().collect();
        if words.len() <= 3 {
            // 只对短查询做同义词扩展
            for word in &words {
                if let Some(syns) = self.dict.get_synonyms(word) {
                    for syn in syns {
                        let variant = query.replace(word, syn);
                        if variant != query {
                            expansions.push(variant);
                        }
                    }
                }
            }
        }

        // 去重
        expansions.sort();
        expansions.dedup();

        // 限制扩展数量
        expansions.truncate(5);

        expansions
    }
}

fn is_chinese(c: char) -> bool {
    matches!(c,
        '\u{4e00}'..='\u{9fff}' |
        '\u{3400}'..='\u{4dbf}' |
        '\u{f900}'..='\u{faff}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_precise() {
        let analyzer = QueryAnalyzer::new();
        assert_eq!(
            analyzer.classify("subhuti_core::engine"),
            QueryIntent::Precise
        );
        assert_eq!(analyzer.classify("fn main()"), QueryIntent::Precise);
        assert_eq!(analyzer.classify("/path/to/file"), QueryIntent::Precise);
        assert_eq!(analyzer.classify("MemoryEnginePort"), QueryIntent::Precise);
    }

    #[test]
    fn test_classify_deep() {
        let analyzer = QueryAnalyzer::new();
        assert_eq!(analyzer.classify("为什么需要向量数据库"), QueryIntent::Deep);
        assert_eq!(
            analyzer.classify("how to implement search"),
            QueryIntent::Deep
        );
        assert_eq!(analyzer.classify("A vs B 有什么区别"), QueryIntent::Deep);
    }

    #[test]
    fn test_classify_standard() {
        let analyzer = QueryAnalyzer::new();
        assert_eq!(analyzer.classify("机器学习算法比较"), QueryIntent::Standard);
        assert_eq!(
            analyzer.classify("memory storage implementation"),
            QueryIntent::Standard
        );
    }

    #[test]
    fn test_query_expansion() {
        let analyzer = QueryAnalyzer::new();
        let expansions = analyzer.expand_query("pg search");
        assert!(!expansions.is_empty());
        // 应该包含原始查询
        assert!(expansions.contains(&"pg search".to_string()));
        // "pg" 应该被展开为 "PostgreSQL 数据库"
        assert!(expansions.iter().any(|e| e.contains("PostgreSQL")));
    }

    #[test]
    fn test_synonym_expansion() {
        let analyzer = QueryAnalyzer::new();
        let expansions = analyzer.expand_query("create api");
        // "create" 的同义词: "创建", "新建", "生成", "add", "new"
        // "api" 的同义词: "接口", "API接口"
        assert!(!expansions.is_empty());
        assert!(expansions.contains(&"create api".to_string()));
    }

    #[test]
    fn test_keyword_extraction() {
        let analyzer = QueryAnalyzer::new();
        let keywords = analyzer.extract_keywords("如何使用 Rust 实现搜索引擎");
        assert!(keywords.contains(&"Rust".to_string()));
        assert!(keywords.contains(&"搜索引擎".to_string()));
        assert!(keywords.contains(&"实现".to_string()));
    }
}
