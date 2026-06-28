//! # 多 Agent 协调编排层 (Orchestrator)
//!
//! ## 设计理念
//!
//! 采用「分层协调 + 策略化调度 + 自治专家池」方案：
//! - **协调编排层**：任务理解、策略选择、专家路由、结果聚合、记忆同步
//! - **专家 Agent 池**：自治插件单元，自带 Soul/Memory/Skill/Flow
//! - **基础设施层**：公共 Skill 池、公共知识库、分层记忆、工具调用引擎
//!
//! ## 调度策略
//!
//! 1. **单专家直连** (SimpleDispatch)：单一领域 & 低复杂度
//! 2. **串行流水线** (Pipeline)：有明确前后依赖的多阶段任务
//! 3. **并行发散-汇总** (MapReduce)：独立子任务并行执行
//! 4. **主管-工人** (ManagerWorker)：复杂任务委派给主管专家
//! 5. **评审迭代** (CritiqueRevise)：生成→评审→修改循环
//!
//! ## 设计模式
//!
//! - 中介者模式：协调器作为全局中心
//! - 策略模式：5 种调度策略动态切换
//! - 工厂模式 + 注册发现：专家插件自动加载
//! - 责任链模式：串行流水线任务传递
//! - 状态模式：任务生命周期状态流转
//! - 组合模式：复杂任务拆解为子任务
//! - 观察者模式：并行任务状态同步

pub mod strategies;

use crate::context::TokenStats;
use crate::expert::{ExpertInfo, ExpertPersona, ExpertPlugin, PluginCategory};
use crate::memory::Memory;
use crate::runtime::Runtime;
use crate::skill::SkillManager;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

// ─── 任务画像 ──────────────────────────────────────────────

/// 任务领域标签
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskDomain {
    /// 通用对话
    General,
    /// 编程开发
    Development,
    /// 数据库/向量
    Database,
    /// 架构设计
    Architecture,
    /// 心理咨询
    Psychology,
    /// 教育学习
    Education,
    /// 商业分析
    Business,
    /// 创意写作
    Writing,
    /// 法律咨询
    Legal,
    /// 医疗健康
    Medical,
    /// 金融投资
    Finance,
    /// 自定义领域
    Custom(String),
}

/// 任务复杂度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskComplexityLevel {
    Simple,
    Medium,
    Complex,
    VeryComplex,
}

/// 任务画像
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskProfile {
    /// 原始用户输入
    pub input: String,
    /// 领域标签
    pub domains: Vec<TaskDomain>,
    /// 复杂度
    pub complexity: TaskComplexityLevel,
    /// 是否需要多专家协作
    pub needs_collaboration: bool,
    /// 是否有明确的前后依赖
    pub has_dependencies: bool,
    /// 是否需要质量评审
    pub needs_review: bool,
    /// 子任务列表（如果可拆分）
    pub sub_tasks: Vec<SubTask>,
    /// 预估步骤数
    pub estimated_steps: usize,
    /// 是否需要工具调用
    pub needs_tools: bool,
    /// 是否需要外部信息
    pub needs_external_info: bool,
    /// 用户意图摘要
    pub user_intent: String,
}

impl Default for TaskProfile {
    fn default() -> Self {
        Self {
            input: String::new(),
            domains: vec![TaskDomain::General],
            complexity: TaskComplexityLevel::Simple,
            needs_collaboration: false,
            has_dependencies: false,
            needs_review: false,
            sub_tasks: Vec::new(),
            estimated_steps: 1,
            needs_tools: false,
            needs_external_info: false,
            user_intent: String::new(),
        }
    }
}

/// 子任务
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubTask {
    /// 子任务 ID
    pub id: String,
    /// 描述
    pub description: String,
    /// 所需领域
    pub domain: TaskDomain,
    /// 依赖的子任务 ID
    pub dependencies: Vec<String>,
    /// 分配给哪个专家
    pub assigned_expert: Option<String>,
    /// 执行结果
    pub result: Option<String>,
    /// 状态
    pub status: SubTaskStatus,
}

/// 子任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubTaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

// ─── 调度策略 ──────────────────────────────────────────────

/// 调度策略枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchStrategy {
    /// 单专家直连
    SimpleDispatch,
    /// 串行流水线
    Pipeline,
    /// 并行发散-汇总 (Map-Reduce)
    MapReduce,
    /// 主管-工人模式
    ManagerWorker,
    /// 评审迭代模式（叠加在其他策略之上）
    CritiqueRevise,
}

/// 调度决策
#[derive(Debug, Clone)]
pub struct DispatchDecision {
    /// 选择的策略
    pub strategy: DispatchStrategy,
    /// 匹配的专家列表
    pub matched_experts: Vec<ExpertMatchResult>,
    /// 任务画像
    pub profile: TaskProfile,
    /// 是否需要叠加评审迭代
    pub needs_critique: bool,
    /// 决策原因
    pub reason: String,
}

/// 专家匹配结果
#[derive(Debug, Clone)]
pub struct ExpertMatchResult {
    /// 专家 ID
    pub expert_id: String,
    /// 专家信息
    pub expert_info: ExpertInfo,
    /// 领域匹配度 (0-1)
    pub domain_match: f32,
    /// 能力覆盖度 (0-1)
    pub capability_coverage: f32,
    /// 历史表现分 (0-1)
    pub historical_score: f32,
    /// 当前负载 (并发任务数)
    pub current_load: u32,
    /// 综合得分
    pub overall_score: f32,
}

// ─── 协调编排结果 ──────────────────────────────────────────

/// 编排执行结果
#[derive(Debug, Clone)]
pub struct OrchestrationResult {
    /// 最终输出
    pub output: String,
    /// 使用的策略
    pub strategy: DispatchStrategy,
    /// 参与专家链（按执行顺序）
    pub expert_chain: Vec<String>,
    /// 各专家输出
    pub expert_outputs: HashMap<String, String>,
    /// Token 统计
    pub tokens: TokenStats,
    /// 总耗时 (ms)
    pub duration_ms: u64,
    /// 评审记录（如果有）
    pub critique_records: Vec<CritiqueRecord>,
}

/// 评审记录
#[derive(Debug, Clone)]
pub struct CritiqueRecord {
    /// 评审专家
    pub reviewer: String,
    /// 被评审内容
    pub content: String,
    /// 评审意见
    pub feedback: String,
    /// 迭代轮次
    pub round: u32,
}

// ─── 专家 Agent 注册信息 ───────────────────────────────────

/// 专家 Agent 注册信息
///
/// 每个专家 Agent 都是一个自治单元：
/// - 自带 Soul（人格）
/// - 自带 Memory（私有记忆）
/// - 自带 Skill（私有技能）
/// - 共享 Runtime（LLM + Tools）
pub struct ExpertAgent {
    /// 专家 ID
    pub id: String,
    /// 专家名称
    pub name: String,
    /// 专家描述
    pub description: String,
    /// 领域标签
    pub domains: Vec<TaskDomain>,
    /// 关键词
    pub keywords: Vec<String>,
    /// 专家人格
    pub persona: ExpertPersona,
    /// 私有 Skill 管理器
    pub skills: SkillManager,
    /// 私有记忆
    pub memory: Memory,
    /// 历史表现统计
    pub performance: ExpertPerformance,
    /// 插件引用（底层插件）
    pub plugin: Arc<dyn ExpertPlugin>,
}

