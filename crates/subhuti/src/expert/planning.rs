//! # 专家自主规划接口
//!
//! 定义专家插件的规划能力规范：
//! - **Planning Trait**：规划能力抽象
//! - **Task 拆解**：将复杂任务分解为子任务
//! - **进度追踪**：记录规划执行进度
//! - **反思调整**：执行后评估并调整计划
//!
//! ## 设计理念
//!
//! 专家插件可以声明自己的规划能力，主框架调用规划接口执行复杂任务。
//! 这样让专家成为"领域专家 + 规划师"，而不是简单的 Skill 集合。
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                   ExpertPlanning                       │
//! │                                                       │
//! │  ┌───────────────┐   ┌───────────────┐              │
//! │  │ analyze_task  │   │  create_plan  │              │
//! │  │   分析任务    │   │   制定计划    │              │
//! │  └───────────────┘   └───────────────┘              │
//! │                                                       │
//! │  ┌───────────────┐   ┌───────────────┐              │
//! │  │ execute_step  │   │  reflect_on   │              │
//! │  │   执行步骤    │   │   反思调整    │              │
//! │  └───────────────┘   └───────────────┘              │
//! │                                                       │
//! └─────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 任务复杂度等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskComplexity {
    /// 简单任务（单步即可完成）
    Simple,
    /// 中等复杂度（需要 2-5 步）
    Medium,
    /// 复杂任务（需要多个步骤和规划）
    Complex,
    /// 极复杂（需要分解为子任务，可能有依赖关系）
    VeryComplex,
}

/// 任务类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskType {
    /// 信息查询（如搜索、检索）
    InformationQuery,
    /// 问题解答（如知识问答）
    QuestionAnswering,
    /// 任务执行（如创建、修改、删除）
    TaskExecution,
    /// 分析推理（如数据分析、逻辑推理）
    AnalysisReasoning,
    /// 创意生成（如写作、设计）
    CreativeGeneration,
    /// 规划决策（如制定方案、决策建议）
    PlanningDecision,
    /// 对话交流（如聊天、咨询）
    Conversation,
    /// 多步骤流程（如办理流程、工作流）
    MultiStepWorkflow,
}

/// 任务分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAnalysis {
    /// 任务类型
    pub task_type: TaskType,
    /// 复杂度等级
    pub complexity: TaskComplexity,
    /// 核心目标
    pub core_goal: String,
    /// 子目标列表
    pub sub_goals: Vec<String>,
    /// 需要的能力
    pub required_capabilities: Vec<String>,
    /// 预估步骤数
    pub estimated_steps: usize,
    /// 是否需要工具调用
    pub needs_tools: bool,
    /// 是否需要外部信息
    pub needs_external_info: bool,
    /// 优先级（1-10）
    pub priority: u8,
    /// 紧急程度（1-10）
    pub urgency: u8,
    /// 用户意图分析
    pub user_intent: String,
}

impl Default for TaskAnalysis {
    fn default() -> Self {
        Self {
            task_type: TaskType::Conversation,
            complexity: TaskComplexity::Simple,
            core_goal: String::new(),
            sub_goals: Vec::new(),
            required_capabilities: Vec::new(),
            estimated_steps: 1,
            needs_tools: false,
            needs_external_info: false,
            priority: 5,
            urgency: 5,
            user_intent: String::new(),
        }
    }
}

/// 规划步骤
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    /// 步骤 ID
    pub id: String,
    /// 步骤序号
    pub sequence: usize,
    /// 步骤名称
    pub name: String,
    /// 步骤描述
    pub description: String,
    /// 使用的 Skill 名称（可选）
    pub skill_name: Option<String>,
    /// 使用的工具名称（可选）
    pub tool_names: Vec<String>,
    /// 输入数据
    pub input: Option<serde_json::Value>,
    /// 输出数据（执行后填充）
    pub output: Option<serde_json::Value>,
    /// 状态
    pub status: StepStatus,
    /// 开始时间
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 结束时间
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 持续时间（毫秒）
    pub duration_ms: Option<u64>,
    /// 依赖的步骤 ID（必须先完成的步骤）
    pub dependencies: Vec<String>,
    /// 错误信息
    pub error: Option<String>,
    /// 备注
    pub notes: Option<String>,
}

