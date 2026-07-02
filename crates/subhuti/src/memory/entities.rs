//! # Entities - 结构化实体提取与注册表
//!
//! 纯规则（无 LLM）从文本中抽取确定性结构化 token，
//! 用于填充实体元数据。
//!
//! ## 组成
//!
//! - [`Entity`] / [`EntityType`] / [`EntitySource`]: 实体数据模型
//! - [`EntityExtractor`][]: 基于正则的纯规则实体抽取器（语言中立）
//! - [`EntityRegistry`][]: 线程安全的个人实体注册表

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{OnceLock, RwLock};
use tracing::{debug, info};

// ── 数据模型 ──────────────────────────────────────────

/// 实体类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// 人物
    Person,
    /// 项目
    Project,
    /// 主题
    Topic,
    /// 概念
    Concept,
    /// 代码符号 / 标识符
    Code,
    /// 工具 / 服务
    Tool,
    /// 未知类型
    Unknown,
}

/// 实体来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitySource {
    /// 用户引导时登记
    Onboarding,
    /// 运行中学习得到
    Learned,
    /// 自动探测得到
    Detected,
}

/// 实体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// 实体名称（保留首次出现的表面形式）
    pub name: String,
    /// 实体类型
    pub entity_type: EntityType,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 来源
    pub source: EntitySource,
}

// ── 正则缓存 ──────────────────────────────────────────
//
// 使用 OnceLock 延迟编译正则，进程内只编译一次。

static RE_BACKTICK: OnceLock<Regex> = OnceLock::new();
static RE_URL: OnceLock<Regex> = OnceLock::new();
static RE_FILE_PATH: OnceLock<Regex> = OnceLock::new();
static RE_DOTTED: OnceLock<Regex> = OnceLock::new();
static RE_CAMEL: OnceLock<Regex> = OnceLock::new();
static RE_SNAKE: OnceLock<Regex> = OnceLock::new();

/// 反引号代码片段：`foo`、`obj.method()`
fn backtick_re() -> &'static Regex {
    RE_BACKTICK.get_or_init(|| Regex::new(r"`([^`\n]+)`").unwrap())
}

/// URL：http(s)://...
fn url_re() -> &'static Regex {
    RE_URL.get_or_init(|| Regex::new(r"\bhttps?://[^\s)>\]\}]+").unwrap())
}

/// 文件路径（含至少一个路径分隔符且以扩展名结尾）：rag/foo.py、a/b/c.tsx
fn file_path_re() -> &'static Regex {
    RE_FILE_PATH
        .get_or_init(|| Regex::new(r"(?:[A-Za-z0-9._-]+/)+[A-Za-z0-9_-]+\.[A-Za-z0-9]+").unwrap())
}

/// 点分限定符：module.func、pkg.Class.method
fn dotted_re() -> &'static Regex {
    RE_DOTTED.get_or_init(|| {
        Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)+\b").unwrap()
    })
}

/// CamelCase 符号（2 个及以上驼峰）：ChromaBackend、MemoryStack
fn camel_re() -> &'static Regex {
    RE_CAMEL.get_or_init(|| Regex::new(r"\b[A-Z][a-z]+(?:[A-Z][a-z]+)+\b").unwrap())
}

/// snake_case 标识符（含至少一个下划线）：do_thing、_extract_authored_at
fn snake_re() -> &'static Regex {
    RE_SNAKE.get_or_init(|| Regex::new(r"\b[a-z_][a-z0-9_]*_[a-z0-9_]+\b").unwrap())
}

/// 匹配区间，用于去重与包含过滤
struct Span {
    text: String,
    start: usize,
    end: usize,
}

// ── 实体抽取器 ────────────────────────────────────────

/// 纯规则实体抽取器（语言中立，无 LLM）
///
/// 使用一组正则从文本中提取结构化 token 候选：
/// 1. 反引号代码片段
/// 2. URL
/// 3. 文件路径（含扩展名）
/// 4. 点分限定符
/// 5. CamelCase 符号
/// 6. snake_case 标识符
#[derive(Debug, Clone)]
pub struct EntityExtractor;

