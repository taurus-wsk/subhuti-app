//! # 领域层核心接口
//!
//! 定义领域专家的纯业务接口，不依赖任何外部框架类型。
//!
//! 六边形架构：
//! - 领域层定义接口（DomainExpert）
//! - 出站适配层实现适配器（DomainExpert → subhuti_core::ExpertAgent）
//! - 应用层通过出站端口（ExpertRepositoryPort）管理专家
//!
//! 这样领域层可以完全独立于框架，便于测试和提取为独立 crate。

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::domain::ports::CommandPort;
use crate::domain::ports::FileSystemPort;
use crate::domain::ports::ToolchainPort;

/// 领域技能信息（纯领域 DTO）
///
/// 专家直接暴露拥有的技能，Graph 不管理技能。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DomainSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
}

/// 领域专家上下文（纯领域类型）
///
/// 封装专家执行时所需的上下文信息，不依赖框架类型。
#[derive(Debug, Clone)]
pub struct DomainContext {
    pub input: String,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
    /// 项目工作目录路径（前端聊天设置传入）
    pub workspace_folder: Option<String>,
    /// 自定义系统提示词（前端聊天设置传入，覆盖专家默认 system prompt）
    pub system_prompt: Option<String>,
    /// 历史消息（从框架 Session 传递，用于多轮对话上下文）
    pub history: Vec<DomainMessage>,
}

/// 领域执行上下文（纯领域类型）
///
/// 封装专家执行时所需的所有依赖，避免参数膨胀。
/// 未来添加新依赖（如 EventBus、Config）只需扩展此结构体。
pub struct DomainExecutionContext {
    /// 业务上下文（输入、会话、用户等）
    pub ctx: DomainContext,
    /// LLM 客户端
    pub llm: Arc<dyn DomainLlm>,
    /// 数据仓库
    pub repository: Arc<dyn DomainRepository>,
    /// 技能ID（可选，用于技能执行）
    pub skill_id: Option<String>,
    /// 技能参数（可选）
    pub skill_params: Option<String>,
    /// Rust 工具链（可选，RustExpert 等需要编译验证的专家使用）
    pub toolchain: Option<Arc<dyn ToolchainPort>>,
    /// 藏经阁记忆引擎（可选，专家可访问结构化记忆系统）
    pub sutra_library: Option<Arc<dyn subhuti_core::SutraLibraryPort>>,
    /// 文件系统操作（可选，用于读写项目文件、搜索文件等）
    pub file_system: Option<Arc<dyn FileSystemPort>>,
    /// 命令行执行（可选，用于运行 cargo build、git 等命令）
    pub command: Option<Arc<dyn CommandPort>>,
    /// 进度报告通道（可选，用于实时推送执行进度到 SSE 流）
    pub progress_tx: Option<mpsc::Sender<String>>,
}

impl Clone for DomainExecutionContext {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            llm: self.llm.clone(),
            repository: self.repository.clone(),
            skill_id: self.skill_id.clone(),
            skill_params: self.skill_params.clone(),
            toolchain: self.toolchain.clone(),
            sutra_library: self.sutra_library.clone(),
            file_system: self.file_system.clone(),
            command: self.command.clone(),
            progress_tx: self.progress_tx.clone(),
        }
    }
}

impl std::fmt::Debug for DomainExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainExecutionContext")
            .field("ctx", &self.ctx)
            .field("skill_id", &self.skill_id)
            .field("skill_params", &self.skill_params)
            .field("has_sutra_library", &self.sutra_library.is_some())
            .field("has_file_system", &self.file_system.is_some())
            .field("has_command", &self.command.is_some())
            .field("has_progress_tx", &self.progress_tx.is_some())
            .finish()
    }
}

/// 领域专家 trait（纯领域接口）
///
/// 定义领域专家的核心能力：识别、技能暴露、执行。
/// 不依赖 subhuti 框架类型，便于独立测试和提取。
#[async_trait]
pub trait DomainExpert: Send + Sync {
    /// 获取专家唯一标识
    fn id(&self) -> &str;

    /// 获取专家名称
    fn name(&self) -> &str;

    /// 获取专家标签（用于匹配和分类）
    fn tags(&self) -> &[String];

    /// 获取专家拥有的技能列表
    fn skills(&self) -> &[DomainSkill];

