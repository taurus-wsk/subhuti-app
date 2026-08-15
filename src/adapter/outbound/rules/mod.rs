//! # 规则适配器（命运规则配置）
//!
//! 封装框架规则配置，实现 `TaskAnalysisRule` / `DispatchRule` / `ExecutionRule` trait。
//!
//! ## 定位
//!
//! 在六边形架构中，规则配置属于出站适配层——它是框架能力的适配器，
//! 将领域层的业务规则注入到框架的执行管线中。
//!
//! 当前使用框架提供的默认规则实现。如果未来需要自定义领域规则逻辑，
//! 可以仿照 `DomainExpert` 模式，在领域层定义领域规则 trait，
//! 然后在此层创建适配器。
//!
//! ## 与框架的关系
//!
//! - 框架提供默认规则实现（`DefaultTaskAnalysisRule` 等）
//! - 通过 `CompositionRoot::build()` 注入到 `Subhuti` 实例
//!
//! ## 示例
//!
//! 使用默认规则：
//!
//! ```rust
//! use subhuti_core::orchestrator::{DefaultTaskAnalysisRule, DefaultDispatchRule, DefaultExecutionRule};
//! use std::sync::Arc;
//!
//! let analysis_rule = Arc::new(DefaultTaskAnalysisRule::new());
//! let dispatch_rule = Arc::new(DefaultDispatchRule::new());
//! let execution_rule = Arc::new(DefaultExecutionRule::new());
//! ```

use async_trait::async_trait;
use std::sync::Arc;
use subhuti_core::orchestrator::{
    DefaultDispatchRule, DefaultExecutionRule, DefaultTaskAnalysisRule, DispatchRule,
    ExecutionRule, RuleConfig, TaskAnalysisRule, TaskProfile,
};

// ─── 规则容器 ───────────────────────────────────────────────

/// 领域规则集合
///
/// 应用层通过 `create_all_rules()` 创建，然后注入到 `Subhuti` 实例。
pub struct DomainRules {
    pub analysis_rule: Arc<dyn TaskAnalysisRule>,
    pub dispatch_rule: Arc<dyn DispatchRule>,
    pub execution_rule: Arc<dyn ExecutionRule>,
}

impl DomainRules {
    /// 使用默认规则创建
    pub fn default() -> Self {
        Self {
            analysis_rule: Arc::new(DefaultTaskAnalysisRule::new()),
            dispatch_rule: Arc::new(DefaultDispatchRule::new()),
            execution_rule: Arc::new(DefaultExecutionRule::new()),
        }
    }
}

// ─── 规则工厂 ───────────────────────────────────────────────

/// 创建所有领域规则
///
/// 在 `CompositionRoot::build()` 中调用，注册到 `Subhuti` 实例。
///
/// 新增规则只需在此函数中替换默认实现。
pub fn create_all_rules() -> DomainRules {
    // ── 开发模板：新增自定义规则在此替换 ──
    //
    // DomainRules {
    //     analysis_rule: Arc::new(MyAnalysisRule::new()),
    //     dispatch_rule: Arc::new(MyDispatchRule::new()),
    //     execution_rule: Arc::new(MyExecutionRule::new()),
    // }

    // 默认使用框架提供的规则
    DomainRules::default()
}

// ─── 自定义规则示例（可扩展）────────────────────────────────

/// 示例：自定义任务分析规则
///
/// 演示如何在领域层实现自定义规则。
/// 取消注释并在 `create_all_rules()` 中使用即可生效。
///
/// ```rust
/// // 在 create_all_rules() 中替换：
/// // analysis_rule: Arc::new(CustomAnalysisRule::new()),
/// ```
#[allow(dead_code)]
struct CustomAnalysisRule {
    // 可添加自定义配置
}

#[allow(dead_code)]
impl CustomAnalysisRule {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
#[allow(dead_code)]
impl TaskAnalysisRule for CustomAnalysisRule {
    fn analyze(&self, input: &str, config: &RuleConfig) -> subhuti_core::Result<TaskProfile> {
        // 自定义任务理解逻辑
        // 可以调用 LLM 做语义分析，或使用自定义关键词表
        DefaultTaskAnalysisRule::new().analyze(input, config)
    }
}
