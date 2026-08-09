//! # 事实核查器 (Fact Checker)
//!
//! 检查 AI 响应与 knowledge_graph 和 entity_registry 中已知事实的一致性。
//!
//! ## 检测的问题类型
//!
//! - **相似名混淆 (SimilarName)**: 文本中出现的名称与已知实体相近
//! - **关系不匹配 (RelationshipMismatch)**: 文本断言的关系与知识图谱矛盾
//! - **过期事实 (StaleFact)**: 文本断言的事实已被知识图谱标记为失效

use super::entities::EntityRegistry;
use super::knowledge_graph::{KnowledgeGraph, QueryDirection, Triple};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock};
use tracing::{debug, info, warn};

// ── 正则缓存 ──────────────────────────────────────────
//
// 使用 OnceLock 延迟编译正则，进程内只编译一次。

/// 缓存 "X is Y's Z" 模式的正则
static RE_IS_POSSESSIVE: OnceLock<Regex> = OnceLock::new();
/// 缓存 "X's Z is Y" 模式的正则
static RE_POSSESSIVE_IS: OnceLock<Regex> = OnceLock::new();
/// 缓存大写词检测正则（用于实体混淆检测）
static RE_CAPITALIZED: OnceLock<Regex> = OnceLock::new();

/// "X is Y's Z" 模式：subject=X, possessor=Y, role=Z
fn is_possessive_re() -> &'static Regex {
    RE_IS_POSSESSIVE.get_or_init(|| {
        Regex::new(r"([A-Z][\w-]+)\s+is\s+([A-Z][\w-]+)'s\s+([a-z]{3,20})").unwrap()
    })
}

/// "X's Z is Y" 模式：possessor=X, role=Z, subject=Y
fn possessive_is_re() -> &'static Regex {
    RE_POSSESSIVE_IS.get_or_init(|| {
        Regex::new(r"([A-Z][\w-]+)'s\s+([a-z]{3,20})\s+is\s+([A-Z][\w-]+)").unwrap()
    })
}

/// 首字母大写的词（用于实体混淆检测）
fn capitalized_re() -> &'static Regex {
    RE_CAPITALIZED.get_or_init(|| Regex::new(r"\b[A-Z][a-zA-Z]+\b").unwrap())
}

// ── 数据模型 ──────────────────────────────────────────

/// 事实问题类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueType {
    /// 相似名混淆：文本中出现的名称与已知实体相近
    SimilarName,
    /// 关系不匹配：文本断言的关系与知识图谱矛盾
    RelationshipMismatch,
    /// 过期事实：文本断言的事实已被知识图谱标记为失效
    StaleFact,
}

/// 事实问题
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactIssue {
    /// 问题类型
    pub issue_type: IssueType,
    /// 问题描述
    pub description: String,
    /// 相关实体
    pub entity: String,
    /// 详细信息
    pub detail: String,
}

// ── 事实核查器 ────────────────────────────────────────

/// 事实核查器
///
/// 基于 knowledge_graph 和 entity_registry 检查文本中的事实问题。
pub struct FactChecker {
    /// knowledge_graph（可选，未配置时跳过 KG 相关检查）
    knowledge_graph: Option<Arc<KnowledgeGraph>>,
    /// entity_registry
    entity_registry: Arc<RwLock<EntityRegistry>>,
}

impl FactChecker {
    /// 创建新的事实核查器
    pub fn new(
        knowledge_graph: Option<Arc<KnowledgeGraph>>,
        entity_registry: Arc<RwLock<EntityRegistry>>,
    ) -> Self {
        Self {
            knowledge_graph,
            entity_registry,
        }
    }

    /// 主入口：检查文本中的事实问题
    ///
    /// 依次执行：
    /// 1. 实体混淆检查（同步）
    /// 2. 知识图谱矛盾检查（异步）
    pub async fn check_text(&self, text: &str) -> Vec<FactIssue> {
        info!("FactChecker: check_text 开始，文本长度={}", text.len());
        let mut issues = Vec::new();

        // 1. 实体混淆检查
        let confusion_issues = self.check_entity_confusion(text);
        debug!(
            "FactChecker: 实体混淆检查完成，发现 {} 个问题",
            confusion_issues.len()
        );
        issues.extend(confusion_issues);

        // 2. 知识图谱矛盾检查
        let kg_issues = self.check_kg_contradictions(text).await;
        debug!(
            "FactChecker: KG 矛盾检查完成，发现 {} 个问题",
            kg_issues.len()
        );
        issues.extend(kg_issues);

        info!(
            "FactChecker: check_text 完成，共发现 {} 个问题",
            issues.len()
        );
        issues
    }