impl PlanStep {
    pub fn new(sequence: usize, name: String, description: String) -> Self {
        Self {
            id: format!("step_{}", sequence),
            sequence,
            name,
            description,
            skill_name: None,
            tool_names: Vec::new(),
            input: None,
            output: None,
            status: StepStatus::Pending,
            started_at: None,
            ended_at: None,
            duration_ms: None,
            dependencies: Vec::new(),
            error: None,
            notes: None,
        }
    }

    pub fn with_skill(mut self, skill_name: &str) -> Self {
        self.skill_name = Some(skill_name.to_string());
        self
    }

    pub fn with_tools(mut self, tool_names: Vec<String>) -> Self {
        self.tool_names = tool_names;
        self
    }

    pub fn with_dependencies(mut self, dependencies: Vec<String>) -> Self {
        self.dependencies = dependencies;
        self
    }

    pub fn start(&mut self) {
        self.status = StepStatus::Running;
        self.started_at = Some(chrono::Utc::now());
    }

    pub fn complete(&mut self, output: serde_json::Value) {
        self.status = StepStatus::Completed;
        self.output = Some(output);
        self.ended_at = Some(chrono::Utc::now());
        if let Some(start) = self.started_at {
            self.duration_ms = Some((chrono::Utc::now() - start).num_milliseconds() as u64);
        }
    }

    pub fn fail(&mut self, error: String) {
        self.status = StepStatus::Failed;
        self.error = Some(error);
        self.ended_at = Some(chrono::Utc::now());
    }
}

/// 步骤状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    /// 待执行
    Pending,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已跳过
    Skipped,
    /// 已取消
    Cancelled,
}

/// 执行计划
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// 计划 ID
    pub id: String,
    /// 计划名称
    pub name: String,
    /// 计划描述
    pub description: String,
    /// 目标任务
    pub goal: String,
    /// 任务分析结果
    pub analysis: TaskAnalysis,
    /// 步骤列表
    pub steps: Vec<PlanStep>,
    /// 创建时间
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// 开始执行时间
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 结束时间
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    /// 总耗时（毫秒）
    pub total_duration_ms: Option<u64>,
    /// 当前步骤索引
    pub current_step_index: usize,
    /// 计划状态
    pub status: PlanStatus,
    /// 元数据
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ExecutionPlan {
    pub fn new(goal: String, analysis: TaskAnalysis) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Plan for: {}", goal),
            description: String::new(),
            goal,
            analysis,
            steps: Vec::new(),
            created_at: chrono::Utc::now(),
            started_at: None,
            ended_at: None,
            total_duration_ms: None,
            current_step_index: 0,
            status: PlanStatus::Created,
            metadata: HashMap::new(),
        }
    }

    pub fn add_step(&mut self, step: PlanStep) {
        self.steps.push(step);
    }

    pub fn start(&mut self) {
        self.status = PlanStatus::Running;
        self.started_at = Some(chrono::Utc::now());
    }

    pub fn complete(&mut self) {
        self.status = PlanStatus::Completed;
        self.ended_at = Some(chrono::Utc::now());
        if let Some(start) = self.started_at {
            self.total_duration_ms = Some((chrono::Utc::now() - start).num_milliseconds() as u64);
        }
    }

    pub fn fail(&mut self, error: String) {
        self.status = PlanStatus::Failed;
        self.metadata
            .insert("error".to_string(), serde_json::json!(error));
        self.ended_at = Some(chrono::Utc::now());
    }

    /// 获取当前待执行的步骤
    pub fn current_step(&self) -> Option<&PlanStep> {
        self.steps.iter().find(|s| s.status == StepStatus::Pending)
    }

    /// 获取下一步（考虑依赖）
    pub fn next_step(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .filter(|s| s.status == StepStatus::Pending)
            .find(|s| {
                // 检查所有依赖是否已完成
                s.dependencies.iter().all(|dep_id| {
                    self.steps
                        .iter()
                        .find(|step| step.id == *dep_id)
                        .map(|step| step.status == StepStatus::Completed)
                        .unwrap_or(false)
                })
            })
    }

    /// 获取进度百分比
    pub fn progress_percent(&self) -> f32 {
        if self.steps.is_empty() {
            return 100.0;
        }
        let completed = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        (completed as f32 / self.steps.len() as f32) * 100.0
    }

    /// 获取执行摘要
    pub fn summary(&self) -> PlanSummary {
        let completed_steps = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .count();
        let failed_steps = self
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Failed)
            .count();

        PlanSummary {
            plan_id: self.id.clone(),
            goal: self.goal.clone(),
            total_steps: self.steps.len(),
            completed_steps,
            failed_steps,
            current_step_index: self.current_step_index,
            progress_percent: self.progress_percent(),
            status: self.status,
            duration_ms: self.total_duration_ms,
        }
    }
}

