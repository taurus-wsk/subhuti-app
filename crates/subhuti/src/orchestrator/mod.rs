//! # 多 Agent 协作层 (Orchestrator)
//!
//! ## 核心架构
//! 调度器负责：注册专家、调度排序、责任链串行执行
//!
//! ## 核心流程
//! 用户任务 → 任务理解 → 调度策略 → 执行监控 → 结果
//!
//! ## 规则引擎三层架构（每层内置全局约束）
//! 1. TaskAnalysisRule: 任务理解（内置输入长度、黑名单约束）
//! 2. DispatchRule: 调度策略（内置专家数量、专家白黑名单约束）
//! 3. ExecutionRule: 执行监控（内置超时、步骤数约束）

use crate::context::TokenStats;
use crate::runtime::{Message, Role, Runtime};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ─── 类型别名 ──────────────────────────────────────────────

pub type AgentId = String;
pub type CtxId = String;

// ─── 上下文存储（享元模式）──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextData {
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct ContextStore {
    store: HashMap<CtxId, ContextData>,
}

impl ContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put(&mut self, data: ContextData) -> CtxId {
        let ctx_id = Uuid::new_v4().to_string();
        self.store.insert(ctx_id.clone(), data);
        ctx_id
    }

    pub fn get(&self, ctx_id: &str) -> Option<&ContextData> {
        self.store.get(ctx_id)
    }

    pub fn remove(&mut self, ctx_id: &str) -> Option<ContextData> {
        self.store.remove(ctx_id)
    }
}

// ─── 任务画像（结构化任务理解结果）─────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub enum TaskType {
    Create,
    Query,
    Analyze,
    Optimize,
    Debug,
    Learn,
    Consult,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct TaskProfile {
    pub input: String,
    pub domain_tags: Vec<String>,
    pub task_type: TaskType,
    pub subject: Option<String>,
    pub predicate: Option<String>,
    pub object: Option<String>,
    pub target_expert: Option<String>,
}

// ─── 全局规则配置 ──────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct RuleConfig {
    pub max_task_length: usize,
    pub max_expert_count: usize,
    pub max_execution_time_ms: u64,
    pub max_steps: usize,
    pub per_step_timeout_ms: u64,
    pub blacklist_keywords: Vec<String>,
    pub allowed_agents: Vec<AgentId>,
    pub denied_agents: Vec<AgentId>,
    pub pipeline_threshold: usize,
    pub pass_full_context: bool,
    pub continue_on_failure: bool,
    pub result_strategy: ResultStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultStrategy {
    #[default]
    TakeLast,
    MergeAll,
    TakeFirst,
}

// ─── 第一层：任务理解规则 (TaskAnalysisRule) ────────────────

pub trait TaskAnalysisRule: Send + Sync + std::fmt::Debug {
    fn analyze(&self, input: &str, config: &RuleConfig) -> Result<TaskProfile>;
}

#[derive(Debug)]
pub struct KeywordBasedTaskAnalysisRule;

impl KeywordBasedTaskAnalysisRule {
    const DOMAIN_KEYWORDS: &'static [(&'static str, &'static str)] = &[
        ("编程", "tech"),
        ("代码", "tech"),
        ("开发", "tech"),
        ("bug", "tech"),
        ("rust", "tech"),
        ("python", "tech"),
        ("java", "tech"),
        ("javascript", "tech"),
        ("天气", "weather"),
        ("温度", "weather"),
        ("下雨", "weather"),
        ("气象", "weather"),
        ("心情", "psychology"),
        ("心理", "psychology"),
        ("咨询", "psychology"),
        ("情绪", "psychology"),
        ("情感", "psychology"),
        ("健康", "health"),
        ("疾病", "health"),
        ("医生", "health"),
        ("健身", "health"),
        ("学习", "education"),
        ("课程", "education"),
        ("教学", "education"),
        ("考试", "education"),
        ("写文章", "writing"),
        ("写报告", "writing"),
        ("总结", "writing"),
        ("翻译", "writing"),
        ("设计", "design"),
        ("画图", "design"),
        ("UI", "design"),
        ("界面", "design"),
        ("数据分析", "data"),
        ("统计", "data"),
        ("可视化", "data"),
        ("报表", "data"),
    ];

    const TASK_TYPE_KEYWORDS: &'static [(&'static str, TaskType)] = &[
        ("创建", TaskType::Create),
        ("制作", TaskType::Create),
        ("生成", TaskType::Create),
        ("写", TaskType::Create),
        ("做", TaskType::Create),
        ("构建", TaskType::Create),
        ("查", TaskType::Query),
        ("查询", TaskType::Query),
        ("搜索", TaskType::Query),
        ("看看", TaskType::Query),
        ("了解", TaskType::Query),
        ("分析", TaskType::Analyze),
        ("评估", TaskType::Analyze),
        ("判断", TaskType::Analyze),
        ("检查", TaskType::Analyze),
        ("优化", TaskType::Optimize),
        ("改进", TaskType::Optimize),
        ("提升", TaskType::Optimize),
        ("增强", TaskType::Optimize),
        ("修复", TaskType::Debug),
        ("调试", TaskType::Debug),
        ("解决", TaskType::Debug),
        ("排除", TaskType::Debug),
        ("学习", TaskType::Learn),
        ("教我", TaskType::Learn),
        ("教程", TaskType::Learn),
        ("入门", TaskType::Learn),
        ("咨询", TaskType::Consult),
        ("建议", TaskType::Consult),
        ("意见", TaskType::Consult),
        ("帮忙", TaskType::Consult),
    ];
}