    /// 实体混淆检查
    ///
    /// 在文本中查找首字母大写的词，与实体注册表中的已知实体进行相似度比对。
    /// 跳过已知实体（精确匹配）和易混淆的常见英文单词。
    pub fn check_entity_confusion(&self, text: &str) -> Vec<FactIssue> {
        info!(
            "FactChecker: check_entity_confusion 开始，文本长度={}",
            text.len()
        );
        let registry = self.entity_registry.read().unwrap();
        let mut issues = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();

        for m in capitalized_re().find_iter(text) {
            let token = m.as_str();
            let key = token.to_lowercase();

            // 去重：同一名称只报告一次
            if !seen.insert(key.clone()) {
                debug!("FactChecker: 跳过重复 token={}", token);
                continue;
            }

            // 跳过已知实体（精确匹配，无需混淆告警）
            if registry.is_known(token) {
                debug!("FactChecker: 跳过已知实体 token={}", token);
                continue;
            }

            // 跳过易混淆的常见英文单词（如 will、grace、april 等）
            if registry.is_ambiguous(token) {
                debug!("FactChecker: 跳过易混淆词 token={}", token);
                continue;
            }

            // 检查与已知实体的相似度（编辑距离 1~2）
            let similar = registry.check_confusion(token);
            if !similar.is_empty() {
                debug!(
                    "FactChecker: 发现混淆 token={}, 相似实体={:?}",
                    token, similar
                );
                issues.push(FactIssue {
                    issue_type: IssueType::SimilarName,
                    description: format!(
                        "文本中的 \"{}\" 与已知实体 {} 相似，可能存在名称混淆",
                        token,
                        similar
                            .iter()
                            .map(|s| format!("\"{}\"", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    entity: token.to_string(),
                    detail: format!("相似已知实体: {}", similar.join(", ")),
                });
            }
        }

        info!(
            "FactChecker: check_entity_confusion 完成，发现 {} 个混淆问题",
            issues.len()
        );
        issues
    }

    /// 知识图谱矛盾检查
    ///
    /// 解析 "X is Y's Z" 和 "X's Z is Y" 模式，提取 (主语, 谓词, 宾语) 三元组，
    /// 然后查询知识图谱验证：
    /// - 若 KG 中同一主语-宾语对的谓词不同，标记为关系不匹配
    /// - 若 KG 中相同事实的 valid_to 已过期，标记为过期事实
    pub async fn check_kg_contradictions(&self, text: &str) -> Vec<FactIssue> {
        info!(
            "FactChecker: check_kg_contradictions 开始，文本长度={}",
            text.len()
        );
        let mut issues = Vec::new();

        // 未配置知识图谱时跳过
        let kg = match &self.knowledge_graph {
            Some(kg) => kg,
            None => {
                debug!("FactChecker: 未配置知识图谱，跳过 KG 矛盾检查");
                return issues;
            }
        };

        // 解析文本中的关系断言
        let assertions = parse_relationships(text);
        debug!("FactChecker: 解析到 {} 个关系断言", assertions.len());

        for (subject, predicate, object) in assertions {
            debug!(
                "FactChecker: 检查断言 subject={}, predicate={}, object={}",
                subject, predicate, object
            );
            // 查询主语的出边关系
            let triples: Vec<Triple> = match kg
                .query_entity(&subject, QueryDirection::Outgoing, None, 50)
                .await
            {
                Ok(t) => t,
                Err(e) => {
                    warn!("FactChecker: 查询 KG 失败 subject={}, error={}", subject, e);
                    continue;
                }
            };

            debug!("FactChecker: KG 查询命中 {} 条三元组", triples.len());

            // 在查询结果中查找同一宾语的三元组
            for triple in &triples {
                if triple.object.to_lowercase() != object.to_lowercase() {
                    continue;
                }

                // 同一主语-宾语对，谓词不同 → 关系不匹配
                if triple.predicate.to_lowercase() != predicate.to_lowercase() {
                    debug!(
                        "FactChecker: 发现关系不匹配 text_predicate={}, kg_predicate={}",
                        predicate, triple.predicate
                    );
                    issues.push(FactIssue {
                        issue_type: IssueType::RelationshipMismatch,
                        description: format!(
                            "文本断言 \"{}\" 是 \"{}\" 的 \"{}\"，但知识图谱记录的谓词为 \"{}\"",
                            subject, object, predicate, triple.predicate
                        ),
                        entity: subject.clone(),
                        detail: format!(
                            "KG 三元组: {} --[{}]--> {} (id={})",
                            triple.subject, triple.predicate, triple.object, triple.id
                        ),
                    });
                } else {
                    // 谓词相同，检查是否已过期
                    if let Some(valid_to) = triple.valid_to {
                        if valid_to < Utc::now() {
                            debug!("FactChecker: 发现过期事实 valid_to={}", valid_to);
                            issues.push(FactIssue {
                                issue_type: IssueType::StaleFact,
                                description: format!(
                                    "文本断言 \"{}\" 是 \"{}\" 的 \"{}\"，但该事实已于 {} 失效",
                                    subject, object, predicate, valid_to
                                ),
                                entity: subject.clone(),
                                detail: format!(
                                    "KG 三元组 valid_to={} (id={})",
                                    valid_to, triple.id
                                ),
                            });
                        }
                    }
                }
            }
        }

        info!(
            "FactChecker: check_kg_contradictions 完成，发现 {} 个 KG 矛盾问题",
            issues.len()
        );
        issues
    }
}

// ── 关系模式解析 ──────────────────────────────────────

/// 解析文本中的关系断言
///
/// 支持两种模式：
/// - "X is Y's Z" → (subject=X, predicate=Z, object=Y)
/// - "X's Z is Y" → (subject=Y, predicate=Z, object=X)
fn parse_relationships(text: &str) -> Vec<(String, String, String)> {
    let mut result = Vec::new();

    // 模式 1: "X is Y's Z"
    // 捕获组: 1=subject(X), 2=possessor(Y), 3=role(Z)
    for cap in is_possessive_re().captures_iter(text) {
        let subject = cap[1].to_string();
        let possessor = cap[2].to_string();
        let role = cap[3].to_string();
        result.push((subject, role, possessor));
    }

    // 模式 2: "X's Z is Y"
    // 捕获组: 1=possessor(X), 2=role(Z), 3=subject(Y)
    for cap in possessive_is_re().captures_iter(text) {
        let possessor = cap[1].to_string();
        let role = cap[2].to_string();
        let subject = cap[3].to_string();
        result.push((subject, role, possessor));
    }

    result
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::entities::{Entity, EntitySource, EntityType};
    use super::*;

    // ── 关系模式解析测试 ──────────────────────────────

    #[test]
    fn test_parse_pattern_is_possessive() {
        // "Alice is Bob's mother" → subject=Alice, predicate=mother, object=Bob
        let assertions = parse_relationships("Alice is Bob's mother");
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].0, "Alice");
        assert_eq!(assertions[0].1, "mother");
        assert_eq!(assertions[0].2, "Bob");
    }

    #[test]
    fn test_parse_pattern_possessive_is() {
        // "Bob's mother is Alice" → subject=Alice, predicate=mother, object=Bob
        let assertions = parse_relationships("Bob's mother is Alice");
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].0, "Alice");
        assert_eq!(assertions[0].1, "mother");
        assert_eq!(assertions[0].2, "Bob");
    }

    #[test]
    fn test_parse_both_patterns_equivalent() {
        // 两种模式表达相同事实，解析结果应一致
        let a1 = parse_relationships("Alice is Bob's mother");
        let a2 = parse_relationships("Bob's mother is Alice");
        assert_eq!(a1, a2);
    }

    #[test]
    fn test_parse_multiple_assertions() {
        // 两种模式同时出现
        let text = "Alice is Bob's mother and Charlie's father is Dave";
        let assertions = parse_relationships(text);
        assert_eq!(assertions.len(), 2);
    }

    #[test]
    fn test_parse_no_match() {
        // 无关系断言的文本
        let assertions = parse_relationships("The quick brown fox jumps");
        assert!(assertions.is_empty());
    }

    #[test]
    fn test_parse_role_length_constraints() {
        // 角色词长度需在 3~20 之间
        // "xy" 长度 2，不匹配
        let a = parse_relationships("Alice is Bob's xy");
        assert!(a.is_empty());
        // "mother" 长度 6，匹配
        let a = parse_relationships("Alice is Bob's mother");
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn test_parse_with_hyphenated_names() {
        // 名字可包含连字符
        let a = parse_relationships("Mary-Jane is John's supervisor");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].0, "Mary-Jane");
        assert_eq!(a[0].2, "John");
    }

    // ── 实体混淆检测测试 ──────────────────────────────

    /// 构建测试用 entity_registry
    fn make_registry_with(entities: &[(&str, EntityType)]) -> Arc<RwLock<EntityRegistry>> {
        let registry = EntityRegistry::new();
        for (name, etype) in entities {
            registry.register(Entity {
                name: name.to_string(),
                entity_type: *etype,
                confidence: 1.0,
                source: EntitySource::Onboarding,
            });
        }
        Arc::new(RwLock::new(registry))
    }

    #[test]
    fn test_entity_confusion_detects_similar() {
        // 注册 "Rust"，文本中出现 "Rast"（编辑距离 1）
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("I prefer Rast over Python");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, IssueType::SimilarName);
        assert_eq!(issues[0].entity, "Rast");
        assert!(issues[0].detail.contains("Rust"));
    }

    #[test]
    fn test_entity_confusion_skips_known_exact() {
        // 精确匹配已知实体时不报告
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("I use Rust everyday");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_skips_ambiguous() {
        // "Will" 是易混淆词，不报告
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("Will you help me?");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_dedup() {
        // 同一名称多次出现只报告一次
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("Rast is great. Rast is fast.");
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_entity_confusion_no_false_positive() {
        // 无相似实体时不报告
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("I like Python and JavaScript");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_multiple_matches() {
        // 同时注册 "Rust" 和 "Rest"，"Rast" 与两者都相近
        let registry =
            make_registry_with(&[("Rust", EntityType::Tool), ("Rest", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("I prefer Rast");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].detail.contains("Rust"));
        assert!(issues[0].detail.contains("Rest"));
    }

    #[test]
    fn test_entity_confusion_empty_text() {
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_entity_confusion("");
        assert!(issues.is_empty());
    }

    // ── KG 矛盾检查（无 KG 配置时）────────────────────

    #[tokio::test]
    async fn test_kg_check_without_kg() {
        // 未配置知识图谱时，KG 检查返回空
        let registry = make_registry_with(&[]);
        let checker = FactChecker::new(None, registry);

        let issues = checker
            .check_kg_contradictions("Alice is Bob's mother")
            .await;
        assert!(issues.is_empty());
    }

    #[tokio::test]
    async fn test_check_text_without_kg() {
        // 未配置 KG 时，check_text 仅执行实体混淆检查
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(None, registry);

        let issues = checker.check_text("I think Rast is great").await;
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].issue_type, IssueType::SimilarName);
    }

    // ── IssueType / FactIssue 序列化测试 ──────────────

    #[test]
    fn test_issue_type_serde() {
        let issue = FactIssue {
            issue_type: IssueType::StaleFact,
            description: "test".to_string(),
            entity: "Alice".to_string(),
            detail: "expired".to_string(),
        };
        let json = serde_json::to_string(&issue).unwrap();
        assert!(json.contains("stale_fact"));
        let de: FactIssue = serde_json::from_str(&json).unwrap();
        assert_eq!(de.issue_type, IssueType::StaleFact);
        assert_eq!(de.entity, "Alice");
    }

    #[test]
    fn test_issue_type_all_variants_serde() {
        for variant in [
            IssueType::SimilarName,
            IssueType::RelationshipMismatch,
            IssueType::StaleFact,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let de: IssueType = serde_json::from_str(&json).unwrap();
            assert_eq!(de, variant);
        }
    }
}