/// 计划状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    /// 已创建
    Created,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 已失败
    Failed,
    /// 已取消
    Cancelled,
}

/// 计划摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSummary {
    pub plan_id: String,
    pub goal: String,
    pub total_steps: usize,
    pub completed_steps: usize,
    pub failed_steps: usize,
    pub current_step_index: usize,
    pub progress_percent: f32,
    pub status: PlanStatus,
    pub duration_ms: Option<u64>,
}

/// 反思结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    /// 执行是否成功
    pub success: bool,
    /// 目标完成度（0-1）
    pub goal_completion: f32,
    /// 问题分析
    pub issues: Vec<String>,
    /// 改进建议
    pub improvements: Vec<String>,
    /// 是否需要调整计划
    pub needs_adjustment: bool,
    /// 新的计划步骤（如果需要调整）
    pub new_steps: Vec<PlanStep>,
    /// 学到的经验
    pub lessons_learned: Vec<String>,
}

impl Default for Reflection {
    fn default() -> Self {
        Self {
            success: true,
            goal_completion: 1.0,
            issues: Vec::new(),
            improvements: Vec::new(),
            needs_adjustment: false,
            new_steps: Vec::new(),
            lessons_learned: Vec::new(),
        }
    }
}

/// 专家规划 Trait
///
/// 专家插件实现此 Trait 以声明自己的规划能力
pub trait ExpertPlanning: Send + Sync {
    /// 分析任务
    ///
    /// 输入：用户的原始请求
    /// 输出：任务分析结果（类型、复杂度、目标等）
    fn analyze_task(&self, input: &str, context: &PlanningContext) -> TaskAnalysis;

    /// 创建执行计划
    ///
    /// 输入：任务分析结果
    /// 输出：详细的执行计划（步骤列表）
    fn create_plan(&self, analysis: &TaskAnalysis) -> ExecutionPlan;

    /// 执行单个步骤
    ///
    /// 输入：计划、当前步骤
    /// 输出：步骤执行结果
    fn execute_step(
        &self,
        plan: &mut ExecutionPlan,
        step: &mut PlanStep,
        executor: &PlanExecutor,
    ) -> Result<serde_json::Value, String>;

    /// 反思执行结果
    ///
    /// 输入：执行计划和结果
    /// 输出：反思分析（是否成功、问题、改进建议）
    fn reflect_on(&self, plan: &ExecutionPlan, result: &serde_json::Value) -> Reflection;

    /// 调整计划（可选）
    ///
    /// 输入：原计划、反思结果
    /// 输出：调整后的计划
    fn adjust_plan(&self, plan: &ExecutionPlan, reflection: &Reflection) -> Option<ExecutionPlan> {
        if reflection.needs_adjustment && !reflection.new_steps.is_empty() {
            let mut new_plan = plan.clone();
            for step in &reflection.new_steps {
                new_plan.add_step(step.clone());
            }
            Some(new_plan)
        } else {
            None
        }
    }

    /// 检查是否需要规划（简单任务不需要）
    fn needs_planning(&self, analysis: &TaskAnalysis) -> bool {
        analysis.complexity != TaskComplexity::Simple
    }

    /// 获取规划能力描述
    fn planning_description(&self) -> String {
        "具备任务分析和规划执行能力".to_string()
    }
}

/// 规划上下文（提供给规划器的环境信息）
#[derive(Debug, Clone, Default)]
pub struct PlanningContext {
    /// 用户 ID
    pub user_id: String,
    /// 会话 ID
    pub session_id: String,
    /// 当前专家 ID
    pub current_expert: Option<String>,
    /// 已有的记忆内容
    pub relevant_memories: Vec<String>,
    /// 可用的 Skill 列表
    pub available_skills: Vec<String>,
    /// 可用的工具列表
    pub available_tools: Vec<String>,
    /// 用户历史偏好
    pub user_preferences: HashMap<String, String>,
}