impl TaskAnalysisRule for KeywordBasedTaskAnalysisRule {
    fn analyze(&self, input: &str, config: &RuleConfig) -> Result<TaskProfile> {
        tracing::info!("═══════════════════════════════════════════");
        tracing::info!("【任务理解·Layer 1】开始分析任务");
        tracing::info!("【任务理解·Layer 1】原始输入: {}", input);
        tracing::info!(
            "【任务理解·Layer 1】全局约束: max_task_length={}, blacklist_keywords={:?}",
            config.max_task_length,
            config.blacklist_keywords
        );

        tracing::info!("【任务理解·1/5】长度校验开始");
        if input.len() > config.max_task_length {
            tracing::error!(
                "【任务理解·1/5】长度校验失败: {} > {}",
                input.len(),
                config.max_task_length
            );
            return Err(anyhow!(
                "任务长度超过限制: {} > {}",
                input.len(),
                config.max_task_length
            ));
        }
        tracing::info!(
            "【任务理解·1/5】长度校验通过: {} <= {}",
            input.len(),
            config.max_task_length
        );

        let input_lower = input.to_lowercase();
        tracing::info!("【任务理解·2/5】黑名单校验开始");
        for keyword in &config.blacklist_keywords {
            if input_lower.contains(&keyword.to_lowercase()) {
                tracing::error!("【任务理解·2/5】黑名单校验失败: 命中关键词 '{}'", keyword);
                return Err(anyhow!("任务包含禁用关键词: {}", keyword));
            }
        }
        tracing::info!("【任务理解·2/5】黑名单校验通过: 未命中任何禁用词");

        let mut profile = TaskProfile {
            input: input.to_string(),
            ..Default::default()
        };

        tracing::info!("【任务理解·3/5】领域标签提取开始");
        let mut domain_tags = Vec::new();
        for (keyword, tag) in Self::DOMAIN_KEYWORDS {
            if input_lower.contains(keyword) {
                domain_tags.push(tag.to_string());
                tracing::debug!("【任务理解·3/5】匹配领域关键词: '{}' -> {}", keyword, tag);
            }
        }
        profile.domain_tags = domain_tags;
        tracing::info!(
            "【任务理解·3/5】领域标签提取完成: {:?} (共{}个)",
            profile.domain_tags,
            profile.domain_tags.len()
        );

        tracing::info!("【任务理解·4/5】任务类型识别开始");
        for (keyword, task_type) in Self::TASK_TYPE_KEYWORDS {
            if input_lower.contains(keyword) {
                profile.task_type = *task_type;
                tracing::info!(
                    "【任务理解·4/5】匹配任务类型: 关键词 '{}' -> {:?}",
                    keyword,
                    task_type
                );
                break;
            }
        }
        tracing::info!("【任务理解·4/5】任务类型识别结果: {:?}", profile.task_type);

        tracing::info!("【任务理解·5/5】主谓宾提取开始");
        let (subject, predicate, object) = Self::extract_spo(input);
        profile.subject = subject;
        profile.predicate = predicate;
        profile.object = object;
        tracing::info!(
            "【任务理解·5/5】主谓宾提取结果: subject={:?}, predicate={:?}, object={:?}",
            profile.subject,
            profile.predicate,
            profile.object
        );

        tracing::info!("【任务理解·Layer 1】分析完成 ✓");
        tracing::info!("  domain_tags: {:?}", profile.domain_tags);
        tracing::info!("  task_type:   {:?}", profile.task_type);
        tracing::info!("  subject:     {:?}", profile.subject);
        tracing::info!("  predicate:   {:?}", profile.predicate);
        tracing::info!("  object:      {:?}", profile.object);
        tracing::info!("═══════════════════════════════════════════");

        Ok(profile)
    }
}