/// 专家历史表现
#[derive(Debug, Clone, Default)]
pub struct ExpertPerformance {
    /// 总任务数
    pub total_tasks: u32,
    /// 成功任务数
    pub successful_tasks: u32,
    /// 失败任务数
    pub failed_tasks: u32,
    /// 平均耗时 (ms)
    pub avg_duration_ms: u64,
    /// 用户满意度评分 (0-1)
    pub satisfaction_score: f32,
}

// ─── 协调编排器 ──────────────────────────────────────────

/// 多 Agent 协调编排器配置
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// 默认领域匹配权重
    pub domain_match_weight: f32,
    /// 能力覆盖权重
    pub capability_weight: f32,
    /// 历史表现权重
    pub historical_weight: f32,
    /// 当前负载权重
    pub load_weight: f32,
    /// 评审迭代最大轮次
    pub max_critique_rounds: u32,
    /// 是否启用自动策略选择
    pub auto_strategy: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            domain_match_weight: 0.6,
            capability_weight: 0.2,
            historical_weight: 0.1,
            load_weight: 0.1,
            max_critique_rounds: 3,
            auto_strategy: true,
        }
    }
}

/// 多 Agent 协调编排器
///
/// 整个系统的唯一入口，不做具体业务执行，只负责全局管控：
/// - 任务画像：解析用户需求
/// - 策略决策：选择最优调度模式
/// - 专家路由：匹配最适合的专家
/// - 流程编排：控制多专家执行顺序
/// - 结果治理：汇总输出、处理冲突
/// - 记忆同步：维护全局公共记忆
pub struct Orchestrator {
    /// 配置
    config: OrchestratorConfig,
    /// 专家 Agent 池
    expert_pool: HashMap<String, ExpertAgent>,
    /// 通用专家（兜底能力，使用公共 Skill + 共享记忆）
    pub general_expert_id: String,
    /// 公共 Skill 管理器（所有专家共享）
    pub shared_skills: Arc<RwLock<SkillManager>>,
    /// 公共记忆（全局共享）
    pub shared_memory: Arc<Memory>,
    /// 运行时（所有专家共享 LLM + Tools）
    pub runtime: Arc<Runtime>,
}

impl std::fmt::Debug for Orchestrator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Orchestrator")
            .field("config", &self.config)
            .field("expert_count", &self.expert_pool.len())
            .field("general_expert_id", &self.general_expert_id)
            .field("has_shared_skills", &true)
            .field("has_shared_memory", &true)
            .field("has_runtime", &true)
            .finish()
    }
}

impl Orchestrator {
    /// 创建新的编排器
    pub fn new(
        config: OrchestratorConfig,
        shared_skills: Arc<RwLock<SkillManager>>,
        shared_memory: Arc<Memory>,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self {
            config,
            expert_pool: HashMap::new(),
            general_expert_id: String::new(),
            shared_skills,
            shared_memory,
            runtime,
        }
    }

    /// 使用默认配置创建
    pub fn with_defaults(
        shared_skills: Arc<RwLock<SkillManager>>,
        shared_memory: Arc<Memory>,
        runtime: Arc<Runtime>,
    ) -> Self {
        Self::new(
            OrchestratorConfig::default(),
            shared_skills,
            shared_memory,
            runtime,
        )
    }

    // ── 专家池管理 ──────────────────────────────────────

    /// 注册专家 Agent
    pub fn register_expert(&mut self, agent: ExpertAgent) {
        let id = agent.id.clone();
        tracing::info!(
            "Orchestrator: Registering expert agent: {} ({} domains, {} skills)",
            agent.name,
            agent.domains.len(),
            agent.skills.get_skills().len()
        );
        self.expert_pool.insert(id, agent);
    }

    /// 设置通用专家 ID（兜底能力）
    pub fn set_general_expert(&mut self, expert_id: &str) {
        self.general_expert_id = expert_id.to_string();
        tracing::info!("Orchestrator: General expert set to: {}", expert_id);
    }

    /// 从插件注册专家 Agent
    pub fn register_expert_from_plugin(&mut self, plugin: Arc<dyn ExpertPlugin>) {
        let info = plugin.info();
        let persona = plugin.persona();

        // 构建领域标签
        let manifest = plugin.manifest();
        let domains = Self::map_domains(&info.keywords, &manifest.category);

        // 构建私有 Skill 管理器
        let mut skills = SkillManager::new();
        for skill in plugin.skills() {
            skills.register_boxed(skill);
        }

        // 构建私有记忆
        let memory = Memory::new();

        // 加载知识库到私有记忆
        for entry in plugin.knowledge() {
            let _ = memory.add_knowledge(entry.content, entry.metadata);
        }

        let agent = ExpertAgent {
            id: info.id.clone(),
            name: info.name.clone(),
            description: info.description.clone(),
            domains,
            keywords: info.keywords.clone(),
            persona,
            skills,
            memory,
            performance: ExpertPerformance::default(),
            plugin,
        };

        self.register_expert(agent);
    }

    /// 移除专家
    pub fn remove_expert(&mut self, expert_id: &str) -> Option<ExpertAgent> {
        let removed = self.expert_pool.remove(expert_id);
        if removed.is_some() {
            tracing::info!("Orchestrator: Removed expert: {}", expert_id);
        }
        removed
    }

    /// 获取专家列表
    pub fn list_experts(&self) -> Vec<ExpertInfo> {
        self.expert_pool
            .values()
            .map(|a| ExpertInfo {
                id: a.id.clone(),
                name: a.name.clone(),
                description: a.description.clone(),
                version: "1.0.0".to_string(),
                author: None,
                category: "expert".to_string(),
                keywords: a.keywords.clone(),
            })
            .collect()
    }

    /// 获取专家数量
    pub fn expert_count(&self) -> usize {
        self.expert_pool.len()
    }

    /// 获取指定专家 Agent（用于策略执行）
    pub fn get_expert(&self, expert_id: &str) -> Option<&ExpertAgent> {
        self.expert_pool.get(expert_id)
    }

    /// 获取通用专家 ID
    pub fn get_general_expert_id(&self) -> String {
        if self.general_expert_id.is_empty() {
            "general".to_string()
        } else {
            self.general_expert_id.clone()
        }
    }

    /// 获取配置引用
    pub fn config(&self) -> &OrchestratorConfig {
        &self.config
    }

    /// 检查专家池是否为空
    pub fn is_empty(&self) -> bool {
        self.expert_pool.is_empty()
    }

    // ── 任务画像 ──────────────────────────────────────

