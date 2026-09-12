//! # 事实核查器 (Fact Checker)
//!
//! 检查 AI 响应与 entity_registry 中已知实体的相似名混淆。
//!
//! ## 检测的问题类型
//!
//! - **相似名混淆 (SimilarName)**: 文本中出现的名称与已知实体相近

use super::entities::EntityRegistry;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, OnceLock, RwLock};
use tracing::{debug, info};

// ── 正则缓存 ──────────────────────────────────────────
//
// 使用 OnceLock 延迟编译正则，进程内只编译一次。

/// 缓存大写词检测正则（用于实体混淆检测）
static RE_CAPITALIZED: OnceLock<Regex> = OnceLock::new();

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
/// 基于 entity_registry 检查文本中的实体混淆问题。
pub struct FactChecker {
    /// entity_registry
    entity_registry: Arc<RwLock<EntityRegistry>>,
}

impl FactChecker {
    /// 创建新的事实核查器
    pub fn new(entity_registry: Arc<RwLock<EntityRegistry>>) -> Self {
        Self { entity_registry }
    }

    /// 主入口：检查文本中的事实问题
    ///
    /// 执行实体混淆检查（同步）。
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
}

// ── 单元测试 ──────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::super::entities::{Entity, EntitySource, EntityType};
    use super::*;

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
        let checker = FactChecker::new(registry);

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
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("I use Rust everyday");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_skips_ambiguous() {
        // "Will" 是易混淆词，不报告
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("Will you help me?");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_dedup() {
        // 同一名称多次出现只报告一次
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("Rast is great. Rast is fast.");
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn test_entity_confusion_no_false_positive() {
        // 无相似实体时不报告
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("I like Python and JavaScript");
        assert!(issues.is_empty());
    }

    #[test]
    fn test_entity_confusion_multiple_matches() {
        // 同时注册 "Rust" 和 "Rest"，"Rast" 与两者都相近
        let registry =
            make_registry_with(&[("Rust", EntityType::Tool), ("Rest", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("I prefer Rast");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].detail.contains("Rust"));
        assert!(issues[0].detail.contains("Rest"));
    }

    #[test]
    fn test_entity_confusion_empty_text() {
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

        let issues = checker.check_entity_confusion("");
        assert!(issues.is_empty());
    }

    // ── 无 KG 时的 check_text 测试 ─────────────────────

    #[tokio::test]
    async fn test_check_text() {
        // check_text 执行实体混淆检查
        let registry = make_registry_with(&[("Rust", EntityType::Tool)]);
        let checker = FactChecker::new(registry);

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