impl KeywordBasedTaskAnalysisRule {
    fn extract_spo(input: &str) -> (Option<String>, Option<String>, Option<String>) {
        let mut subject = None;
        let mut predicate = None;
        let mut object = None;

        let words: Vec<&str> = input
            .split(|c: char| c.is_whitespace() || c == '，' || c == '。' || c == '？' || c == '！')
            .filter(|s| !s.is_empty())
            .collect();

        if words.is_empty() {
            return (subject, predicate, object);
        }

        if words.len() >= 2 {
            predicate = Some(words[1].to_string());
        }

        if words.len() >= 3 {
            object = Some(words[2..].join(""));
        }

        if !words.is_empty() {
            subject = Some(words[0].to_string());
        }

        (subject, predicate, object)
    }
}

// ─── 第二层：调度策略规则 (DispatchRule) ────────────────────

pub trait DispatchRule: Send + Sync + std::fmt::Debug {
    fn decide_strategy(
        &self,
        profile: &TaskProfile,
        matched_count: usize,
        config: &RuleConfig,
    ) -> DispatchStrategy;
    fn filter_and_limit(
        &self,
        agent_ids: Vec<AgentId>,
        profile: &TaskProfile,
        config: &RuleConfig,
    ) -> Vec<AgentId>;
}

#[derive(Debug)]
pub struct DomainBasedDispatchRule;

impl DispatchRule for DomainBasedDispatchRule {
    fn decide_strategy(
        &self,
        profile: &TaskProfile,
        matched_count: usize,
        config: &RuleConfig,
    ) -> DispatchStrategy {
        tracing::info!("═══════════════════════════════════════════");
        tracing::info!("【调度策略·Layer 2】开始策略决策");
        tracing::info!(
            "【调度策略·Layer 2】全局约束: pipeline_threshold={}, max_expert_count={}",
            config.pipeline_threshold,
            config.max_expert_count
        );
        tracing::info!(
            "【调度策略·Layer 2】输入参数: matched_count={}, domain_tags={:?}",
            matched_count,
            profile.domain_tags
        );

        let reason = if matched_count >= config.pipeline_threshold {
            format!(
                "匹配专家数 {} >= 阈值 {}",
                matched_count, config.pipeline_threshold
            )
        } else if profile.domain_tags.len() >= 2 {
            format!("领域标签数 {} >= 2", profile.domain_tags.len())
        } else {
            format!(
                "匹配专家数 {} < 阈值 {} 且领域标签数 {} < 2",
                matched_count,
                config.pipeline_threshold,
                profile.domain_tags.len()
            )
        };

        let strategy =
            if matched_count >= config.pipeline_threshold || profile.domain_tags.len() >= 2 {
                DispatchStrategy::Pipeline
            } else {
                DispatchStrategy::SimpleDispatch
            };

        tracing::info!("【调度策略·Layer 2】决策结果: {:?}", strategy);
        tracing::info!("【调度策略·Layer 2】决策原因: {}", reason);
        tracing::info!("═══════════════════════════════════════════");

        strategy
    }

