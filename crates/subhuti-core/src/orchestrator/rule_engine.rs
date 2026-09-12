//! # RuleEngine - 规则执行引擎
//!
//! 框架级规则执行引擎，配合 Orchestrator（命运编织者）完成任务的执行。
//!
//! ## 概念定位
//!
//! Orchestrator 是"命运编织者"——决定走哪条路；
//! RuleEngine 是"命运执行者"——按规则之路一步步推进。
//!
//! 当 Orchestrator 选择规则策略时，本引擎负责：
//!   - 占卜：理解任务，提取领域标签和任务类型
//!   - 抉择：匹配专家，生成执行计划
//!   - 守护：执行步骤，处理超时和失败
//!
//! ## 架构
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              RuleEngine                     │
//! │  ┌───────────────────────────────────────┐  │
//! │  │ Layer 1: TaskAnalysisRule             │  │
//! │  │ 任务理解：长度校验、黑名单、领域标签、  │  │
//! │  │ 任务类型、主谓宾提取                   │  │
//! │  │ 输出: TaskProfile                      │  │
//! │  └──────────────┬────────────────────────┘  │
//! │                 │                           │
//! │  ┌──────────────▼────────────────────────┐  │
//! │  │ Layer 2: DispatchRule                 │  │
//! │  │ 执行规划：专家匹配、策略选择、         │  │
//! │  │ 过滤限制、优先级排序                   │  │
//! │  │ 输出: DispatchPlan                     │  │
//! │  └──────────────┬────────────────────────┘  │
//! │                 │                           │
//! │  ┌──────────────▼────────────────────────┐  │
//! │  │ Layer 3: ExecutionRule                │  │
//! │  │ 执行监控：步骤限制、超时检查、         │  │
//! │  │ 失败处理、结果聚合                     │  │
//! │  │ 输出: ExecutionResult                  │  │
//! │  └───────────────────────────────────────┘  │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## 设计原则
//!
//! - **框架提供机制，应用层填充规则**：三层 Rule 均为 trait，可自定义替换
//! - **零 AI 执行规划**：默认实现完全基于关键词匹配，不调用 LLM
//! - **全局约束分散**：约束配置集中在 RuleConfig，在各层分散检查

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use super::ExpertAgent;

// ═══════════════════════════════════════════════════════════
// 数据结构
// ═══════════════════════════════════════════════════════════

/// 调度策略
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchStrategy {
    /// 单专家直连
    SimpleDispatch,
    /// 串行流水线
    Pipeline,
}

/// 结果聚合策略
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ResultStrategy {
    /// 取最后一个专家的输出
    #[default]
    TakeLast,
    /// 取第一个专家的输出
    TakeFirst,
    /// 合并所有专家输出
    MergeAll,
}

/// 任务画像（Layer 1 输出）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskProfile {
    /// 领域标签（关键词提取）
    pub domain_tags: Vec<String>,
    /// 任务类型
    pub task_type: String,
    /// 主语
    pub subject: Option<String>,
    /// 谓语
    pub predicate: Option<String>,
    /// 宾语
    pub object: Option<String>,
}

/// 调度计划（Layer 2 输出）
#[derive(Debug, Clone)]
pub struct DispatchPlan {
    /// 策略
    pub strategy: DispatchStrategy,
    /// 执行步骤（已排序、已过滤）
    pub steps: Vec<Step>,
}

/// 执行步骤
#[derive(Debug, Clone)]
pub struct Step {
    /// 专家 ID
    pub agent_id: String,
    /// 输入来源（上一步输出 or 原始输入）
    pub use_previous_output: bool,
}

/// 执行结果（Layer 3 输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// 最终输出
    pub output: String,
    /// 执行链路
    pub expert_chain: Vec<String>,
    /// 是否成功
    pub success: bool,
    /// 策略名称
    pub strategy: String,
}

// ═══════════════════════════════════════════════════════════
// 规则配置
// ═══════════════════════════════════════════════════════════