    /// 分析任务，生成任务画像
    ///
    /// 通过关键词匹配和规则引擎进行任务分析。
    /// 复杂场景可以调用 LLM 进行深度分析。
    pub fn analyze_task(&self, input: &str) -> TaskProfile {
        let lower_input = input.to_lowercase();
        let mut profile = TaskProfile {
            input: input.to_string(),
            ..Default::default()
        };

        // 1. 领域识别
        let mut domains = Vec::new();

        // 编程开发领域
        if self.match_keywords(
            &lower_input,
            &[
                "代码",
                "编程",
                "函数",
                "bug",
                "编译",
                "rust",
                "python",
                "api",
                "接口",
                "数据库",
                "sql",
                "postgres",
                "mysql",
                "redis",
            ],
        ) {
            domains.push(TaskDomain::Development);
        }

        // 数据库/向量领域
        if self.match_keywords(
            &lower_input,
            &[
                "向量",
                "embedding",
                "pgvector",
                "索引",
                "查询优化",
                "ivfflat",
                "hnsw",
            ],
        ) {
            domains.push(TaskDomain::Database);
        }

        // 架构设计领域
        if self.match_keywords(
            &lower_input,
            &["架构", "设计", "方案", "系统设计", "微服务", "分层", "模块"],
        ) {
            domains.push(TaskDomain::Architecture);
        }

        // 心理咨询领域
        if self.match_keywords(
            &lower_input,
            &[
                "心理",
                "情绪",
                "心情",
                "压力",
                "焦虑",
                "抑郁",
                "失眠",
                "咨询",
                "不开心",
                "难过",
                "烦躁",
            ],
        ) {
            domains.push(TaskDomain::Psychology);
        }

        // 如果没有匹配到特定领域，使用通用领域
        if domains.is_empty() {
            domains.push(TaskDomain::General);
        }

        profile.domains = domains;

        // 2. 复杂度判断
        let input_len = input.chars().count();
        let has_multi_step = self.match_keywords(
            &lower_input,
            &[
                "然后",
                "接着",
                "之后",
                "先",
                "再",
                "步骤",
                "流程",
                "第一步",
                "最后",
                "对比",
                "比较",
                "分析",
            ],
        );
        let has_review_need = self.match_keywords(
            &lower_input,
            &["评审", "检查", "审查", "review", "优化", "改进"],
        );

        if input_len > 200 || (has_multi_step && input_len > 100) {
            profile.complexity = TaskComplexityLevel::Complex;
        } else if has_multi_step || input_len > 100 {
            profile.complexity = TaskComplexityLevel::Medium;
        } else {
            profile.complexity = TaskComplexityLevel::Simple;
        }

        // 3. 协作判断
        profile.needs_collaboration =
            profile.domains.len() > 1 || profile.complexity == TaskComplexityLevel::Complex;

        // 4. 依赖判断
        profile.has_dependencies = has_multi_step;

        // 5. 评审判断
        profile.needs_review = has_review_need;

        // 6. 用户意图
        if self.match_keywords(&lower_input, &["怎么", "如何", "什么", "为什么"]) {
            profile.user_intent = "知识查询".to_string();
        } else if self.match_keywords(&lower_input, &["开发", "实现", "搭建", "构建", "写"])
        {
            profile.user_intent = "任务执行".to_string();
        } else if self.match_keywords(&lower_input, &["分析", "对比", "比较", "评估"]) {
            profile.user_intent = "分析推理".to_string();
        } else {
            profile.user_intent = "对话交流".to_string();
        }

        profile
    }

    // ── 专家匹配 ──────────────────────────────────────