    fn filter_and_limit(
        &self,
        agent_ids: Vec<AgentId>,
        _profile: &TaskProfile,
        config: &RuleConfig,
    ) -> Vec<AgentId> {
        tracing::info!("═══════════════════════════════════════════");
        tracing::info!("【调度策略·专家过滤】开始过滤专家");
        tracing::info!("【调度策略·专家过滤】全局约束: allowed_agents={:?}, denied_agents={:?}, max_expert_count={}",
            config.allowed_agents, config.denied_agents, config.max_expert_count);
        tracing::info!(
            "【调度策略·专家过滤】待过滤专家列表: {:?} (共{}个)",
            agent_ids,
            agent_ids.len()
        );

        let original_count = agent_ids.len();
        let mut filtered = Vec::new();
        for id in agent_ids {
            if config.denied_agents.contains(&id) {
                tracing::warn!(
                    "【调度策略·专家过滤】专家 '{}' 被拒绝（在denied_agents黑名单中）",
                    id
                );
                continue;
            }
            if !config.allowed_agents.is_empty() && !config.allowed_agents.contains(&id) {
                tracing::warn!(
                    "【调度策略·专家过滤】专家 '{}' 被拒绝（不在allowed_agents白名单中）",
                    id
                );
                continue;
            }
            tracing::info!("【调度策略·专家过滤】专家 '{}' 通过过滤", id);
            filtered.push(id);
            if filtered.len() >= config.max_expert_count {
                tracing::warn!(
                    "【调度策略·专家过滤】已达到最大专家数量限制 {}，停止过滤",
                    config.max_expert_count
                );
                break;
            }
        }
        tracing::info!(
            "【调度策略·专家过滤】过滤结果: {} -> {} 个专家",
            original_count,
            filtered.len()
        );
        tracing::info!("【调度策略·专家过滤】最终专家列表: {:?}", filtered);
        tracing::info!("═══════════════════════════════════════════");
        filtered
    }
}

// ─── 第三层：执行监控规则 (ExecutionRule) ───────────────────

#[async_trait::async_trait]
pub trait ExecutionRule: Send + Sync + std::fmt::Debug {
    async fn check_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()>;
    async fn check_step_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()>;
    fn should_continue(&self, step_result: Result<CtxId>, config: &RuleConfig) -> bool;
    fn merge_results(&self, results: Vec<&ContextData>, config: &RuleConfig) -> String;
    fn check_max_steps(&self, current_step: usize, config: &RuleConfig) -> Result<()>;
}

#[derive(Debug)]
pub struct DefaultExecutionRule;

#[async_trait::async_trait]
impl ExecutionRule for DefaultExecutionRule {
    async fn check_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()> {
        let elapsed_ms = elapsed.as_millis();
        tracing::debug!(
            "【执行监控·总超时检查】已执行 {}ms / 限制 {}ms",
            elapsed_ms,
            config.max_execution_time_ms
        );
        if elapsed_ms > config.max_execution_time_ms as u128 {
            tracing::error!(
                "【执行监控·总超时检查】失败: {}ms > {}ms",
                elapsed_ms,
                config.max_execution_time_ms
            );
            Err(anyhow!(
                "总执行超时: {}ms > {}ms",
                elapsed_ms,
                config.max_execution_time_ms
            ))
        } else {
            tracing::debug!(
                "【执行监控·总超时检查】通过: {}ms <= {}ms",
                elapsed_ms,
                config.max_execution_time_ms
            );
            Ok(())
        }
    }

    async fn check_step_timeout(&self, elapsed: Duration, config: &RuleConfig) -> Result<()> {
        let elapsed_ms = elapsed.as_millis();
        tracing::debug!(
            "【执行监控·单步超时检查】已执行 {}ms / 限制 {}ms",
            elapsed_ms,
            config.per_step_timeout_ms
        );
        if elapsed_ms > config.per_step_timeout_ms as u128 {
            tracing::error!(
                "【执行监控·单步超时检查】失败: {}ms > {}ms",
                elapsed_ms,
                config.per_step_timeout_ms
            );
            Err(anyhow!(
                "步骤执行超时: {}ms > {}ms",
                elapsed_ms,
                config.per_step_timeout_ms
            ))
        } else {
            tracing::debug!(
                "【执行监控·单步超时检查】通过: {}ms <= {}ms",
                elapsed_ms,
                config.per_step_timeout_ms
            );
            Ok(())
        }
    }

    fn should_continue(&self, step_result: Result<CtxId>, config: &RuleConfig) -> bool {
        let is_err = step_result.is_err();
        let decision = !is_err || config.continue_on_failure;

        if is_err {
            tracing::warn!(
                "【执行监控·失败处理】步骤执行失败, continue_on_failure={}, 决策: {}",
                config.continue_on_failure,
                if decision {
                    "继续执行"
                } else {
                    "中断执行"
                }
            );
        } else {
            tracing::debug!("【执行监控·失败处理】步骤执行成功, 继续执行");
        }

        decision
    }