/// 规划执行器（执行步骤的代理）
pub struct PlanExecutor {
    /// Skill 调用函数
    skill_executor: Option<SkillExecutorFn>,
    /// 工具调用函数
    tool_executor: Option<ToolExecutorFn>,
}

/// Skill 执行函数类型
type SkillExecutorFn =
    Box<dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;

/// 工具执行函数类型
type ToolExecutorFn =
    Box<dyn Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync>;

impl PlanExecutor {
    pub fn new() -> Self {
        Self {
            skill_executor: None,
            tool_executor: None,
        }
    }

    pub fn with_skill_executor<F>(mut self, executor: F) -> Self
    where
        F: Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        self.skill_executor = Some(Box::new(executor));
        self
    }

    pub fn with_tool_executor<F>(mut self, executor: F) -> Self
    where
        F: Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String>
            + Send
            + Sync
            + 'static,
    {
        self.tool_executor = Some(Box::new(executor));
        self
    }

    pub fn execute_skill(
        &self,
        skill_name: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if let Some(executor) = &self.skill_executor {
            executor(skill_name, input)
        } else {
            Err("Skill executor not configured".to_string())
        }
    }

    pub fn execute_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if let Some(executor) = &self.tool_executor {
            executor(tool_name, input)
        } else {
            Err("Tool executor not configured".to_string())
        }
    }
}

impl Default for PlanExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_analysis() {
        let analysis = TaskAnalysis {
            task_type: TaskType::QuestionAnswering,
            complexity: TaskComplexity::Medium,
            core_goal: "回答用户问题".to_string(),
            estimated_steps: 2,
            ..Default::default()
        };
        assert_eq!(analysis.task_type, TaskType::QuestionAnswering);
    }

    #[test]
    fn test_execution_plan() {
        let analysis = TaskAnalysis::default();
        let mut plan = ExecutionPlan::new("测试目标".to_string(), analysis);

        plan.add_step(PlanStep::new(
            1,
            "第一步".to_string(),
            "测试描述".to_string(),
        ));
        plan.add_step(PlanStep::new(
            2,
            "第二步".to_string(),
            "测试描述".to_string(),
        ));

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.status, PlanStatus::Created);
    }

    #[test]
    fn test_step_execution() {
        let mut step = PlanStep::new(1, "测试步骤".to_string(), "测试".to_string());
        step.start();
        assert_eq!(step.status, StepStatus::Running);

        step.complete(serde_json::json!({ "result": "success" }));
        assert_eq!(step.status, StepStatus::Completed);
        assert!(step.output.is_some());
        assert!(step.duration_ms.is_some());
    }

    #[test]
    fn test_plan_progress() {
        let analysis = TaskAnalysis::default();
        let mut plan = ExecutionPlan::new("目标".to_string(), analysis);

        plan.add_step(PlanStep::new(1, "step1".to_string(), "".to_string()));
        plan.add_step(PlanStep::new(2, "step2".to_string(), "".to_string()));
        plan.add_step(PlanStep::new(3, "step3".to_string(), "".to_string()));

        // 完成第一步
        plan.steps[0].status = StepStatus::Completed;

        // 浮点数精度问题，使用约等于比较
        let progress = plan.progress_percent();
        assert!(progress > 30.0 && progress < 40.0);
    }

    #[test]
    fn test_step_dependencies() {
        let analysis = TaskAnalysis::default();
        let mut plan = ExecutionPlan::new("目标".to_string(), analysis);

        let step1 = PlanStep::new(1, "step1".to_string(), "".to_string());
        let step1_id = step1.id.clone();

        let step2 =
            PlanStep::new(2, "step2".to_string(), "".to_string()).with_dependencies(vec![step1_id]);

        plan.add_step(step1);
        plan.add_step(step2);

        // step1 未完成，step2 不能执行
        let next = plan.next_step();
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, "step_1");

        // 完成 step1
        plan.steps[0].status = StepStatus::Completed;

        // 现在 step2 可以执行
        let next = plan.next_step();
        assert!(next.is_some());
        assert_eq!(next.unwrap().id, "step_2");
    }
}