    /// 匹配专家
    ///
    /// 加权多维度匹配算法：
    /// - 领域匹配度（60%）
    /// - 能力覆盖度（20%）
    /// - 历史表现（10%）
    /// - 当前负载（10%）
    pub fn match_experts(&self, profile: &TaskProfile, limit: usize) -> Vec<ExpertMatchResult> {
        let mut results: Vec<ExpertMatchResult> = Vec::new();

        for agent in self.expert_pool.values() {
            // 计算各维度得分
            let domain_match = self.calculate_domain_match(&profile.domains, &agent.domains);
            let capability_coverage = self.calculate_capability_coverage(profile, agent);
            let historical_score = self.calculate_historical_score(&agent.performance);
            let current_load = 0; // TODO: 从运行时获取

            let overall_score = domain_match * self.config.domain_match_weight
                + capability_coverage * self.config.capability_weight
                + historical_score * self.config.historical_weight
                - (current_load as f32 * 0.05) * self.config.load_weight;

            if overall_score > 0.0 {
                results.push(ExpertMatchResult {
                    expert_id: agent.id.clone(),
                    expert_info: ExpertInfo {
                        id: agent.id.clone(),
                        name: agent.name.clone(),
                        description: agent.description.clone(),
                        version: "1.0.0".to_string(),
                        author: None,
                        category: "expert".to_string(),
                        keywords: agent.keywords.clone(),
                    },
                    domain_match,
                    capability_coverage,
                    historical_score,
                    current_load,
                    overall_score,
                });
            }
        }

        // 按综合得分降序排序
        results.sort_by(|a, b| {
            b.overall_score
                .partial_cmp(&a.overall_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(limit);
        results
    }

    /// 获取最佳匹配专家
    pub fn best_expert(&self, profile: &TaskProfile) -> Option<ExpertMatchResult> {
        self.match_experts(profile, 1).into_iter().next()
    }

    // ── 策略决策 ──────────────────────────────────────

    /// 选择最优调度策略
    ///
    /// 决策树：
    /// - 单一领域 & 低复杂度 → 单专家直连
    /// - 多领域 & 有明确前后依赖 → 串行流水线
    /// - 多领域 & 子任务无依赖 → 并行发散-汇总
    /// - 超复杂 & 需动态拆解 → 主管-工人模式
    /// - 高要求 & 需质量把控 → 叠加评审迭代
    pub fn decide_strategy(&self, profile: &TaskProfile) -> DispatchDecision {
        let matched_experts = self.match_experts(profile, 5);

        let strategy = if profile.complexity == TaskComplexityLevel::VeryComplex {
            DispatchStrategy::ManagerWorker
        } else if profile.domains.len() > 1 && !profile.has_dependencies {
            DispatchStrategy::MapReduce
        } else if profile.domains.len() > 1 && profile.has_dependencies {
            DispatchStrategy::Pipeline
        } else {
            DispatchStrategy::SimpleDispatch
        };

        let reason = match strategy {
            DispatchStrategy::SimpleDispatch => "单一领域简单任务，直接路由给匹配专家".to_string(),
            DispatchStrategy::Pipeline => "多领域有依赖任务，使用串行流水线模式".to_string(),
            DispatchStrategy::MapReduce => "多领域独立子任务，使用并行发散-汇总模式".to_string(),
            DispatchStrategy::ManagerWorker => "超复杂任务，委派给主管专家自主拆解".to_string(),
            DispatchStrategy::CritiqueRevise => "需要质量评审的任务".to_string(),
        };

        DispatchDecision {
            strategy,
            matched_experts,
            profile: profile.clone(),
            needs_critique: profile.needs_review,
            reason,
        }
    }

    // ── 核心执行入口 ──────────────────────────────────

    /// 执行编排（根据任务画像自动选择策略）
    pub async fn execute(&self, input: &str, _user_id: &str) -> Result<OrchestrationResult> {
        let start = std::time::Instant::now();

        // 1. 任务画像
        let profile = self.analyze_task(input);

        // 2. 策略决策
        let decision = self.decide_strategy(&profile);
        tracing::info!(
            "Orchestrator: Strategy = {:?}, reason = {}, experts = {:?}",
            decision.strategy,
            decision.reason,
            decision
                .matched_experts
                .iter()
                .map(|m| &m.expert_id)
                .collect::<Vec<_>>()
        );

        // 3. 根据策略执行
        let mut result = match decision.strategy {
            DispatchStrategy::SimpleDispatch => {
                strategies::simple_dispatch::execute(self, &profile, &decision).await?
            }
            DispatchStrategy::Pipeline => {
                strategies::pipeline::execute(self, &profile, &decision).await?
            }
            DispatchStrategy::MapReduce => {
                strategies::map_reduce::execute(self, &profile, &decision).await?
            }
            DispatchStrategy::ManagerWorker => {
                strategies::manager_worker::execute(self, &profile, &decision).await?
            }
            DispatchStrategy::CritiqueRevise => {
                // CritiqueRevise 是叠加模式，单独使用意义不大
                // 但保留入口
                strategies::critique_revise::execute(self, &profile, &decision).await?
            }
        };

        // 4. 如果需要评审，叠加评审迭代
        if decision.needs_critique && result.strategy != DispatchStrategy::CritiqueRevise {
            let critique_result = strategies::critique_revise::execute_on_result(
                self,
                &profile,
                &decision,
                &result.output,
            )
            .await?;
            result.output = critique_result.output;
            result.critique_records = critique_result.critique_records;
        }

        result.duration_ms = start.elapsed().as_millis() as u64;

        tracing::info!(
            "Orchestrator: Completed in {}ms, strategy={:?}, chain={:?}",
            result.duration_ms,
            result.strategy,
            result.expert_chain
        );

        Ok(result)
    }

    /// 执行编排（指定策略）
    pub async fn execute_with_strategy(
        &self,
        input: &str,
        _user_id: &str,
        strategy: DispatchStrategy,
    ) -> Result<OrchestrationResult> {
        let start = std::time::Instant::now();
        let profile = self.analyze_task(input);
        let decision = self.decide_strategy(&profile);

        // 强制使用指定策略
        let forced_decision = DispatchDecision {
            strategy,
            ..decision
        };

        let mut result = self.execute_with_decision(&forced_decision).await?;
        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// 使用决策执行
    async fn execute_with_decision(
        &self,
        decision: &DispatchDecision,
    ) -> Result<OrchestrationResult> {
        match decision.strategy {
            DispatchStrategy::SimpleDispatch => {
                strategies::simple_dispatch::execute(self, &decision.profile, decision).await
            }
            DispatchStrategy::Pipeline => {
                strategies::pipeline::execute(self, &decision.profile, decision).await
            }
            DispatchStrategy::MapReduce => {
                strategies::map_reduce::execute(self, &decision.profile, decision).await
            }
            DispatchStrategy::ManagerWorker => {
                strategies::manager_worker::execute(self, &decision.profile, decision).await
            }
            DispatchStrategy::CritiqueRevise => {
                strategies::critique_revise::execute(self, &decision.profile, decision).await
            }
        }
    }

    // ── 内部辅助方法 ──────────────────────────────────

    /// 关键词匹配
    fn match_keywords(&self, input: &str, keywords: &[&str]) -> bool {
        keywords.iter().any(|kw| input.contains(kw))
    }

    /// 领域标签映射
    pub(crate) fn map_domains(keywords: &[String], category: &PluginCategory) -> Vec<TaskDomain> {
        let mut domains = Vec::new();

        match category {
            PluginCategory::Psychology => domains.push(TaskDomain::Psychology),
            PluginCategory::Development => domains.push(TaskDomain::Development),
            PluginCategory::Education => domains.push(TaskDomain::Education),
            PluginCategory::Business => domains.push(TaskDomain::Business),
            PluginCategory::Writing => domains.push(TaskDomain::Writing),
            PluginCategory::Legal => domains.push(TaskDomain::Legal),
            PluginCategory::Medical => domains.push(TaskDomain::Medical),
            PluginCategory::Finance => domains.push(TaskDomain::Finance),
            _ => {}
        }

        // 通过关键词补充领域识别
        let all_keywords: Vec<String> = keywords.iter().map(|k| k.to_lowercase()).collect();

        if all_keywords
            .iter()
            .any(|k| ["数据库", "向量", "sql", "postgres", "mysql"].contains(&k.as_str()))
            && !domains.contains(&TaskDomain::Database)
        {
            domains.push(TaskDomain::Database);
        }

        if all_keywords
            .iter()
            .any(|k| ["架构", "设计", "系统"].contains(&k.as_str()))
            && !domains.contains(&TaskDomain::Architecture)
        {
            domains.push(TaskDomain::Architecture);
        }

        if domains.is_empty() {
            domains.push(TaskDomain::General);
        }

        domains
    }

    /// 计算领域匹配度
    fn calculate_domain_match(
        &self,
        task_domains: &[TaskDomain],
        expert_domains: &[TaskDomain],
    ) -> f32 {
        if task_domains.is_empty() || expert_domains.is_empty() {
            return 0.0;
        }

        let matched: usize = task_domains
            .iter()
            .filter(|td| expert_domains.contains(td))
            .count();

        // Jaccard 相似度
        let union = task_domains.len() + expert_domains.len() - matched;
        if union == 0 {
            return 0.0;
        }
        matched as f32 / union as f32
    }

    /// 计算能力覆盖度
    fn calculate_capability_coverage(&self, profile: &TaskProfile, agent: &ExpertAgent) -> f32 {
        let mut score = 0.5; // 基础分

        // 专家有关键词匹配 → 加分
        let lower_input = profile.input.to_lowercase();
        let keyword_matches: usize = agent
            .keywords
            .iter()
            .filter(|kw| lower_input.contains(&kw.to_lowercase()))
            .count();

        if keyword_matches > 0 {
            score += 0.1 * keyword_matches as f32;
        }

        // 专家有私有 Skill → 加分
        let skill_count = agent.skills.get_skills().len();
        if skill_count > 0 {
            score += 0.05 * skill_count as f32;
        }

        score.min(1.0)
    }

    /// 计算历史表现分
    fn calculate_historical_score(&self, perf: &ExpertPerformance) -> f32 {
        if perf.total_tasks == 0 {
            return 0.5; // 新专家给中等分数
        }

        let success_rate = perf.successful_tasks as f32 / perf.total_tasks as f32;
        let satisfaction = perf.satisfaction_score;

        // 综合成功率和满意度
        success_rate * 0.6 + satisfaction * 0.4
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expert::{
        ExpertPersona, PluginCategory, PluginManifest, PluginPermissions, SandboxConfig,
    };
    use crate::memory::MemoryConfig;
    use crate::runtime::llm::MockLLM;
    use crate::runtime::RuntimeConfig;
    use crate::skill::DefaultChatSkill;

    fn create_test_orchestrator() -> Orchestrator {
        let skills = Arc::new(RwLock::new({
            let mut sm = SkillManager::new();
            sm.register(DefaultChatSkill);
            sm
        }));
        let memory = Arc::new(Memory::with_config(MemoryConfig::default()));
        let runtime = Arc::new(Runtime::with_config(RuntimeConfig::default()));

        Orchestrator::with_defaults(skills, memory, runtime)
    }

    /// 创建一个带 MockLLM 的 Runtime，用于测试策略执行
    fn create_mock_runtime(responses: Vec<&str>) -> Arc<Runtime> {
        let runtime = Arc::new(Runtime::with_config(RuntimeConfig::default()));
        let mock = MockLLM::new();
        for r in responses {
            mock.add_response(r);
        }
        runtime.set_mock_llm(mock);
        runtime
    }

    /// 创建一个带有模拟专家的编排器
    fn create_orchestrator_with_expert(
        expert_id: &str,
        domain: TaskDomain,
        system_prompt: &str,
    ) -> Orchestrator {
        let skills = Arc::new(RwLock::new({
            let mut sm = SkillManager::new();
            sm.register(DefaultChatSkill);
            sm
        }));
        let memory = Arc::new(Memory::with_config(MemoryConfig::default()));
        let runtime = create_mock_runtime(vec![&format!("[{}] 这是模拟的专家回复", expert_id)]);

        let mut orch = Orchestrator::with_defaults(skills, memory, runtime);

        let persona = ExpertPersona {
            name: format!("Expert-{}", expert_id),
            description: format!("Test expert {}", expert_id),
            tone: crate::soul::ToneStyle::Formal,
            emotional_tendency: crate::soul::EmotionalTendency::Neutral,
            big_five: crate::soul::BigFive::default(),
            traits: vec!["专业".to_string()],
            expertise_areas: {
                let mut m = std::collections::HashMap::new();
                m.insert(format!("{:?}", domain), 0.9);
                m
            },
            system_prompt: system_prompt.to_string(),
        };

        // 创建一个简单的 mock plugin
        let manifest = PluginManifest {
            id: expert_id.to_string(),
            name: format!("Expert-{}", expert_id),
            version: "1.0.0".to_string(),
            description: format!("Test expert {}", expert_id),
            category: PluginCategory::Other,
            keywords: vec![format!("{:?}", domain).to_lowercase()],
            author: None,
            permissions: PluginPermissions::default(),
            sandbox: SandboxConfig::default(),
            hooks: vec![],
            dependencies: vec![],
            min_framework_version: None,
            homepage: None,
            license: None,
        };

        let agent = ExpertAgent {
            id: expert_id.to_string(),
            name: format!("Expert-{}", expert_id),
            description: format!("Test expert {}", expert_id),
            domains: vec![domain.clone()],
            keywords: vec![format!("{:?}", domain).to_lowercase()],
            persona,
            skills: {
                let mut sm = SkillManager::new();
                sm.register(DefaultChatSkill);
                sm
            },
            memory: Memory::new(),
            performance: ExpertPerformance::default(),
            plugin: Arc::new(MockExpertPlugin { manifest }),
        };

        orch.register_expert(agent);
        orch
    }

    /// 一个简单的 Mock ExpertPlugin 用于测试
    struct MockExpertPlugin {
        manifest: PluginManifest,
    }

    impl crate::expert::ExpertPlugin for MockExpertPlugin {
        fn info(&self) -> ExpertInfo {
            ExpertInfo {
                id: self.manifest.id.clone(),
                name: self.manifest.name.clone(),
                description: self.manifest.description.clone(),
                version: self.manifest.version.clone(),
                author: self.manifest.author.clone().map(|a| a.name),
                category: self.manifest.category.to_string(),
                keywords: self.manifest.keywords.clone(),
            }
        }

        fn manifest(&self) -> PluginManifest {
            self.manifest.clone()
        }

        fn persona(&self) -> ExpertPersona {
            ExpertPersona {
                name: self.manifest.name.clone(),
                description: self.manifest.description.clone(),
                tone: crate::soul::ToneStyle::Formal,
                emotional_tendency: crate::soul::EmotionalTendency::Neutral,
                big_five: crate::soul::BigFive::default(),
                traits: vec!["专业".to_string()],
                expertise_areas: std::collections::HashMap::new(),
                system_prompt: "你是一个测试专家。".to_string(),
            }
        }

        fn skills(&self) -> Vec<Box<dyn crate::skill::Skill>> {
            vec![]
        }

        fn knowledge(&self) -> Vec<crate::expert::KnowledgeEntry> {
            vec![]
        }
    }

    // ═══════════════════════════════════════════════════════════
    // 任务画像分析测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_task_analysis_simple() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("你好，今天天气怎么样");

        assert!(profile.domains.contains(&TaskDomain::General));
        assert_eq!(profile.complexity, TaskComplexityLevel::Simple);
        assert!(!profile.needs_collaboration);
    }

    #[test]
    fn test_task_analysis_psychology() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("我最近压力很大，感觉很焦虑");

        assert!(profile.domains.contains(&TaskDomain::Psychology));
    }

    #[test]
    fn test_task_analysis_development() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("帮我写一个 Rust 函数，实现数据库连接池");

        assert!(profile.domains.contains(&TaskDomain::Development));
    }

    #[test]
    fn test_task_analysis_database() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("帮我优化 pgvector 向量索引的查询性能，需要修改数据库配置");

        assert!(profile.domains.contains(&TaskDomain::Database));
        // "数据库" 关键词同时匹配 Development 和 Database
        assert!(profile.domains.contains(&TaskDomain::Development));
    }

    #[test]
    fn test_task_analysis_architecture() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("帮我设计一个微服务架构方案");

        assert!(profile.domains.contains(&TaskDomain::Architecture));
    }