    /// 执行专家逻辑（通用入口）
    ///
    /// # 参数
    /// - `exec_ctx`: 领域执行上下文，封装所有依赖
    ///
    /// # 返回
    /// - 执行结果字符串
    async fn run(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String>;

    /// 执行指定技能
    ///
    /// 根据技能ID执行对应的技能逻辑，每个专家可以实现多个技能。
    /// 默认实现：构建技能执行上下文，调用通用 run 方法。
    ///
    /// # 参数
    /// - `skill_id`: 技能ID
    /// - `params`: 技能参数（JSON格式字符串）
    /// - `exec_ctx`: 领域执行上下文
    ///
    /// # 返回
    /// - 技能执行结果字符串
    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        // 默认实现：将技能信息附加到输入中，构建技能执行上下文
        let enhanced_input = format!(
            "技能ID: {}, 参数: {}\n\n{}",
            skill_id, params, exec_ctx.ctx.input
        );
        let skill_exec_ctx = DomainExecutionContext {
            ctx: DomainContext {
                input: enhanced_input,
                ..exec_ctx.ctx
            },
            skill_id: Some(skill_id.to_string()),
            skill_params: Some(params.to_string()),
            ..exec_ctx
        };
        self.run(skill_exec_ctx).await
    }

    /// 使用 LLM 自动规划并执行多技能组合
    ///
    /// 通用默认实现：让 LLM 分析用户意图，生成技能执行计划，然后按顺序执行。
    /// 所有专家都可以使用此方法，无需重复实现。
    ///
    /// # 参数
    /// - `exec_ctx`: 领域执行上下文
    ///
    /// # 返回
    /// - 所有技能执行结果的汇总
    async fn plan_and_execute(&self, mut exec_ctx: DomainExecutionContext) -> DomainResult<String> {
        let input = exec_ctx.ctx.input.clone();
        let skills = self.skills().to_vec();
        let expert_name = self.name().to_string();

        tracing::info!(
            "[plan_and_execute] 专家={}, 输入长度={}, 可用技能数={}",
            expert_name,
            input.len(),
            skills.len()
        );

        // 1. 发送进度通知：开始规划
        let _ = send_progress(
            &exec_ctx.progress_tx,
            &format!("🔍 {} 正在分析需求，制定执行计划...", expert_name),
        );

        // 2. 让 LLM 生成执行计划
        let plan = generate_plan(&exec_ctx, &input, &skills, &expert_name).await?;

        // 记录计划详情
        tracing::info!(
            "[plan_and_execute] 生成计划: description={}, steps={:?}",
            plan.description,
            plan.steps
                .iter()
                .map(|s| format!("{}:{:?}", s.skill_id, s.description))
                .collect::<Vec<_>>()
        );

        // 3. 发送计划通知
        let _ = send_progress(
            &exec_ctx.progress_tx,
            &format!(
                "📋 执行计划：{}\n共 {} 个步骤",
                plan.description,
                plan.step_count()
            ),
        );

        // 4. 按顺序执行每个步骤
        let mut results = Vec::new();
        let total_steps = plan.steps.len();

        for (idx, step) in plan.steps.iter().enumerate() {
            let step_num = idx + 1;

            // 发送进度通知：开始执行步骤
            let _ = send_progress(
                &exec_ctx.progress_tx,
                &format!(
                    "⚡ 步骤 {}/{}: [{}] {} - 执行中...",
                    step_num, total_steps, step.skill_id, step.description
                ),
            );

            // 执行技能
            let skill_result = self
                .execute_skill(&step.skill_id, &step.params, exec_ctx.clone())
                .await;

            match &skill_result {
                Ok(output) => {
                    let _ = send_progress(
                        &exec_ctx.progress_tx,
                        &format!(
                            "✅ 步骤 {}/{} 完成: {}",
                            step_num, total_steps, step.description
                        ),
                    );
                    results.push(format!(
                        "## 步骤 {}: {}\n\n{}",
                        step_num, step.description, output
                    ));
                }
                Err(e) => {
                    let _ = send_progress(
                        &exec_ctx.progress_tx,
                        &format!(
                            "❌ 步骤 {}/{} 失败: {} - 错误: {}",
                            step_num, total_steps, step.description, e
                        ),
                    );
                    results.push(format!(
                        "## 步骤 {}: {} [失败]\n\n错误: {}",
                        step_num, step.description, e
                    ));
                }
            }

            // 更新上下文，将前一步结果注入到下一步
            exec_ctx.ctx.input = match &skill_result {
                Ok(output) => output.clone(),
                Err(e) => format!("上一步失败: {}", e),
            };
        }

        // 5. 汇总结果
        let summary = format!(
            "# {} 执行结果\n\n{}\n\n---\n共执行 {} 个步骤",
            expert_name,
            results.join("\n\n---\n\n"),
            total_steps
        );

        tracing::info!(
            "[plan_and_execute] 专家={} 执行完成，共{}个步骤",
            expert_name,
            total_steps
        );

        Ok(summary)
    }
}