    fn merge_results(&self, results: Vec<&ContextData>, config: &RuleConfig) -> String {
        tracing::info!("═══════════════════════════════════════════");
        tracing::info!("【执行监控·结果聚合】开始聚合结果");
        tracing::info!(
            "【执行监控·结果聚合】策略: {:?}, 结果数: {}",
            config.result_strategy,
            results.len()
        );

        let output = match config.result_strategy {
            ResultStrategy::TakeLast => {
                let result = results
                    .last()
                    .map(|d| d.content.clone())
                    .unwrap_or_default();
                tracing::info!(
                    "【执行监控·结果聚合】TakeLast策略: 取最后一个结果 (长度: {})",
                    result.len()
                );
                result
            }
            ResultStrategy::TakeFirst => {
                let result = results
                    .first()
                    .map(|d| d.content.clone())
                    .unwrap_or_default();
                tracing::info!(
                    "【执行监控·结果聚合】TakeFirst策略: 取第一个结果 (长度: {})",
                    result.len()
                );
                result
            }
            ResultStrategy::MergeAll => {
                let merged = results
                    .iter()
                    .enumerate()
                    .map(|(i, d)| format!("【步骤 {}】\n{}", i + 1, d.content))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                tracing::info!(
                    "【执行监控·结果聚合】MergeAll策略: 合并{}个结果 (总长度: {})",
                    results.len(),
                    merged.len()
                );
                merged
            }
        };

        tracing::info!("【执行监控·结果聚合】聚合完成，输出长度: {}", output.len());
        tracing::info!("═══════════════════════════════════════════");
        output
    }

    fn check_max_steps(&self, current_step: usize, config: &RuleConfig) -> Result<()> {
        tracing::debug!(
            "【执行监控·步骤数检查】当前步骤: {} / 最大: {}",
            current_step,
            config.max_steps
        );
        if current_step > config.max_steps {
            tracing::error!(
                "【执行监控·步骤数检查】失败: {} > {}",
                current_step,
                config.max_steps
            );
            Err(anyhow!(
                "超过最大步骤数: {} > {}",
                current_step,
                config.max_steps
            ))
        } else {
            tracing::debug!(
                "【执行监控·步骤数检查】通过: {} <= {}",
                current_step,
                config.max_steps
            );
            Ok(())
        }
    }
}

// ─── 规则引擎 ──────────────────────────────────────────────

#[derive(Debug)]
pub struct RuleEngine {
    analysis_rule: Box<dyn TaskAnalysisRule>,
    dispatch_rule: Box<dyn DispatchRule>,
    execution_rule: Box<dyn ExecutionRule>,
    config: RuleConfig,
}

impl RuleEngine {
    pub fn new(config: RuleConfig) -> Self {
        Self {
            analysis_rule: Box::new(KeywordBasedTaskAnalysisRule),
            dispatch_rule: Box::new(DomainBasedDispatchRule),
            execution_rule: Box::new(DefaultExecutionRule),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(RuleConfig {
            max_task_length: 10000,
            max_expert_count: 10,
            max_execution_time_ms: 300_000,
            max_steps: 10,
            per_step_timeout_ms: 60_000,
            blacklist_keywords: vec!["暴力".into(), "攻击".into(), "色情".into()],
            allowed_agents: Vec::new(),
            denied_agents: Vec::new(),
            pipeline_threshold: 2,
            pass_full_context: false,
            continue_on_failure: false,
            result_strategy: ResultStrategy::TakeLast,
        })
    }

    pub fn analyze_task(&self, input: &str) -> Result<TaskProfile> {
        self.analysis_rule.analyze(input, &self.config)
    }

    pub fn decide_strategy(&self, profile: &TaskProfile, matched_count: usize) -> DispatchStrategy {
        let strategy = self
            .dispatch_rule
            .decide_strategy(profile, matched_count, &self.config);
        tracing::info!(
            "【调度策略】领域标签数={}, 匹配专家数={}, 选择策略={:?}",
            profile.domain_tags.len(),
            matched_count,
            strategy
        );
        strategy
    }

    pub fn filter_and_limit_agents(
        &self,
        agent_ids: Vec<AgentId>,
        profile: &TaskProfile,
    ) -> Vec<AgentId> {
        self.dispatch_rule
            .filter_and_limit(agent_ids, profile, &self.config)
    }

    pub async fn check_timeout(&self, elapsed: Duration) -> Result<()> {
        self.execution_rule
            .check_timeout(elapsed, &self.config)
            .await
    }

    pub async fn check_step_timeout(&self, elapsed: Duration) -> Result<()> {
        self.execution_rule
            .check_step_timeout(elapsed, &self.config)
            .await
    }

    pub fn should_continue(&self, step_result: Result<CtxId>) -> bool {
        self.execution_rule
            .should_continue(step_result, &self.config)
    }

    pub fn merge_results(&self, results: Vec<&ContextData>) -> String {
        self.execution_rule.merge_results(results, &self.config)
    }

    pub fn check_max_steps(&self, current_step: usize) -> Result<()> {
        self.execution_rule
            .check_max_steps(current_step, &self.config)
    }

    pub fn config(&self) -> &RuleConfig {
        &self.config
    }

    pub fn set_analysis_rule(&mut self, rule: Box<dyn TaskAnalysisRule>) {
        self.analysis_rule = rule;
    }

    pub fn set_dispatch_rule(&mut self, rule: Box<dyn DispatchRule>) {
        self.dispatch_rule = rule;
    }

    pub fn set_execution_rule(&mut self, rule: Box<dyn ExecutionRule>) {
        self.execution_rule = rule;
    }
}

// ─── 调度策略 ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum DispatchStrategy {
    SimpleDispatch,
    Pipeline,
}

