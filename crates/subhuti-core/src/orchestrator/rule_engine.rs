//! 任务分析规则（原三层 RuleEngine 精简后的插件位）
//!
//! 历史说明：本文件曾承载三层规则引擎（TaskAnalysisRule / DispatchRule / ExecutionRule）。
//! Graph 删除（M1c）后，框架路由统一收敛为 Orchestrator 的标签打分
//! （`relevant_actors`，与 dispatch 主链路同源），Dispatch / Execution 两层
//! 从未被主链路读取，已删除。本文件现在只保留两样东西：
//!
//! - [`TaskProfile`]：`analyze_task` 对外返回的任务画像（HTTP / MCP 查询端点的响应结构）
//! - [`TaskAnalysisRule`]：可替换的分析规则插件位——应用层可用
//!   `Orchestrator::set_analysis_rule` 注入自定义实现；未注入时，
//!   Orchestrator 用内置的「专家 tags × 输入匹配」派生画像（与路由零漂移）
//!
//! 设计红线：内置路径的 `domain_tags` 一律来自专家注册的 tags 打分，
//! 不再维护独立关键词表——独立关键词表会造成
//! 「match_expert 预览 ≠ dispatch 实际路由」的两套真相。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 任务画像（对外查询端点的响应结构）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskProfile {
    /// 命中的专家领域标签（与 dispatch 路由同源：专家 tags × 输入匹配）
    pub domain_tags: Vec<String>,
    /// 任务类型（输入侧关键词粗分类，不涉及专家知识）
    pub task_type: String,
    /// 主语（简单空格分词提取，仅供展示）
    pub subject: Option<String>,
    /// 谓语
    pub predicate: Option<String>,
    /// 宾语
    pub object: Option<String>,
}

/// 任务分析规则插件位（应用层可替换）
///
/// 通过 `Orchestrator::set_analysis_rule` 注入后，`analyze_task` / `match_expert`
/// 的画像将改用该实现产出；未注入时用内置 tags 打分派生。
#[async_trait]
pub trait TaskAnalysisRule: Send + Sync {
    /// 分析任务，返回任务画像
    fn analyze(&self, input: &str) -> crate::Result<TaskProfile>;
}

/// 任务类型粗分类（仅看输入本身，无专家知识，不会与路由漂移）
pub(crate) fn classify_task_type(input_lower: &str) -> String {
    if input_lower.contains("翻译") || input_lower.contains("translate") {
        "translate".into()
    } else if input_lower.contains("写")
        || input_lower.contains("生成")
        || input_lower.contains("create")
    {
        "generate".into()
    } else if input_lower.contains("分析") || input_lower.contains("analyze") {
        "analyze".into()
    } else if input_lower.contains("查询")
        || input_lower.contains("查")
        || input_lower.contains("search")
    {
        "query".into()
    } else if input_lower.contains("修改")
        || input_lower.contains("修复")
        || input_lower.contains("fix")
    {
        "fix".into()
    } else {
        "chat".into()
    }
}

/// 简单主谓宾提取（空格分词，仅供展示）
pub(crate) fn extract_spo(input: &str) -> (Option<String>, Option<String>, Option<String>) {
    let words: Vec<&str> = input.split_whitespace().collect();
    if words.is_empty() {
        return (None, None, None);
    }
    let subject = Some(words[0].to_string());
    let predicate = words.get(1).map(|s| s.to_string());
    let object = words.get(2).map(|s| s.to_string());
    (subject, predicate, object)
}

// ═══════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_profile_serializes() {
        let p = TaskProfile {
            domain_tags: vec!["blender".into()],
            task_type: "generate".into(),
            ..Default::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["task_type"], "generate");
        assert_eq!(v["domain_tags"][0], "blender");
    }

    #[test]
    fn classify_covers_basic_types() {
        assert_eq!(classify_task_type("帮我翻译这段话"), "translate");
        assert_eq!(classify_task_type("写一个插件"), "generate");
        assert_eq!(classify_task_type("分析一下这段代码"), "analyze");
        assert_eq!(classify_task_type("修复这个bug"), "fix");
        assert_eq!(classify_task_type("你好呀"), "chat");
    }

    #[test]
    fn spo_extracts_words() {
        let (s, p, o) = extract_spo("用 rust 写插件");
        assert_eq!(s.as_deref(), Some("用"));
        assert_eq!(p.as_deref(), Some("rust"));
        assert_eq!(o.as_deref(), Some("写插件"));
        let (s, p, o) = extract_spo("");
        assert!(s.is_none() && p.is_none() && o.is_none());
    }
}