/// 解析 LLM 输出的执行计划（独立函数，避免 trait 对象大小问题）
pub fn parse_plan(output: &str) -> DomainResult<SkillPlan> {
    // 尝试提取 JSON（可能被 markdown 代码块包裹）
    let json_str = if let Some(start) = output.find("```json") {
        let start = start + 7;
        if let Some(end) = output[start..].find("```") {
            output[start..start + end].trim().to_string()
        } else {
            output.trim().to_string()
        }
    } else if let Some(start) = output.find("```") {
        let start = start + 3;
        if let Some(end) = output[start..].find("```") {
            output[start..start + end].trim().to_string()
        } else {
            output.trim().to_string()
        }
    } else {
        output.trim().to_string()
    };

    serde_json::from_str(&json_str).map_err(|e| {
        DomainError::LlmError(format!("解析执行计划失败: {}, 原始输出: {}", e, output))
    })
}

/// 使用 LLM 生成执行计划（独立函数）
pub async fn generate_plan(
    exec_ctx: &DomainExecutionContext,
    input: &str,
    skills: &[DomainSkill],
    expert_name: &str,
) -> DomainResult<SkillPlan> {
    let skills_desc: String = skills
        .iter()
        .map(|s| {
            format!(
                "- **{}**: {} (参数: {})",
                s.id,
                s.description,
                s.parameters.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let system_prompt = format!(
        r#"你是 {}，擅长规划任务执行。

请分析用户需求，从可用技能中选择最合适的技能组合，制定执行计划。

可用技能：
{}

规则：
1. 根据用户需求选择 1-3 个技能组合执行
2. 技能按顺序执行，前一步的输出作为后一步的输入
3. 如果用户只是闲聊或询问信息，只需使用 chat 技能
4. 如果需要编码，先 chat 确认需求，再 coding 执行
5. 以 JSON 格式返回执行计划"#,
        expert_name, skills_desc
    );

    let user_prompt = format!(
        "用户需求：\n{}\n\n请制定执行计划，以以下 JSON 格式返回：\n```json\n{{\n  \"description\": \"计划描述\",\n  \"steps\": [\n    {{\n      \"order\": 1,\n      \"skill_id\": \"rust-chat\",\n      \"description\": \"步骤描述\",\n      \"params\": \"技能参数\"\n    }}\n  ]\n}}\n```",
        input
    );

    let messages = vec![
        DomainMessage {
            role: DomainRole::System,
            content: system_prompt,
        },
        DomainMessage {
            role: DomainRole::User,
            content: user_prompt,
        },
    ];

    let llm_output = exec_ctx.llm.chat(messages).await?;

    // 解析 JSON 计划
    parse_plan(&llm_output)
}

/// 发送进度通知（辅助函数）
fn send_progress(tx: &Option<mpsc::Sender<String>>, message: &str) {
    if let Some(sender) = tx {
        let _ = sender.try_send(message.to_string());
    }
}

/// 领域 LLM 接口（纯领域类型）
///
/// 定义领域层使用的 LLM 能力，不依赖框架实现。
#[async_trait]
pub trait DomainLlm: Send + Sync {
    /// 发送消息并获取响应
    async fn chat(&self, messages: Vec<DomainMessage>) -> DomainResult<String>;

    /// 获取模型名称
    fn model_name(&self) -> &str;
}

/// 领域消息类型
#[derive(Debug, Clone)]
pub struct DomainMessage {
    pub role: DomainRole,
    pub content: String,
}

/// 领域消息角色
#[derive(Debug, Clone, PartialEq)]
pub enum DomainRole {
    System,
    User,
    Assistant,
}

/// 领域结果类型
pub type DomainResult<T> = Result<T, DomainError>;

/// 领域数据仓库接口（纯领域接口）
///
/// 定义领域层使用的数据访问能力，不依赖具体数据库实现。
/// 支持键值存储模式，便于专家存储和读取数据。
#[async_trait]
pub trait DomainRepository: Send + Sync {
    /// 保存数据
    async fn save(&self, key: &str, value: &str) -> DomainResult<()>;

    /// 加载数据
    async fn load(&self, key: &str) -> DomainResult<Option<String>>;

    /// 删除数据
    async fn delete(&self, key: &str) -> DomainResult<()>;

    /// 批量保存数据
    async fn save_batch(&self, items: &[(&str, &str)]) -> DomainResult<()>;

    /// 批量加载数据
    async fn load_batch(&self, keys: &[&str]) -> DomainResult<Vec<(String, String)>>;
}

/// 领域错误类型
#[derive(Debug)]
pub enum DomainError {
    LlmError(String),
    ContextError(String),
    ExecutionError(String),
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainError::LlmError(e) => write!(f, "LLM 错误: {}", e),
            DomainError::ContextError(e) => write!(f, "上下文错误: {}", e),
            DomainError::ExecutionError(e) => write!(f, "执行错误: {}", e),
        }
    }
}

impl std::error::Error for DomainError {}

// ─── 规划器相关类型 ─────────────────────────────────────────────

/// 执行计划步骤
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanStep {
    /// 步骤序号（从1开始）
    pub order: u32,
    /// 使用的技能ID
    pub skill_id: String,
    /// 步骤描述
    pub description: String,
    /// 技能参数（可以是字符串或对象）
    #[serde(deserialize_with = "deserialize_params")]
    pub params: String,
}

/// 自定义反序列化：支持 params 为字符串或对象
fn deserialize_params<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

/// 执行计划
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillPlan {
    /// 计划描述
    pub description: String,
    /// 步骤列表（按顺序执行）
    pub steps: Vec<PlanStep>,
}

impl SkillPlan {
    pub fn new(description: &str) -> Self {
        Self {
            description: description.to_string(),
            steps: Vec::new(),
        }
    }

    pub fn add_step(mut self, step: PlanStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_plan_creation() {
        let plan = SkillPlan::new("测试计划")
            .add_step(PlanStep {
                order: 1,
                skill_id: "rust-chat".to_string(),
                description: "闲聊".to_string(),
                params: "你好".to_string(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "rust-coding".to_string(),
                description: "编码".to_string(),
                params: "创建项目".to_string(),
            });

        assert_eq!(plan.description, "测试计划");
        assert_eq!(plan.step_count(), 2);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
        assert_eq!(plan.steps[1].skill_id, "rust-coding");
    }

    #[test]
    fn test_parse_plan_from_json() {
        let json = r#"{
            "description": "简单计划",
            "steps": [
                {
                    "order": 1,
                    "skill_id": "rust-chat",
                    "description": "回复问候",
                    "params": "你好"
                }
            ]
        }"#;

        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.description, "简单计划");
        assert_eq!(plan.step_count(), 1);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
    }

    #[test]
    fn test_parse_plan_from_markdown_code_block() {
        let input = r#"以下是执行计划：

```json
{
  "description": "编码计划",
  "steps": [
    {
      "order": 1,
      "skill_id": "rust-chat",
      "description": "确认需求",
      "params": "用户要求创建Rust项目"
    },
    {
      "order": 2,
      "skill_id": "rust-coding",
      "description": "执行编码",
      "params": "创建项目并编写代码"
    }
  ]
}
```"#;

        let plan = parse_plan(input).unwrap();
        assert_eq!(plan.description, "编码计划");
        assert_eq!(plan.step_count(), 2);
    }

    #[test]
    fn test_parse_plan_invalid_json() {
        let input = "这不是有效的JSON";
        let result = parse_plan(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_step_serialization() {
        let step = PlanStep {
            order: 1,
            skill_id: "rust-chat".to_string(),
            description: "测试步骤".to_string(),
            params: "参数".to_string(),
        };

        let json = serde_json::to_string(&step).unwrap();
        let deserialized: PlanStep = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.order, 1);
        assert_eq!(deserialized.skill_id, "rust-chat");
        assert_eq!(deserialized.description, "测试步骤");
    }

    #[test]
    fn test_parse_plan_with_object_params() {
        // 测试 params 为对象的情况（LLM 可能生成这种格式）
        let json = r#"{
            "description": "测试对象参数",
            "steps": [
                {
                    "order": 1,
                    "skill_id": "rust-chat",
                    "description": "测试步骤",
                    "params": {
                        "question": "你好",
                        "context": "测试"
                    }
                }
            ]
        }"#;

        let plan = parse_plan(json).unwrap();
        assert_eq!(plan.description, "测试对象参数");
        assert_eq!(plan.step_count(), 1);
        assert_eq!(plan.steps[0].skill_id, "rust-chat");
        // params 应该被转换为字符串
        assert!(plan.steps[0].params.contains("question"));
    }
}