// ─── Agent 元信息（注册中心）───────────────────────────────

#[derive(Debug, Clone)]
pub struct AgentMeta {
    pub id: AgentId,
    pub name: String,
    pub tags: Vec<String>,
    pub priority: u32,
}

// ─── 专家抽象（模板方法模式）───────────────────────────────

#[async_trait::async_trait]
pub trait ExpertAgent: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn tags(&self) -> Vec<String>;
    fn priority(&self) -> u32 {
        0
    }

    async fn run(&self, ctx_id: &str, store: &mut ContextStore) -> Result<CtxId>;
}

// ─── 线性链路（责任链模式）─────────────────────────────────

#[derive(Debug, Clone)]
pub struct Step {
    pub agent_id: AgentId,
    pub input_ctx_id: CtxId,
}

// ─── 顶层调度器 ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    pub max_rounds: u32,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self { max_rounds: 5 }
    }
}

pub struct Orchestrator {
    config: OrchestratorConfig,
    agent_registry: HashMap<AgentId, Arc<dyn ExpertAgent>>,
    meta_registry: HashMap<AgentId, AgentMeta>,
    context_store: ContextStore,
    rule_engine: RuleEngine,
    default_expert_id: AgentId,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("config", &self.config)
            .field("agent_count", &self.agent_registry.len())
            .finish()
    }
}