/// 全局规则配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleConfig {
    // ── Layer 1 约束 ──
    /// 任务最大长度
    pub max_task_length: usize,
    /// 禁用关键词
    pub blacklist_keywords: Vec<String>,

    // ── Layer 2 约束 ──
    /// 最大专家数量
    pub max_expert_count: usize,
    /// 专家白名单（空 = 不限制）
    pub allowed_agents: Vec<String>,
    /// 专家黑名单
    pub denied_agents: Vec<String>,
    /// 触发 Pipeline 的匹配阈值
    pub pipeline_threshold: usize,

    // ── Layer 3 约束 ──
    /// 最大执行步骤数
    pub max_steps: usize,
    /// 总执行超时（ms）
    pub max_execution_time_ms: u64,
    /// 单步执行超时（ms）
    pub per_step_timeout_ms: u64,
    /// 是否传递完整上下文
    pub pass_full_context: bool,
    /// 失败是否继续
    pub continue_on_failure: bool,
    /// 结果聚合策略
    pub result_strategy: ResultStrategy,
}

impl Default for RuleConfig {
    fn default() -> Self {
        Self {
            max_task_length: 10000,
            blacklist_keywords: vec!["暴力".into(), "攻击".into(), "色情".into()],

            max_expert_count: 10,
            allowed_agents: Vec::new(),
            denied_agents: Vec::new(),
            pipeline_threshold: 2,

            max_steps: 10,
            max_execution_time_ms: 300_000,
            per_step_timeout_ms: 60_000,
            pass_full_context: false,
            continue_on_failure: false,
            result_strategy: ResultStrategy::default(),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Layer 1: TaskAnalysisRule
// ═══════════════════════════════════════════════════════════

/// 任务分析规则（Layer 1）
///
/// 负责任务理解、结构化提取。
/// 框架提供默认实现，应用层可替换。
#[async_trait]
pub trait TaskAnalysisRule: Send + Sync {
    /// 分析任务，返回任务画像
    fn analyze(&self, input: &str, config: &RuleConfig) -> crate::Result<TaskProfile>;
}

/// 默认任务分析规则：基于关键词匹配
pub struct DefaultTaskAnalysisRule;

impl DefaultTaskAnalysisRule {
    pub fn new() -> Self {
        Self
    }

    fn extract_domain_tags(input_lower: &str) -> Vec<String> {
        let domain_keywords: &[(&str, &str)] = &[
            ("编程", "coding"),
            ("代码", "coding"),
            ("rust", "coding"),
            ("开发", "coding"),
            ("bug", "coding"),
            ("程序", "coding"),
            ("心情", "psychology"),
            ("心理", "psychology"),
            ("情绪", "psychology"),
            ("咨询", "psychology"),
            ("焦虑", "psychology"),
            ("压力", "psychology"),
            ("天气", "weather"),
            ("温度", "weather"),
            ("下雨", "weather"),
            ("气象", "weather"),
            ("翻译", "translate"),
            ("translate", "translate"),
            ("写作", "writing"),
            ("write", "writing"),
            ("审查", "review"),
            ("评审", "review"),
            ("审核", "review"),
            ("review", "review"),
            ("blender", "blender"),
            ("3d", "blender"),
            ("动画", "blender"),
            ("建模", "blender"),
            ("渲染", "blender"),
            ("材质", "blender"),
            ("节点", "blender"),
            ("粒子", "blender"),
            ("python", "blender"),
        ];

        let mut tags: Vec<String> = domain_keywords
            .iter()
            .filter(|(kw, _)| input_lower.contains(kw))
            .map(|(_, tag)| tag.to_string())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    fn extract_task_type(input_lower: &str) -> String {
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

    /// 简单主谓宾提取
    fn extract_spo(input: &str) -> (Option<String>, Option<String>, Option<String>) {
        let words: Vec<&str> = input.split_whitespace().collect();
        if words.is_empty() {
            return (None, None, None);
        }
        let subject = words.first().map(|s| s.to_string());
        let predicate = if words.len() >= 2 {
            Some(words[1].to_string())
        } else {
            None
        };
        let object = if words.len() >= 3 {
            Some(words[2].to_string())
        } else {
            None
        };
        (subject, predicate, object)
    }
}

impl Default for DefaultTaskAnalysisRule {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TaskAnalysisRule for DefaultTaskAnalysisRule {
    fn analyze(&self, input: &str, config: &RuleConfig) -> crate::Result<TaskProfile> {
        // 1. 长度校验
        if input.len() > config.max_task_length {
            return Err(crate::Error::Orchestrator(format!(
                "任务长度 {} 超过最大限制 {}",
                input.len(),
                config.max_task_length
            )));
        }

        // 2. 黑名单校验
        let input_lower = input.to_lowercase();
        for kw in &config.blacklist_keywords {
            if input_lower.contains(&kw.to_lowercase()) {
                return Err(crate::Error::Orchestrator(format!(
                    "任务包含禁用关键词: {}",
                    kw
                )));
            }
        }

        // 3. 领域标签提取
        let domain_tags = Self::extract_domain_tags(&input_lower);
        tracing::debug!("[任务理解·Layer1] 领域标签: {:?}", domain_tags);

        // 4. 任务类型识别
        let task_type = Self::extract_task_type(&input_lower);
        tracing::debug!("[任务理解·Layer1] 任务类型: {}", task_type);

        // 5. 主谓宾提取
        let (subject, predicate, object) = Self::extract_spo(input);

        Ok(TaskProfile {
            domain_tags,
            task_type,
            subject,
            predicate,
            object,
        })
    }
}

// ═══════════════════════════════════════════════════════════
// Layer 2: DispatchRule
// ═══════════════════════════════════════════════════════════

/// 调度决策规则（Layer 2）
///
/// 负责专家匹配、策略决策、过滤限制、优先级排序。
#[async_trait]
pub trait DispatchRule: Send + Sync {
    /// 决策调度计划
    fn decide(
        &self,
        profile: &TaskProfile,
        agents: &[Arc<dyn ExpertAgent>],
        config: &RuleConfig,
    ) -> crate::Result<DispatchPlan>;
}

/// 默认调度规则
pub struct DefaultDispatchRule;

impl DefaultDispatchRule {
    pub fn new() -> Self {
        Self
    }

    fn match_experts(
        profile: &TaskProfile,
        agents: &[Arc<dyn ExpertAgent>],
    ) -> Vec<(Arc<dyn ExpertAgent>, u32)> {
        let mut matched: Vec<(Arc<dyn ExpertAgent>, u32)> = Vec::new();

        for agent in agents {
            let mut score = 0u32;
            for tag in agent.tags() {
                let tag_lower = tag.to_lowercase();
                if profile.domain_tags.iter().any(|dt| dt == &tag_lower) {
                    score += 1;
                }
            }
            if score > 0 {
                matched.push((agent.clone(), score));
            }
        }

        matched.sort_by_key(|m| std::cmp::Reverse(m.1));
        matched
    }

    fn filter_and_limit(
        matched: Vec<(Arc<dyn ExpertAgent>, u32)>,
        config: &RuleConfig,
    ) -> Vec<Arc<dyn ExpertAgent>> {
        let filtered: Vec<Arc<dyn ExpertAgent>> = matched
            .into_iter()
            .map(|(a, _)| a)
            .filter(|a| {
                let id = a.id();
                if !config.allowed_agents.is_empty()
                    && !config.allowed_agents.contains(&id.to_string())
                {
                    return false;
                }
                if config.denied_agents.contains(&id.to_string()) {
                    return false;
                }
                true
            })
            .take(config.max_expert_count)
            .collect();
        filtered
    }
}

impl Default for DefaultDispatchRule {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DispatchRule for DefaultDispatchRule {
    fn decide(
        &self,
        profile: &TaskProfile,
        agents: &[Arc<dyn ExpertAgent>],
        config: &RuleConfig,
    ) -> crate::Result<DispatchPlan> {
        let matched = Self::match_experts(profile, agents);
        tracing::debug!("[调度策略·Layer2] 匹配到 {} 个专家", matched.len());

        let filtered = Self::filter_and_limit(matched, config);
        tracing::debug!("[调度策略·Layer2] 过滤后 {} 个专家", filtered.len());

        if filtered.is_empty() {
            return Ok(DispatchPlan {
                strategy: DispatchStrategy::SimpleDispatch,
                steps: Vec::new(),
            });
        }

        let strategy =
            if filtered.len() >= config.pipeline_threshold || profile.domain_tags.len() >= 2 {
                DispatchStrategy::Pipeline
            } else {
                DispatchStrategy::SimpleDispatch
            };
        tracing::debug!("[调度策略·Layer2] 策略: {:?}", strategy);

        let steps: Vec<Step> = filtered
            .iter()
            .enumerate()
            .map(|(i, agent)| Step {
                agent_id: agent.id().to_string(),
                use_previous_output: !config.pass_full_context && i > 0,
            })
            .collect();

        Ok(DispatchPlan { strategy, steps })
    }
}

// ═══════════════════════════════════════════════════════════
// Layer 3: ExecutionRule
// ═══════════════════════════════════════════════════════════

/// 执行监控规则（Layer 3）
///
/// 负责步骤限制、超时检查、失败处理、结果聚合。
#[async_trait]
pub trait ExecutionRule: Send + Sync {
    /// 检查步骤数是否超限
    fn check_max_steps(&self, current_step: usize, config: &RuleConfig) -> crate::Result<()>;

    /// 检查总超时
    fn check_timeout(&self, elapsed: Duration, config: &RuleConfig) -> crate::Result<()>;

    /// 单步超时
    fn per_step_timeout(&self, config: &RuleConfig) -> Duration;

    /// 失败后是否继续
    fn should_continue(&self, config: &RuleConfig) -> bool;

    /// 聚合结果
    fn merge_results(&self, results: &[String], config: &RuleConfig) -> String;
}

/// 默认执行规则
pub struct DefaultExecutionRule;

impl DefaultExecutionRule {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultExecutionRule {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionRule for DefaultExecutionRule {
    fn check_max_steps(&self, current_step: usize, config: &RuleConfig) -> crate::Result<()> {
        if current_step >= config.max_steps {
            return Err(crate::Error::Orchestrator(format!(
                "执行步骤数 {} 达到最大限制 {}",
                current_step, config.max_steps
            )));
        }
        Ok(())
    }

    fn check_timeout(&self, elapsed: Duration, config: &RuleConfig) -> crate::Result<()> {
        let elapsed_ms = elapsed.as_millis() as u64;
        if elapsed_ms >= config.max_execution_time_ms {
            return Err(crate::Error::Orchestrator(format!(
                "总执行超时: {}ms >= {}ms",
                elapsed_ms, config.max_execution_time_ms
            )));
        }
        Ok(())
    }

    fn per_step_timeout(&self, config: &RuleConfig) -> Duration {
        Duration::from_millis(config.per_step_timeout_ms)
    }

    fn should_continue(&self, config: &RuleConfig) -> bool {
        config.continue_on_failure
    }

    fn merge_results(&self, results: &[String], config: &RuleConfig) -> String {
        if results.is_empty() {
            return String::new();
        }
        match config.result_strategy {
            ResultStrategy::TakeLast => results.last().cloned().unwrap_or_default(),
            ResultStrategy::TakeFirst => results.first().cloned().unwrap_or_default(),
            ResultStrategy::MergeAll => results
                .iter()
                .enumerate()
                .map(|(i, r)| format!("[步骤{}] {}", i + 1, r))
                .collect::<Vec<_>>()
                .join("\n\n"),
        }
    }
}

// ═══════════════════════════════════════════════════════════
// RuleEngine
// ═══════════════════════════════════════════════════════════

/// 规则引擎 - 协调三层规则
pub struct RuleEngine {
    config: RuleConfig,
    /// 三条规则均为 `RwLock<Arc<dyn ..>>`：
    /// 支持运行时热替换（`&self`），同时让 Orchestrator 可被并发共享，
    /// 不必为「改规则」而在编排主链路上加排他锁。
    analysis_rule: RwLock<Arc<dyn TaskAnalysisRule>>,
    dispatch_rule: RwLock<Arc<dyn DispatchRule>>,
    execution_rule: RwLock<Arc<dyn ExecutionRule>>,
}

impl RuleEngine {
    /// 创建规则引擎（使用默认规则）
    pub fn new(config: RuleConfig) -> Self {
        Self {
            config,
            analysis_rule: RwLock::new(Arc::new(DefaultTaskAnalysisRule::new())),
            dispatch_rule: RwLock::new(Arc::new(DefaultDispatchRule::new())),
            execution_rule: RwLock::new(Arc::new(DefaultExecutionRule::new())),
        }
    }

    /// 使用默认配置创建
    pub fn with_defaults() -> Self {
        Self::new(RuleConfig::default())
    }

    /// 替换任务分析规则
    pub fn with_analysis_rule(mut self, rule: Arc<dyn TaskAnalysisRule>) -> Self {
        self.analysis_rule = RwLock::new(rule);
        self
    }

    /// 替换调度规则
    pub fn with_dispatch_rule(mut self, rule: Arc<dyn DispatchRule>) -> Self {
        self.dispatch_rule = RwLock::new(rule);
        self
    }

    /// 替换执行规则
    pub fn with_execution_rule(mut self, rule: Arc<dyn ExecutionRule>) -> Self {
        self.execution_rule = RwLock::new(rule);
        self
    }

    // ── 规则读取（读锁快照，锁中毒时回退默认规则）──

    fn current_analysis_rule(&self) -> Arc<dyn TaskAnalysisRule> {
        match self.analysis_rule.read() {
            Ok(guard) => guard.clone(),
            Err(e) => {
                tracing::error!("RuleEngine.analysis_rule 读锁中毒，回退默认规则: {}", e);
                Arc::new(DefaultTaskAnalysisRule::new())
            }
        }
    }

    fn current_dispatch_rule(&self) -> Arc<dyn DispatchRule> {
        match self.dispatch_rule.read() {
            Ok(guard) => guard.clone(),
            Err(e) => {
                tracing::error!("RuleEngine.dispatch_rule 读锁中毒，回退默认规则: {}", e);
                Arc::new(DefaultDispatchRule::new())
            }
        }
    }

    fn current_execution_rule(&self) -> Arc<dyn ExecutionRule> {
        match self.execution_rule.read() {
            Ok(guard) => guard.clone(),
            Err(e) => {
                tracing::error!("RuleEngine.execution_rule 读锁中毒，回退默认规则: {}", e);
                Arc::new(DefaultExecutionRule::new())
            }
        }
    }

    fn replace_analysis_rule(&self, rule: Arc<dyn TaskAnalysisRule>) {
        if let Ok(mut guard) = self.analysis_rule.write() {
            *guard = rule;
        }
    }

    fn replace_dispatch_rule(&self, rule: Arc<dyn DispatchRule>) {
        if let Ok(mut guard) = self.dispatch_rule.write() {
            *guard = rule;
        }
    }

    fn replace_execution_rule(&self, rule: Arc<dyn ExecutionRule>) {
        if let Ok(mut guard) = self.execution_rule.write() {
            *guard = rule;
        }
    }

    // ── 运行时替换（不消费 self，供应用层在启动后替换）──

    /// 运行时替换任务分析规则（Layer 1）
    pub fn set_analysis_rule(&self, rule: Arc<dyn TaskAnalysisRule>) {
        self.replace_analysis_rule(rule);
    }

    /// 运行时替换调度决策规则（Layer 2）
    pub fn set_dispatch_rule(&self, rule: Arc<dyn DispatchRule>) {
        self.replace_dispatch_rule(rule);
    }

    /// 运行时替换执行监控规则（Layer 3）
    pub fn set_execution_rule(&self, rule: Arc<dyn ExecutionRule>) {
        self.replace_execution_rule(rule);
    }

    /// 获取配置引用
    pub fn config(&self) -> &RuleConfig {
        &self.config
    }

    /// 获取执行规则引用
    /// 获取当前执行规则（返回 Arc 克隆，不再暴露内部引用）
    pub fn execution_rule(&self) -> Arc<dyn ExecutionRule> {
        self.current_execution_rule()
    }

    // ── Layer 1: 任务分析 ──

    pub fn analyze_task(&self, input: &str) -> crate::Result<TaskProfile> {
        tracing::info!("[任务理解·Layer1] 开始分析任务");
        let profile = self.current_analysis_rule().analyze(input, &self.config)?;
        tracing::info!(
            "[任务理解·Layer1] 分析完成: domain_tags={:?}, task_type={}",
            profile.domain_tags,
            profile.task_type
        );
        Ok(profile)
    }

    // ── Layer 2: 调度决策 ──

    pub fn decide_strategy(
        &self,
        profile: &TaskProfile,
        agents: &[Arc<dyn ExpertAgent>],
    ) -> crate::Result<DispatchPlan> {
        let plan = self
            .current_dispatch_rule()
            .decide(profile, agents, &self.config)?;
        tracing::info!(
            "[调度策略·Layer2] 策略: {:?}, 步骤数: {}",
            plan.strategy,
            plan.steps.len()
        );
        Ok(plan)
    }

    // ── Layer 3: 执行监控（由 Orchestrator 在执行循环中调用）──

    pub fn check_max_steps(&self, current_step: usize) -> crate::Result<()> {
        self.current_execution_rule()
            .check_max_steps(current_step, &self.config)
    }

    pub fn check_timeout(&self, elapsed: Duration) -> crate::Result<()> {
        self.current_execution_rule()
            .check_timeout(elapsed, &self.config)
    }

    pub fn per_step_timeout(&self) -> Duration {
        self.current_execution_rule().per_step_timeout(&self.config)
    }

    pub fn should_continue(&self) -> bool {
        self.current_execution_rule().should_continue(&self.config)
    }

    pub fn merge_results(&self, results: &[String]) -> String {
        self.current_execution_rule()
            .merge_results(results, &self.config)
    }
}

impl std::fmt::Debug for RuleEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuleEngine")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

// ═══════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{AgentContext, ExpertState};
    use async_trait::async_trait;

    // 测试用假专家
    struct MockExpert {
        id: String,
        name: String,
        tags: Vec<String>,
    }

    impl MockExpert {
        fn new(id: &str, name: &str, tags: Vec<&str>) -> Self {
            Self {
                id: id.to_string(),
                name: name.to_string(),
                tags: tags.into_iter().map(String::from).collect(),
            }
        }
    }

    #[async_trait]
    impl ExpertAgent for MockExpert {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> &str {
            &self.name
        }
        fn tags(&self) -> &[String] {
            &self.tags
        }
        async fn run(
            &self,
            _ctx: &mut AgentContext,
            _state: &ExpertState,
        ) -> crate::Result<String> {
            Ok(format!("result_from_{}", self.id))
        }
    }

    #[test]
    fn test_task_analysis_basic() {
        let rule = DefaultTaskAnalysisRule::new();
        let config = RuleConfig::default();

        let profile = rule.analyze("帮我写一段 Rust 代码", &config).unwrap();
        assert!(profile.domain_tags.contains(&"coding".to_string()));
        assert_eq!(profile.task_type, "generate");
    }

    #[test]
    fn test_task_analysis_blacklist() {
        let rule = DefaultTaskAnalysisRule::new();
        let config = RuleConfig::default();

        let result = rule.analyze("暴力攻击", &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("禁用关键词"));
    }

    #[test]
    fn test_task_analysis_length() {
        let rule = DefaultTaskAnalysisRule::new();
        let config = RuleConfig {
            max_task_length: 5,
            ..Default::default()
        };

        let result = rule.analyze("这是一段很长的任务描述", &config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("长度"));
    }

    #[test]
    fn test_dispatch_simple() {
        let rule = DefaultDispatchRule::new();
        let config = RuleConfig::default();

        let profile = TaskProfile {
            domain_tags: vec!["coding".into()],
            task_type: "generate".into(),
            ..Default::default()
        };

        let agents: Vec<Arc<dyn ExpertAgent>> = vec![Arc::new(MockExpert::new(
            "coding",
            "编程专家",
            vec!["coding"],
        ))];

        let plan = rule.decide(&profile, &agents, &config).unwrap();
        assert_eq!(plan.strategy, DispatchStrategy::SimpleDispatch);
        assert_eq!(plan.steps.len(), 1);
    }

    #[test]
    fn test_dispatch_pipeline() {
        let rule = DefaultDispatchRule::new();
        let config = RuleConfig {
            pipeline_threshold: 2,
            ..Default::default()
        };

        let profile = TaskProfile {
            domain_tags: vec!["coding".into(), "psychology".into()],
            task_type: "analyze".into(),
            ..Default::default()
        };

        let agents: Vec<Arc<dyn ExpertAgent>> = vec![
            Arc::new(MockExpert::new("coding", "编程专家", vec!["coding"])),
            Arc::new(MockExpert::new(
                "psychology",
                "心理咨询师",
                vec!["psychology"],
            )),
        ];

        let plan = rule.decide(&profile, &agents, &config).unwrap();
        assert_eq!(plan.strategy, DispatchStrategy::Pipeline);
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.steps[1].use_previous_output);
    }

    #[test]
    fn test_dispatch_filter_denied() {
        let rule = DefaultDispatchRule::new();
        let config = RuleConfig {
            denied_agents: vec!["coding".into()],
            ..Default::default()
        };

        let profile = TaskProfile {
            domain_tags: vec!["coding".into()],
            task_type: "generate".into(),
            ..Default::default()
        };

        let agents: Vec<Arc<dyn ExpertAgent>> = vec![Arc::new(MockExpert::new(
            "coding",
            "编程专家",
            vec!["coding"],
        ))];

        let plan = rule.decide(&profile, &agents, &config).unwrap();
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn test_execution_check_max_steps() {
        let rule = DefaultExecutionRule::new();
        let config = RuleConfig {
            max_steps: 3,
            ..Default::default()
        };

        assert!(rule.check_max_steps(2, &config).is_ok());
        assert!(rule.check_max_steps(3, &config).is_err());
    }

    #[test]
    fn test_execution_check_timeout() {
        let rule = DefaultExecutionRule::new();
        let config = RuleConfig {
            max_execution_time_ms: 1000,
            ..Default::default()
        };

        assert!(rule
            .check_timeout(Duration::from_millis(500), &config)
            .is_ok());
        assert!(rule
            .check_timeout(Duration::from_millis(1000), &config)
            .is_err());
    }

    #[test]
    fn test_merge_results_take_last() {
        let rule = DefaultExecutionRule::new();
        let config = RuleConfig {
            result_strategy: ResultStrategy::TakeLast,
            ..Default::default()
        };

        let results = vec!["first".into(), "second".into(), "third".into()];
        assert_eq!(rule.merge_results(&results, &config), "third");
    }

    #[test]
    fn test_merge_results_take_first() {
        let rule = DefaultExecutionRule::new();
        let config = RuleConfig {
            result_strategy: ResultStrategy::TakeFirst,
            ..Default::default()
        };

        let results = vec!["first".into(), "second".into()];
        assert_eq!(rule.merge_results(&results, &config), "first");
    }

    #[test]
    fn test_merge_results_merge_all() {
        let rule = DefaultExecutionRule::new();
        let config = RuleConfig {
            result_strategy: ResultStrategy::MergeAll,
            ..Default::default()
        };

        let results = vec!["first".into(), "second".into()];
        let merged = rule.merge_results(&results, &config);
        assert!(merged.contains("first"));
        assert!(merged.contains("second"));
    }

    #[test]
    fn test_rule_engine_full_flow() {
        let engine = RuleEngine::with_defaults();

        let profile = engine.analyze_task("帮我写一段 Rust 代码").unwrap();
        assert!(!profile.domain_tags.is_empty());

        let agents: Vec<Arc<dyn ExpertAgent>> = vec![Arc::new(MockExpert::new(
            "coding",
            "编程专家",
            vec!["coding"],
        ))];

        let plan = engine.decide_strategy(&profile, &agents).unwrap();
        assert!(!plan.steps.is_empty());
    }
}