impl EntityExtractor {
    /// 创建抽取器（无状态，仅为 API 一致性）
    pub fn new() -> Self {
        Self
    }

    /// 从文本中抽取结构化实体候选
    ///
    /// 处理流程：
    /// 1. 多组正则并行匹配，收集所有候选区间
    /// 2. 包含过滤：被更长区间完全覆盖的子匹配被丢弃
    /// 3. 大小写不敏感去重，保留首次出现的表面形式
    /// 4. 长度过滤：2..=64 字符
    /// 5. 按出现次数降序排序（相同次数按首次出现位置升序）
    /// 6. 截断至最多 24 个
    pub fn extract(text: &str) -> Vec<String> {
        info!(
            "Entities: EntityExtractor::extract 开始，文本长度={}",
            text.len()
        );
        const MAX_ENTITIES: usize = 24;
        const MIN_LEN: usize = 2;
        const MAX_LEN: usize = 64;

        let mut spans: Vec<Span> = Vec::new();

        for cap in backtick_re().captures_iter(text) {
            if let Some(m) = cap.get(1) {
                let s = m.as_str().trim();
                if !s.is_empty() {
                    spans.push(Span {
                        text: s.to_string(),
                        start: m.start(),
                        end: m.end(),
                    });
                }
            }
        }
        for m in url_re().find_iter(text) {
            spans.push(Span {
                text: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in file_path_re().find_iter(text) {
            spans.push(Span {
                text: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in dotted_re().find_iter(text) {
            spans.push(Span {
                text: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in camel_re().find_iter(text) {
            spans.push(Span {
                text: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }
        for m in snake_re().find_iter(text) {
            spans.push(Span {
                text: m.as_str().to_string(),
                start: m.start(),
                end: m.end(),
            });
        }

        debug!("Entities: 原始匹配数={}", spans.len());

        spans.sort_by_key(|b| std::cmp::Reverse(b.end - b.start));
        let mut kept: Vec<Span> = Vec::new();
        for s in spans {
            let contained = kept.iter().any(|k| s.start >= k.start && s.end <= k.end);
            if !contained {
                kept.push(s);
            }
        }

        debug!("Entities: 包含过滤后保留={}", kept.len());

        let mut order: Vec<String> = Vec::new();
        let mut first_pos: HashMap<String, usize> = HashMap::new();
        let mut first_surface: HashMap<String, String> = HashMap::new();
        let mut count: HashMap<String, usize> = HashMap::new();

        kept.sort_by_key(|a| a.start);

        for s in kept {
            let text = s.text;
            let len = text.chars().count();
            if !(MIN_LEN..=MAX_LEN).contains(&len) {
                continue;
            }
            let key = text.to_lowercase();
            *count.entry(key.clone()).or_insert(0) += 1;
            first_pos.entry(key.clone()).or_insert(s.start);
            if !first_surface.contains_key(&key) {
                first_surface.insert(key.clone(), text);
                order.push(key);
            }
        }

        order.sort_by(|a, b| {
            let ca = count.get(a).copied().unwrap_or(0);
            let cb = count.get(b).copied().unwrap_or(0);
            match cb.cmp(&ca) {
                std::cmp::Ordering::Equal => {
                    let pa = first_pos.get(a).copied().unwrap_or(usize::MAX);
                    let pb = first_pos.get(b).copied().unwrap_or(usize::MAX);
                    pa.cmp(&pb)
                }
                other => other,
            }
        });

        let result: Vec<String> = order
            .into_iter()
            .take(MAX_ENTITIES)
            .map(|k| first_surface.remove(&k).unwrap_or(k))
            .collect();

        info!(
            "Entities: EntityExtractor::extract 完成，抽取实体数={}",
            result.len()
        );
        debug!("Entities: 抽取结果 {:?}", result);

        result
    }
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self::new()
    }
}

// ── 实体注册表 ────────────────────────────────────────

/// 易与姓名混淆的常见英文单词集合
fn ambiguous_words() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        [
            "ever", "grace", "will", "bill", "mark", "april", "may", "june", "joy", "hope", "faith",
        ]
        .into_iter()
        .collect()
    })
}

/// 计算两个字符串的 Levenshtein 编辑距离
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let m = a.len();
    let n = b.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// 线程安全的个人实体注册表
///
/// 存储已知实体，支持大小写不敏感查询、按类型列举、
/// 易混淆词检测与基于编辑距离的相似名检查。
pub struct EntityRegistry {
    /// 键为实体名称的小写形式，值保留原始表面形式
    entities: RwLock<HashMap<String, Entity>>,
}

impl EntityRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        info!("Entities: EntityRegistry 初始化");
        Self {
            entities: RwLock::new(HashMap::new()),
        }
    }

    /// 查询实体（大小写不敏感）
    pub fn lookup(&self, name: &str) -> Option<Entity> {
        let entities = self.entities.read().unwrap();
        let result = entities.get(&name.to_lowercase()).cloned();
        debug!(
            "Entities: EntityRegistry::lookup name={}, found={}",
            name,
            result.is_some()
        );
        result
    }

    /// 注册实体（同名覆盖）
    pub fn register(&self, entity: Entity) {
        info!(
            "Entities: EntityRegistry::register name={}, type={:?}, confidence={}",
            entity.name, entity.entity_type, entity.confidence
        );
        let mut entities = self.entities.write().unwrap();
        let was_present = entities.contains_key(&entity.name.to_lowercase());
        entities.insert(entity.name.to_lowercase(), entity);
        if was_present {
            debug!("Entities: EntityRegistry::register 已覆盖同名实体");
        }
    }

    /// 判断实体是否已知（大小写不敏感）
    pub fn is_known(&self, name: &str) -> bool {
        let entities = self.entities.read().unwrap();
        let result = entities.contains_key(&name.to_lowercase());
        debug!(
            "Entities: EntityRegistry::is_known name={}, result={}",
            name, result
        );
        result
    }

    /// 按类型列出实体
    pub fn list_by_type(&self, entity_type: EntityType) -> Vec<Entity> {
        let entities = self.entities.read().unwrap();
        let result: Vec<Entity> = entities
            .values()
            .filter(|e| e.entity_type == entity_type)
            .cloned()
            .collect();
        info!(
            "Entities: EntityRegistry::list_by_type type={:?}, count={}",
            entity_type,
            result.len()
        );
        result
    }

    /// 判断名称是否为易与姓名混淆的常见英文单词
    ///
    /// 例如 "will"、"grace"、"april" 等既可以是人名也可以是普通单词。
    pub fn is_ambiguous(&self, name: &str) -> bool {
        let result = ambiguous_words().contains(name.to_lowercase().as_str());
        debug!(
            "Entities: EntityRegistry::is_ambiguous name={}, result={}",
            name, result
        );
        result
    }

    /// 检查名称与已知实体的混淆情况
    ///
    /// 返回编辑距离 1~2 的已知实体名称列表（不包含完全相同的名称）。
    pub fn check_confusion(&self, name: &str) -> Vec<String> {
        info!("Entities: EntityRegistry::check_confusion name={}", name);
        let entities = self.entities.read().unwrap();
        let target = name.to_lowercase();
        let mut result: Vec<String> = Vec::new();
        for (key, entity) in entities.iter() {
            let dist = levenshtein(&target, key);
            if dist > 0 && dist <= 2 {
                result.push(entity.name.clone());
            }
        }
        info!(
            "Entities: EntityRegistry::check_confusion name={}, 发现 {} 个相似实体",
            name,
            result.len()
        );
        debug!(
            "Entities: EntityRegistry::check_confusion 相似实体 {:?}",
            result
        );
        result
    }

    /// 已注册实体数量
    pub fn len(&self) -> usize {
        let entities = self.entities.read().unwrap();
        entities.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for EntityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── EntityExtractor 模式测试 ──────────────────────

    #[test]
    fn test_extract_backtick() {
        let entities = EntityExtractor::extract("使用 `foo` 函数处理数据");
        assert!(entities.contains(&"foo".to_string()));
    }

    #[test]
    fn test_extract_backtick_with_method() {
        let entities = EntityExtractor::extract("调用 `obj.method()` 完成");
        assert!(entities.contains(&"obj.method()".to_string()));
    }

    #[test]
    fn test_extract_url() {
        let entities = EntityExtractor::extract("参见 https://example.com/page 详情");
        assert!(entities
            .iter()
            .any(|e| e.starts_with("https://example.com")));
    }

    #[test]
    fn test_extract_file_path() {
        let entities = EntityExtractor::extract("代码位于 rag/foo.py 中");
        assert!(entities.contains(&"rag/foo.py".to_string()));

        let entities = EntityExtractor::extract("路径 a/b/c.tsx");
        assert!(entities.contains(&"a/b/c.tsx".to_string()));
    }

    #[test]
    fn test_extract_dotted() {
        let entities = EntityExtractor::extract("使用 module.func 和 pkg.Class.method");
        assert!(entities.contains(&"module.func".to_string()));
        assert!(entities.contains(&"pkg.Class.method".to_string()));
    }

    #[test]
    fn test_extract_camel_case() {
        let entities = EntityExtractor::extract("采用 ChromaBackend 与 MemoryStack");
        assert!(entities.contains(&"ChromaBackend".to_string()));
        assert!(entities.contains(&"MemoryStack".to_string()));
    }

    #[test]
    fn test_extract_snake_case() {
        let entities = EntityExtractor::extract("执行 do_thing 或 _extract_authored_at");
        assert!(entities.contains(&"do_thing".to_string()));
        assert!(entities.contains(&"_extract_authored_at".to_string()));
    }

    // ── EntityExtractor 行为测试 ──────────────────────

    #[test]
    fn test_extract_dedup_case_insensitive() {
        // 同名不同大小写应合并，保留首次出现的表面形式
        let entities = EntityExtractor::extract("用 `FooBar` 然后 `foobar` 再次 `FOOBAR`");
        let count = entities
            .iter()
            .filter(|e| e.eq_ignore_ascii_case("FooBar"))
            .count();
        assert_eq!(count, 1);
        assert!(entities.contains(&"FooBar".to_string()));
    }

    #[test]
    fn test_extract_max_entities() {
        // 生成 30 个不同的反引号 token，应截断至 24
        let words: Vec<String> = (0..30).map(|i| format!("`token{}`", i)).collect();
        let text = words.join(" ");
        let entities = EntityExtractor::extract(&text);
        assert_eq!(entities.len(), 24);
    }

    #[test]
    fn test_extract_length_filter() {
        // 过短（长度 < 2）被过滤；长度 == 2 保留
        let entities = EntityExtractor::extract("`a` 和 `ab`");
        assert!(!entities.contains(&"a".to_string()));
        assert!(entities.contains(&"ab".to_string()));
    }

    #[test]
    fn test_extract_ranking_by_count() {
        // foo 出现 3 次，bar 出现 1 次 → foo 应排在 bar 之前
        let text = "`bar` then `foo` and `foo` again `foo`";
        let entities = EntityExtractor::extract(text);
        let foo_pos = entities.iter().position(|e| e == "foo");
        let bar_pos = entities.iter().position(|e| e == "bar");
        assert!(foo_pos.is_some());
        assert!(bar_pos.is_some());
        assert!(foo_pos < bar_pos);
    }

    #[test]
    fn test_extract_empty() {
        let entities = EntityExtractor::extract("");
        assert!(entities.is_empty());
    }

    #[test]
    fn test_extract_containment_filter() {
        // foo.py 在 rag/foo.py 内部，应被包含过滤掉
        let entities = EntityExtractor::extract("路径 rag/foo.py");
        assert!(entities.contains(&"rag/foo.py".to_string()));
        assert!(!entities.contains(&"foo.py".to_string()));
    }

    #[test]
    fn test_extract_mixed() {
        let text = "在 `ChromaBackend` 中调用 `store.add(doc)` \
                    参见 https://docs.example.com/guide \
                    文件 src/memory/entities.rs \
                    使用 pkg.Module.helper 和 do_thing";
        let entities = EntityExtractor::extract(text);
        assert!(entities.contains(&"ChromaBackend".to_string()));
        assert!(entities.contains(&"store.add(doc)".to_string()));
        assert!(entities.contains(&"src/memory/entities.rs".to_string()));
        assert!(entities.contains(&"pkg.Module.helper".to_string()));
        assert!(entities.contains(&"do_thing".to_string()));
    }

    // ── EntityRegistry 测试 ───────────────────────────

    #[test]
    fn test_registry_register_and_lookup() {
        let registry = EntityRegistry::new();
        registry.register(Entity {
            name: "ChromaBackend".to_string(),
            entity_type: EntityType::Tool,
            confidence: 0.9,
            source: EntitySource::Learned,
        });

        // 大小写不敏感查询
        assert_eq!(
            registry.lookup("chromabackend").unwrap().name,
            "ChromaBackend"
        );
        assert_eq!(
            registry.lookup("CHROMABACKEND").unwrap().name,
            "ChromaBackend"
        );
    }

    #[test]
    fn test_registry_is_known() {
        let registry = EntityRegistry::new();
        registry.register(Entity {
            name: "Rust".to_string(),
            entity_type: EntityType::Tool,
            confidence: 1.0,
            source: EntitySource::Onboarding,
        });
        assert!(registry.is_known("Rust"));
        assert!(registry.is_known("rust"));
        assert!(!registry.is_known("Python"));
    }

    #[test]
    fn test_registry_list_by_type() {
        let registry = EntityRegistry::new();
        registry.register(Entity {
            name: "Rust".to_string(),
            entity_type: EntityType::Tool,
            confidence: 1.0,
            source: EntitySource::Onboarding,
        });
        registry.register(Entity {
            name: "Alice".to_string(),
            entity_type: EntityType::Person,
            confidence: 1.0,
            source: EntitySource::Onboarding,
        });
        registry.register(Entity {
            name: "Python".to_string(),
            entity_type: EntityType::Tool,
            confidence: 0.8,
            source: EntitySource::Learned,
        });

        let tools = registry.list_by_type(EntityType::Tool);
        assert_eq!(tools.len(), 2);
        let persons = registry.list_by_type(EntityType::Person);
        assert_eq!(persons.len(), 1);
        assert_eq!(persons[0].name, "Alice");
    }

    #[test]
    fn test_registry_is_ambiguous() {
        let registry = EntityRegistry::new();
        assert!(registry.is_ambiguous("Will"));
        assert!(registry.is_ambiguous("will"));
        assert!(registry.is_ambiguous("Grace"));
        assert!(registry.is_ambiguous("april"));
        assert!(!registry.is_ambiguous("Rust"));
        assert!(!registry.is_ambiguous("ChromaBackend"));
    }

    #[test]
    fn test_registry_check_confusion() {
        let registry = EntityRegistry::new();
        registry.register(Entity {
            name: "Rust".to_string(),
            entity_type: EntityType::Tool,
            confidence: 1.0,
            source: EntitySource::Onboarding,
        });
        registry.register(Entity {
            name: "Rest".to_string(),
            entity_type: EntityType::Tool,
            confidence: 0.5,
            source: EntitySource::Detected,
        });

        // "Rast" 与 "Rust" 编辑距离 1，与 "Rest" 编辑距离 1
        let confused = registry.check_confusion("Rast");
        assert!(confused.contains(&"Rust".to_string()));
        assert!(confused.contains(&"Rest".to_string()));

        // "Rust" 与 "Rest" 编辑距离 1；不返回自身（距离 0）
        let confused = registry.check_confusion("Rust");
        assert!(confused.contains(&"Rest".to_string()));
        assert!(!confused.contains(&"Rust".to_string()));
    }

    #[test]
    fn test_registry_empty() {
        let registry = EntityRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.lookup("anything").is_none());
        assert!(registry.check_confusion("anything").is_empty());
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("abc", "abd"), 1);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_entity_serde() {
        let entity = Entity {
            name: "Test".to_string(),
            entity_type: EntityType::Concept,
            confidence: 0.5,
            source: EntitySource::Detected,
        };
        let json = serde_json::to_string(&entity).unwrap();
        let de: Entity = serde_json::from_str(&json).unwrap();
        assert_eq!(de.name, "Test");
        assert_eq!(de.entity_type, EntityType::Concept);
        assert_eq!(de.source, EntitySource::Detected);
        assert!((de.confidence - 0.5).abs() < f32::EPSILON);
    }
}