impl Orchestrator {
    pub fn new(config: OrchestratorConfig) -> Self {
        Self {
            config,
            agent_registry: HashMap::new(),
            meta_registry: HashMap::new(),
            context_store: ContextStore::new(),
            rule_engine: RuleEngine::with_defaults(),
            default_expert_id: String::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(OrchestratorConfig::default())
    }

    // ── 注册中心 ────────────────────────────────────────────

    pub fn register_agent(&mut self, agent: Arc<dyn ExpertAgent>) {
        let id = agent.id().to_string();
        let meta = AgentMeta {
            id: id.clone(),
            name: agent.name().to_string(),
            tags: agent.tags(),
            priority: agent.priority(),
        };

        tracing::info!(
            "【注册中心】注册专家: id={}, name={}, tags={}",
            id,
            meta.name,
            meta.tags.join(",")
        );

        self.agent_registry.insert(id.clone(), agent);
        self.meta_registry.insert(id, meta);
    }

    pub fn get_agent(&self, agent_id: &str) -> Option<Arc<dyn ExpertAgent>> {
        self.agent_registry.get(agent_id).cloned()
    }

    pub fn list_agents(&self) -> Vec<AgentMeta> {
        self.meta_registry.values().cloned().collect()
    }

    pub fn set_default_expert(&mut self, expert_id: &str) {
        if self.agent_registry.contains_key(expert_id) {
            self.default_expert_id = expert_id.to_string();
        }
    }

    pub fn set_general_expert(&mut self, expert_id: &str) {
        self.set_default_expert(expert_id);
    }

    // ── 上下文管理 ──────────────────────────────────────────

    pub fn put_context(&mut self, content: String) -> CtxId {
        let data = ContextData {
            content,
            metadata: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        self.context_store.put(data)
    }

    pub fn get_context(&self, ctx_id: &str) -> Option<&ContextData> {
        self.context_store.get(ctx_id)
    }

    // ── 调度算法：任务理解 + 关键词匹配 + 策略决策 ──────────────

    pub fn schedule(&mut self, target: &str) -> Result<(Vec<Step>, DispatchStrategy, TaskProfile)> {
        tracing::info!("【调度算法】开始分析任务: {}", target);

        let profile = self.rule_engine.analyze_task(target)?;
        let material_ctx_id = self.put_context(target.to_string());
        tracing::debug!("【调度算法】上下文 ID: {}", material_ctx_id);

        let agents: Vec<AgentMeta> = self.meta_registry.values().cloned().collect();
        tracing::info!("【调度算法】注册中心共有 {} 个专家", agents.len());

        let mut matched: Vec<(&AgentMeta, usize)> = Vec::new();
        let target_lower = target.to_lowercase();

        for agent in &agents {
            for (idx, tag) in agent.tags.iter().enumerate() {
                if target_lower.contains(&tag.to_lowercase()) || profile.domain_tags.contains(tag) {
                    matched.push((agent, idx));
                    tracing::info!("【调度算法】匹配成功: expert={}, tag={}", agent.name, tag);
                    break;
                }
            }
        }

        if matched.is_empty() {
            if let Some(agent) = agents.first() {
                tracing::warn!("【调度算法】无匹配专家，使用默认: {}", agent.name);
                matched.push((agent, 0));
            } else {
                return Err(anyhow!("无可用专家"));
            }
        }

        let strategy = self.rule_engine.decide_strategy(&profile, matched.len());

        matched.sort_by(|a, b| {
            let priority_cmp = b.0.priority.cmp(&a.0.priority);
            if priority_cmp != std::cmp::Ordering::Equal {
                return priority_cmp;
            }
            a.1.cmp(&b.1)
        });

        let matched_ids: Vec<AgentId> = matched.iter().map(|(a, _)| a.id.clone()).collect();
        let filtered_ids = self
            .rule_engine
            .filter_and_limit_agents(matched_ids, &profile);

        let chain: Vec<Step> = filtered_ids
            .iter()
            .map(|id| Step {
                agent_id: id.clone(),
                input_ctx_id: material_ctx_id.clone(),
            })
            .collect();

        tracing::info!(
            "【调度算法】生成链路: {} 个步骤, 策略={:?}",
            chain.len(),
            strategy
        );
        for (i, step) in chain.iter().enumerate() {
            tracing::info!("【调度算法】Step[{}] -> agent_id={}", i + 1, step.agent_id);
        }

        Ok((chain, strategy, profile))
    }

    // ── 执行入口：责任链串行执行 ──────────────────────────────

    pub async fn execute(&mut self, target: &str) -> Result<ExecutionResult> {
        let start = Instant::now();

        let (chain, strategy, _profile) = self.schedule(target)?;

        tracing::info!(
            "【责任链执行】开始执行 {} 个步骤, 策略={:?}",
            chain.len(),
            strategy
        );
        let mut current_ctx_id = None;
        let mut agent_chain = Vec::new();
        let mut all_outputs: Vec<String> = Vec::new();

        for (index, step) in chain.iter().enumerate() {
            self.rule_engine.check_max_steps(index + 1)?;
            self.rule_engine.check_timeout(start.elapsed()).await?;

            let agent = self
                .agent_registry
                .get(&step.agent_id)
                .ok_or_else(|| anyhow!("专家不存在: {}", step.agent_id))?;

            tracing::info!(
                "【责任链执行】Step[{}/{}] 执行专家: {}",
                index + 1,
                chain.len(),
                agent.name()
            );

            let input_ctx_id = if self.rule_engine.config().pass_full_context {
                &step.input_ctx_id as &str
            } else {
                current_ctx_id
                    .as_deref()
                    .unwrap_or(&step.input_ctx_id as &str)
            };

            let step_start = Instant::now();
            let step_result = agent.run(input_ctx_id, &mut self.context_store).await;
            let step_duration = step_start.elapsed().as_millis();

            match step_result {
                Ok(ctx_id) => {
                    let output = self
                        .context_store
                        .get(&ctx_id)
                        .map(|d| d.content.clone())
                        .unwrap_or_default();
                    all_outputs.push(output);
                    current_ctx_id = Some(ctx_id);
                    agent_chain.push(step.agent_id.clone());

                    tracing::info!(
                        "【责任链执行】Step[{}] 完成: expert={}, duration={}ms",
                        index + 1,
                        agent.name(),
                        step_duration
                    );
                }
                Err(e) => {
                    tracing::error!(
                        "【责任链执行】Step[{}] 失败: expert={}, error={:?}",
                        index + 1,
                        agent.name(),
                        e
                    );

                    if !self.rule_engine.should_continue(Err::<CtxId, _>(e)) {
                        return Err(anyhow!("专家执行失败: {}", agent.name()));
                    }
                }
            }
        }

        let final_output = if all_outputs.is_empty() {
            String::new()
        } else {
            let results: Vec<ContextData> = all_outputs
                .iter()
                .map(|c| ContextData {
                    content: c.clone(),
                    metadata: HashMap::new(),
                    created_at: 0,
                })
                .collect();
            let refs: Vec<&ContextData> = results.iter().collect();
            self.rule_engine.merge_results(refs)
        };
        let duration_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            "【责任链执行】全部完成，耗时 {}ms, 策略={:?}",
            duration_ms,
            strategy
        );

        Ok(ExecutionResult {
            output: final_output,
            agent_chain,
            tokens: TokenStats::default(),
            duration_ms,
            strategy,
        })
    }

    // ── 兼容旧 API ──────────────────────────────────────────

    pub async fn run_orchestrated(
        &mut self,
        input: &str,
        _user_id: &str,
    ) -> Result<OrchestrationResult> {
        let result = self.execute(input).await?;
        Ok(OrchestrationResult {
            output: result.output,
            strategy: match result.strategy {
                DispatchStrategy::SimpleDispatch => "SimpleDispatch".to_string(),
                DispatchStrategy::Pipeline => "Pipeline".to_string(),
            },
            expert_chain: result.agent_chain,
            expert_outputs: HashMap::new(),
            tokens: result.tokens,
            duration_ms: result.duration_ms,
            critique_records: Vec::new(),
        })
    }
}

// ─── 执行结果 ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub output: String,
    pub agent_chain: Vec<String>,
    pub tokens: TokenStats,
    pub duration_ms: u64,
    pub strategy: DispatchStrategy,
}