    #[test]
    fn test_task_analysis_complex() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task(
            "帮我设计一个微服务架构，先分析需求，然后设计数据库表结构，最后给出 Rust 代码实现",
        );

        assert!(profile.complexity != TaskComplexityLevel::Simple);
        assert!(profile.needs_collaboration);
        assert!(profile.has_dependencies);
    }

    #[test]
    fn test_task_analysis_multi_domain() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task(
            "帮我分析一下当前系统的数据库架构，然后写一段 Rust 代码来优化向量索引查询性能",
        );

        assert!(profile.domains.contains(&TaskDomain::Development));
        assert!(profile.domains.contains(&TaskDomain::Architecture));
        assert!(profile.domains.contains(&TaskDomain::Database));
    }

    #[test]
    fn test_task_analysis_user_intent_query() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("怎么使用 Rust 的 async/await?");

        assert_eq!(profile.user_intent, "知识查询");
    }

    #[test]
    fn test_task_analysis_user_intent_execution() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("帮我开发一个 REST API 接口");

        assert_eq!(profile.user_intent, "任务执行");
    }

    #[test]
    fn test_task_analysis_user_intent_analysis() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("分析对比一下微服务和单体架构的优劣");

        assert_eq!(profile.user_intent, "分析推理");
    }

    #[test]
    fn test_task_analysis_user_intent_chat() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("今天心情真好");

        assert_eq!(profile.user_intent, "对话交流");
    }

    #[test]
    fn test_task_analysis_needs_review() {
        let orch = create_test_orchestrator();
        let profile = orch.analyze_task("帮我评审一下这段代码的架构设计是否合理");

        assert!(profile.needs_review);
    }

    #[test]
    fn test_task_analysis_complexity_medium() {
        let orch = create_test_orchestrator();
        // 中等长度 + 多步骤关键词
        let input = "先分析用户需求，然后给出设计方案的详细说明。".repeat(3);
        let profile = orch.analyze_task(&input);

        assert!(
            profile.complexity == TaskComplexityLevel::Medium
                || profile.complexity == TaskComplexityLevel::Complex
        );
    }

    // ═══════════════════════════════════════════════════════════
    // 领域匹配计算测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_domain_matching_exact() {
        let orch = create_test_orchestrator();
        let task = vec![TaskDomain::Psychology];
        let expert = vec![TaskDomain::Psychology];

        let score = orch.calculate_domain_match(&task, &expert);
        assert!((score - 1.0).abs() < 0.01, "Expected 1.0, got {}", score);
    }

    #[test]
    fn test_domain_matching_partial() {
        let orch = create_test_orchestrator();
        let task = vec![TaskDomain::Psychology];
        let expert = vec![TaskDomain::Psychology, TaskDomain::General];

        let score = orch.calculate_domain_match(&task, &expert);
        assert!(score > 0.0);
        assert!(score < 1.0);
    }

    #[test]
    fn test_domain_matching_none() {
        let orch = create_test_orchestrator();
        let task = vec![TaskDomain::Psychology];
        let expert = vec![TaskDomain::Development];

        let score = orch.calculate_domain_match(&task, &expert);
        assert!((score - 0.0).abs() < 0.01, "Expected 0.0, got {}", score);
    }

    #[test]
    fn test_domain_matching_empty() {
        let orch = create_test_orchestrator();
        let task: Vec<TaskDomain> = vec![];
        let expert = vec![TaskDomain::Psychology];

        let score = orch.calculate_domain_match(&task, &expert);
        assert!((score - 0.0).abs() < 0.01);
    }

    // ═══════════════════════════════════════════════════════════
    // 策略决策测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_strategy_decision_simple() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::General],
            complexity: TaskComplexityLevel::Simple,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::SimpleDispatch);
    }

    #[test]
    fn test_strategy_decision_pipeline() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::Development, TaskDomain::Database],
            complexity: TaskComplexityLevel::Medium,
            has_dependencies: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::Pipeline);
    }

    #[test]
    fn test_strategy_decision_map_reduce() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::Development, TaskDomain::Psychology],
            complexity: TaskComplexityLevel::Medium,
            has_dependencies: false,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::MapReduce);
    }

    #[test]
    fn test_strategy_decision_manager_worker() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::Development, TaskDomain::Architecture],
            complexity: TaskComplexityLevel::VeryComplex,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::ManagerWorker);
    }

    #[test]
    fn test_strategy_decision_with_review() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::General],
            complexity: TaskComplexityLevel::Simple,
            needs_review: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::SimpleDispatch);
        assert!(decision.needs_critique);
    }

    // ═══════════════════════════════════════════════════════════
    // 专家池管理测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_register_expert() {
        let mut orch = create_test_orchestrator();
        assert_eq!(orch.expert_count(), 0);

        let agent = ExpertAgent {
            id: "test-001".to_string(),
            name: "Test Expert".to_string(),
            description: "A test expert".to_string(),
            domains: vec![TaskDomain::General],
            keywords: vec!["test".to_string()],
            persona: ExpertPersona {
                name: "Test".to_string(),
                description: "Test".to_string(),
                tone: crate::soul::ToneStyle::Formal,
                emotional_tendency: crate::soul::EmotionalTendency::Neutral,
                big_five: crate::soul::BigFive::default(),
                traits: vec![],
                expertise_areas: std::collections::HashMap::new(),
                system_prompt: "You are a test expert.".to_string(),
            },
            skills: SkillManager::new(),
            memory: Memory::new(),
            performance: ExpertPerformance::default(),
            plugin: Arc::new(MockExpertPlugin {
                manifest: PluginManifest {
                    id: "test-001".to_string(),
                    name: "Test Expert".to_string(),
                    version: "1.0.0".to_string(),
                    description: "Test".to_string(),
                    category: PluginCategory::Other,
                    keywords: vec![],
                    author: None,
                    permissions: PluginPermissions::default(),
                    sandbox: SandboxConfig::default(),
                    hooks: vec![],
                    dependencies: vec![],
                    min_framework_version: None,
                    homepage: None,
                    license: None,
                },
            }),
        };

        orch.register_expert(agent);
        assert_eq!(orch.expert_count(), 1);
        assert!(!orch.is_empty());
    }

    #[test]
    fn test_remove_expert() {
        let mut orch = create_test_orchestrator();
        let agent = ExpertAgent {
            id: "test-remove".to_string(),
            name: "Remove Me".to_string(),
            description: "Expert to be removed".to_string(),
            domains: vec![TaskDomain::General],
            keywords: vec![],
            persona: ExpertPersona {
                name: "Remove".to_string(),
                description: "Remove".to_string(),
                tone: crate::soul::ToneStyle::Formal,
                emotional_tendency: crate::soul::EmotionalTendency::Neutral,
                big_five: crate::soul::BigFive::default(),
                traits: vec![],
                expertise_areas: std::collections::HashMap::new(),
                system_prompt: "Remove me".to_string(),
            },
            skills: SkillManager::new(),
            memory: Memory::new(),
            performance: ExpertPerformance::default(),
            plugin: Arc::new(MockExpertPlugin {
                manifest: PluginManifest {
                    id: "test-remove".to_string(),
                    name: "Remove Me".to_string(),
                    version: "1.0.0".to_string(),
                    description: "Remove".to_string(),
                    category: PluginCategory::Other,
                    keywords: vec![],
                    author: None,
                    permissions: PluginPermissions::default(),
                    sandbox: SandboxConfig::default(),
                    hooks: vec![],
                    dependencies: vec![],
                    min_framework_version: None,
                    homepage: None,
                    license: None,
                },
            }),
        };

        orch.register_expert(agent);
        assert_eq!(orch.expert_count(), 1);

        let removed = orch.remove_expert("test-remove");
        assert!(removed.is_some());
        assert_eq!(orch.expert_count(), 0);
    }

    #[test]
    fn test_remove_nonexistent_expert() {
        let mut orch = create_test_orchestrator();
        let removed = orch.remove_expert("nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_list_experts_empty() {
        let orch = create_test_orchestrator();
        let experts = orch.list_experts();
        assert!(experts.is_empty());
    }

    #[test]
    fn test_get_expert_not_found() {
        let orch = create_test_orchestrator();
        assert!(orch.get_expert("nonexistent").is_none());
    }

    #[test]
    fn test_is_empty() {
        let orch = create_test_orchestrator();
        assert!(orch.is_empty());
    }

    #[test]
    fn test_get_general_expert_id_default() {
        let orch = create_test_orchestrator();
        assert_eq!(orch.get_general_expert_id(), "general");
    }

    #[test]
    fn test_set_general_expert() {
        let mut orch = create_test_orchestrator();
        orch.set_general_expert("custom-general");
        assert_eq!(orch.get_general_expert_id(), "custom-general");
    }

    // ═══════════════════════════════════════════════════════════
    // 专家匹配测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_match_experts_no_experts() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::Development],
            ..Default::default()
        };

        let matches = orch.match_experts(&profile, 5);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_experts_with_expert() {
        let orch = create_orchestrator_with_expert(
            "dev-001",
            TaskDomain::Development,
            "你是一个 Rust 开发专家。",
        );

        let profile = TaskProfile {
            input: "帮我写一个 Rust 函数".to_string(),
            domains: vec![TaskDomain::Development],
            ..Default::default()
        };

        let matches = orch.match_experts(&profile, 5);
        assert!(!matches.is_empty());
        assert!(matches[0].overall_score > 0.0);
    }

    #[test]
    fn test_best_expert_no_experts() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::General],
            ..Default::default()
        };

        assert!(orch.best_expert(&profile).is_none());
    }

    // ═══════════════════════════════════════════════════════════
    // 历史表现分计算测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_historical_score_new_expert() {
        let orch = create_test_orchestrator();
        let perf = ExpertPerformance::default();
        let score = orch.calculate_historical_score(&perf);
        assert!(
            (score - 0.5).abs() < 0.01,
            "New expert should have 0.5, got {}",
            score
        );
    }

    #[test]
    fn test_historical_score_perfect() {
        let orch = create_test_orchestrator();
        let perf = ExpertPerformance {
            total_tasks: 100,
            successful_tasks: 100,
            failed_tasks: 0,
            avg_duration_ms: 100,
            satisfaction_score: 1.0,
        };
        let score = orch.calculate_historical_score(&perf);
        assert!((score - 1.0).abs() < 0.01, "Expected 1.0, got {}", score);
    }

    #[test]
    fn test_historical_score_half() {
        let orch = create_test_orchestrator();
        let perf = ExpertPerformance {
            total_tasks: 100,
            successful_tasks: 50,
            failed_tasks: 50,
            avg_duration_ms: 200,
            satisfaction_score: 0.5,
        };
        let score = orch.calculate_historical_score(&perf);
        // 0.5 * 0.6 + 0.5 * 0.4 = 0.3 + 0.2 = 0.5
        assert!((score - 0.5).abs() < 0.01, "Expected 0.5, got {}", score);
    }

    // ═══════════════════════════════════════════════════════════
    // 策略执行测试（使用 MockLLM）
    // ═══════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_simple_dispatch_strategy_execution() {
        let orch = create_orchestrator_with_expert(
            "dev-001",
            TaskDomain::Development,
            "你是一个 Rust 开发专家。",
        );

        let profile = TaskProfile {
            input: "帮我写一个函数".to_string(),
            domains: vec![TaskDomain::Development],
            complexity: TaskComplexityLevel::Simple,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);

        let result = strategies::simple_dispatch::execute(&orch, &profile, &decision)
            .await
            .expect("SimpleDispatch should succeed");

        assert_eq!(result.strategy, DispatchStrategy::SimpleDispatch);
        assert!(!result.expert_chain.is_empty());
        assert!(!result.output.is_empty());
    }

    #[tokio::test]
    async fn test_simple_dispatch_no_expert_fallback() {
        let skills = Arc::new(RwLock::new({
            let mut sm = SkillManager::new();
            sm.register(DefaultChatSkill);
            sm
        }));
        let memory = Arc::new(Memory::with_config(MemoryConfig::default()));
        let runtime = create_mock_runtime(vec!["mock response"]);

        let orch = Orchestrator::with_defaults(skills, memory, runtime);

        let profile = TaskProfile {
            input: "测试问题".to_string(),
            domains: vec![TaskDomain::General],
            complexity: TaskComplexityLevel::Simple,
            ..Default::default()
        };

        // 空专家池，decision 也不会有 matched experts
        let decision = orch.decide_strategy(&profile);

        let result = strategies::simple_dispatch::execute(&orch, &profile, &decision).await;

        // 没有匹配专家时应该返回错误
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_pipeline_strategy_execution() {
        let mut orch = create_orchestrator_with_expert(
            "arch-001",
            TaskDomain::Architecture,
            "你是一个架构设计专家。",
        );

        // 注册第二个专家
        let persona2 = ExpertPersona {
            name: "Expert-dev-002".to_string(),
            description: "Development expert".to_string(),
            tone: crate::soul::ToneStyle::Formal,
            emotional_tendency: crate::soul::EmotionalTendency::Neutral,
            big_five: crate::soul::BigFive::default(),
            traits: vec!["专业".to_string()],
            expertise_areas: std::collections::HashMap::new(),
            system_prompt: "你是一个开发专家。".to_string(),
        };

        let manifest2 = PluginManifest {
            id: "dev-002".to_string(),
            name: "Expert-dev-002".to_string(),
            version: "1.0.0".to_string(),
            description: "Dev expert".to_string(),
            category: PluginCategory::Other,
            keywords: vec!["development".to_string()],
            author: None,
            permissions: PluginPermissions::default(),
            sandbox: SandboxConfig::default(),
            hooks: vec![],
            dependencies: vec![],
            min_framework_version: None,
            homepage: None,
            license: None,
        };

        let agent2 = ExpertAgent {
            id: "dev-002".to_string(),
            name: "Expert-dev-002".to_string(),
            description: "Development expert".to_string(),
            domains: vec![TaskDomain::Development],
            keywords: vec!["development".to_string()],
            persona: persona2,
            skills: SkillManager::new(),
            memory: Memory::new(),
            performance: ExpertPerformance::default(),
            plugin: Arc::new(MockExpertPlugin {
                manifest: manifest2,
            }),
        };

        orch.register_expert(agent2);

        let profile = TaskProfile {
            input: "先设计架构，然后实现代码".to_string(),
            domains: vec![TaskDomain::Architecture, TaskDomain::Development],
            complexity: TaskComplexityLevel::Medium,
            has_dependencies: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::Pipeline);

        let result = strategies::pipeline::execute(&orch, &profile, &decision)
            .await
            .expect("Pipeline should succeed");

        assert_eq!(result.strategy, DispatchStrategy::Pipeline);
        assert!(!result.expert_chain.is_empty());
    }

    #[tokio::test]
    async fn test_pipeline_builds_stages() {
        let orch =
            create_orchestrator_with_expert("arch-001", TaskDomain::Architecture, "架构专家。");

        let profile = TaskProfile {
            input: "设计系统架构".to_string(),
            domains: vec![TaskDomain::Architecture, TaskDomain::Development],
            complexity: TaskComplexityLevel::Medium,
            has_dependencies: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);

        let result = strategies::pipeline::execute(&orch, &profile, &decision)
            .await
            .expect("Pipeline should succeed");

        assert!(!result.output.is_empty());
    }

    #[tokio::test]
    async fn test_critique_revise_satisfactory_check() {
        // 测试 is_satisfactory 函数
        assert!(strategies::critique_revise::is_satisfactory_test(
            "SATISFACTORY"
        ));
        assert!(strategies::critique_revise::is_satisfactory_test(
            "这个回答很满意"
        ));
        assert!(strategies::critique_revise::is_satisfactory_test("很好"));
        assert!(strategies::critique_revise::is_satisfactory_test(
            "无需修改"
        ));
        assert!(strategies::critique_revise::is_satisfactory_test(
            "评审通过"
        ));
        assert!(strategies::critique_revise::is_satisfactory_test("PASS"));
        assert!(strategies::critique_revise::is_satisfactory_test(
            "approved"
        ));
        assert!(!strategies::critique_revise::is_satisfactory_test(
            "需要改进"
        ));
        assert!(!strategies::critique_revise::is_satisfactory_test(
            "这个回答不够好"
        ));
    }

    #[tokio::test]
    async fn test_critique_revise_strategy_execution() {
        let orch = create_orchestrator_with_expert(
            "writer-001",
            TaskDomain::Writing,
            "你是一个写作专家。",
        );

        let profile = TaskProfile {
            input: "写一篇短文".to_string(),
            domains: vec![TaskDomain::Writing],
            complexity: TaskComplexityLevel::Simple,
            needs_review: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);

        // CritiqueRevise 先调用 simple_dispatch，然后评审迭代
        let result = strategies::critique_revise::execute(&orch, &profile, &decision)
            .await
            .expect("CritiqueRevise should succeed");

        assert_eq!(result.strategy, DispatchStrategy::CritiqueRevise);
        assert!(!result.output.is_empty());
        // 至少有一条评审记录（MockLLM 会返回默认内容，可能触发不满意度）
    }

    #[tokio::test]
    async fn test_critique_revise_on_result() {
        let orch = create_orchestrator_with_expert("writer-001", TaskDomain::Writing, "写作专家。");

        let profile = TaskProfile {
            input: "写一篇短文".to_string(),
            domains: vec![TaskDomain::Writing],
            complexity: TaskComplexityLevel::Simple,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);

        let result = strategies::critique_revise::execute_on_result(
            &orch,
            &profile,
            &decision,
            "这是一篇测试文章。",
        )
        .await
        .expect("CritiqueRevise on result should succeed");

        assert_eq!(result.strategy, DispatchStrategy::CritiqueRevise);
        assert!(!result.output.is_empty());
    }

    // ═══════════════════════════════════════════════════════════
    // 编排器配置测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.domain_match_weight, 0.6);
        assert_eq!(config.capability_weight, 0.2);
        assert_eq!(config.historical_weight, 0.1);
        assert_eq!(config.load_weight, 0.1);
        assert_eq!(config.max_critique_rounds, 3);
        assert!(config.auto_strategy);
    }

    #[test]
    fn test_orchestrator_config_custom() {
        let config = OrchestratorConfig {
            domain_match_weight: 0.5,
            capability_weight: 0.3,
            historical_weight: 0.1,
            load_weight: 0.1,
            max_critique_rounds: 5,
            auto_strategy: false,
        };
        assert_eq!(config.max_critique_rounds, 5);
        assert!(!config.auto_strategy);
    }

    // ═══════════════════════════════════════════════════════════
    // TaskProfile 默认值测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_task_profile_default() {
        let profile = TaskProfile::default();
        assert!(profile.input.is_empty());
        assert_eq!(profile.domains.len(), 1);
        assert!(profile.domains.contains(&TaskDomain::General));
        assert_eq!(profile.complexity, TaskComplexityLevel::Simple);
        assert!(!profile.needs_collaboration);
        assert!(!profile.has_dependencies);
        assert!(!profile.needs_review);
        assert!(profile.sub_tasks.is_empty());
        assert_eq!(profile.estimated_steps, 1);
    }

    // ═══════════════════════════════════════════════════════════
    // SubTask 和 SubTaskStatus 测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_subtask_creation() {
        let subtask = SubTask {
            id: "task-1".to_string(),
            description: "分析需求".to_string(),
            domain: TaskDomain::Architecture,
            dependencies: vec![],
            assigned_expert: None,
            result: None,
            status: SubTaskStatus::Pending,
        };

        assert_eq!(subtask.id, "task-1");
        assert_eq!(subtask.status, SubTaskStatus::Pending);
        assert!(subtask.dependencies.is_empty());
    }

    #[test]
    fn test_subtask_with_dependencies() {
        let subtask = SubTask {
            id: "task-2".to_string(),
            description: "实现代码".to_string(),
            domain: TaskDomain::Development,
            dependencies: vec!["task-1".to_string()],
            assigned_expert: Some("dev-001".to_string()),
            result: Some("代码已实现".to_string()),
            status: SubTaskStatus::Completed,
        };

        assert_eq!(subtask.dependencies.len(), 1);
        assert_eq!(subtask.dependencies[0], "task-1");
        assert_eq!(subtask.status, SubTaskStatus::Completed);
        assert!(subtask.result.is_some());
    }

    // ═══════════════════════════════════════════════════════════
    // 领域映射测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_map_domains_from_category_psychology() {
        let domains = Orchestrator::map_domains(&["心理".to_string()], &PluginCategory::Psychology);
        assert!(domains.contains(&TaskDomain::Psychology));
    }

    #[test]
    fn test_map_domains_from_category_development() {
        let domains =
            Orchestrator::map_domains(&["rust".to_string()], &PluginCategory::Development);
        assert!(domains.contains(&TaskDomain::Development));
    }

    #[test]
    fn test_map_domains_from_keywords_database() {
        let domains = Orchestrator::map_domains(
            &["数据库".to_string(), "向量".to_string()],
            &PluginCategory::Other,
        );
        assert!(domains.contains(&TaskDomain::Database));
    }

    #[test]
    fn test_map_domains_from_keywords_architecture() {
        let domains = Orchestrator::map_domains(
            &["架构".to_string(), "设计".to_string()],
            &PluginCategory::Other,
        );
        assert!(domains.contains(&TaskDomain::Architecture));
    }

    #[test]
    fn test_map_domains_fallback_to_general() {
        let domains = Orchestrator::map_domains(&["未知".to_string()], &PluginCategory::Other);
        assert!(domains.contains(&TaskDomain::General));
        assert_eq!(domains.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════
    // DispatchDecision 测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_dispatch_decision_reason() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::General],
            complexity: TaskComplexityLevel::Simple,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert!(!decision.reason.is_empty());
        assert_eq!(decision.strategy, DispatchStrategy::SimpleDispatch);
    }

    #[test]
    fn test_dispatch_decision_pipeline_reason() {
        let orch = create_test_orchestrator();
        let profile = TaskProfile {
            domains: vec![TaskDomain::Development, TaskDomain::Database],
            complexity: TaskComplexityLevel::Medium,
            has_dependencies: true,
            ..Default::default()
        };

        let decision = orch.decide_strategy(&profile);
        assert_eq!(decision.strategy, DispatchStrategy::Pipeline);
        assert!(decision.reason.contains("流水线"));
    }

    // ═══════════════════════════════════════════════════════════
    // OrchestrationResult 结构测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_orchestration_result_default_fields() {
        let result = OrchestrationResult {
            output: "测试输出".to_string(),
            strategy: DispatchStrategy::SimpleDispatch,
            expert_chain: vec!["expert-1".to_string()],
            expert_outputs: std::collections::HashMap::new(),
            tokens: crate::context::TokenStats::default(),
            duration_ms: 0,
            critique_records: vec![],
        };

        assert_eq!(result.strategy, DispatchStrategy::SimpleDispatch);
        assert_eq!(result.expert_chain.len(), 1);
        assert!(result.critique_records.is_empty());
        assert_eq!(result.duration_ms, 0);
    }

    // ═══════════════════════════════════════════════════════════
    // 完整编排流程测试（execute）
    // ═══════════════════════════════════════════════════════════

    #[tokio::test]
    async fn test_execute_simple_task() {
        let orch = create_orchestrator_with_expert(
            "general-001",
            TaskDomain::General,
            "你是一个通用助手。",
        );

        let result = orch
            .execute("你好，今天天气怎么样？", "user-001")
            .await
            .expect("Execute should succeed");

        assert!(!result.output.is_empty());
        // duration_ms 在 mock 环境下可能为 0（执行太快），u64 永远 >= 0，无需断言
    }

    #[tokio::test]
    async fn test_execute_with_strategy_override() {
        let orch = create_orchestrator_with_expert(
            "general-001",
            TaskDomain::General,
            "你是一个通用助手。",
        );

        let result = orch
            .execute_with_strategy("你好", "user-001", DispatchStrategy::SimpleDispatch)
            .await
            .expect("Execute with strategy should succeed");

        assert_eq!(result.strategy, DispatchStrategy::SimpleDispatch);
    }

    // ═══════════════════════════════════════════════════════════
    // ExpertPerformance 测试
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn test_expert_performance_default() {
        let perf = ExpertPerformance::default();
        assert_eq!(perf.total_tasks, 0);
        assert_eq!(perf.successful_tasks, 0);
        assert_eq!(perf.failed_tasks, 0);
        assert_eq!(perf.avg_duration_ms, 0);
        assert_eq!(perf.satisfaction_score, 0.0);
    }
}