// ─── 默认专家实现 ──────────────────────────────────────────

#[derive(Debug)]
pub struct DefaultExpert {
    id: String,
    name: String,
    tags: Vec<String>,
    priority: u32,
    role: String,
    backstory: String,
    goal: String,
    runtime: Arc<Runtime>,
}

impl DefaultExpert {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        tags: Vec<String>,
        priority: u32,
        role: String,
        backstory: String,
        goal: String,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            id,
            name,
            tags,
            priority,
            role,
            backstory,
            goal,
            runtime,
        }
    }
}

#[async_trait::async_trait]
impl ExpertAgent for DefaultExpert {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn tags(&self) -> Vec<String> {
        self.tags.clone()
    }
    fn priority(&self) -> u32 {
        self.priority
    }

    async fn run(&self, ctx_id: &str, store: &mut ContextStore) -> Result<CtxId> {
        let context = store
            .get(ctx_id)
            .ok_or_else(|| anyhow!("上下文不存在: {}", ctx_id))?;

        let prompt = format!(
            r#"你是 {}。
角色背景：{}
目标：{}

当前任务：{}

请直接回答这个任务。"#,
            self.role, self.backstory, self.goal, context.content
        );

        let messages = vec![
            Message {
                role: Role::System,
                content: format!(
                    "你是 {}。角色背景：{}。目标：{}",
                    self.role, self.backstory, self.goal
                ),
                tool_call_id: None,
            },
            Message {
                role: Role::User,
                content: prompt,
                tool_call_id: None,
            },
        ];

        let response = self.runtime.call_llm(messages).await?;

        Ok(store.put(ContextData {
            content: response,
            metadata: HashMap::new(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }))
    }
}

// ─── 兼容旧 API ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CollaborationResult {
    pub output: String,
    pub expert_chain: Vec<String>,
    pub expert_outputs: HashMap<String, String>,
    pub tokens: TokenStats,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct OrchestrationResult {
    pub output: String,
    pub strategy: String,
    pub expert_chain: Vec<String>,
    pub expert_outputs: HashMap<String, String>,
    pub tokens: TokenStats,
    pub duration_ms: u64,
    pub critique_records: Vec<CritiqueRecord>,
}

#[derive(Debug, Clone)]
pub struct CritiqueRecord;

#[derive(Debug, Clone)]
pub struct ExpertMatchResult;

#[derive(Debug, Clone)]
pub struct ExpertPerformance;

pub type Expert = DefaultExpert;
pub type ExpertAgentTrait = dyn ExpertAgent;
