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
    /// 引擎侧 LLM（用于规划等框架能力，由适配器注入）
    pub engine_llm: Option<Arc<dyn subhuti_core::LLM>>,
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
            engine_llm: self.engine_llm.clone(),
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
    /// 通用默认实现：让引擎 planner 生成技能执行计划，再由引擎按顺序驱动执行。
    /// 领域层只负责把"单步技能执行"供给引擎（通过 run_step 闭包），
    /// 规划的生成与循环驱动机制全部收敛在引擎。
    ///
    /// # 参数
    /// - `exec_ctx`: 领域执行上下文
    ///
    /// # 返回
    /// - 所有技能执行结果的汇总
    async fn plan_and_execute(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String> {
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
        send_progress(
            &exec_ctx.progress_tx,
            &format!("🔍 {} 正在分析需求，制定执行计划...", expert_name),
        );

        // 2. 让引擎 planner 用 LLM 生成执行计划
        let skill_infos: Vec<subhuti_core::orchestrator::SkillInfo> = skills
            .iter()
            .map(|s| subhuti_core::orchestrator::SkillInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.parameters.clone(),
            })
            .collect();

        let engine_llm = exec_ctx
            .engine_llm
            .clone()
            .ok_or_else(|| DomainError::LlmError("缺少引擎 LLM，无法进行计划规划".to_string()))?;
        let plan = subhuti_core::orchestrator::planner::generate_plan(
            &engine_llm,
            &input,
            &skill_infos,
            &expert_name,
        )
        .await
        .map_err(|e| DomainError::LlmError(e.to_string()))?;

        tracing::info!(
            "[plan_and_execute] 生成计划: description={}, steps={:?}",
            plan.description,
            plan.steps
                .iter()
                .map(|s| format!("{}:{:?}", s.skill_id, s.description))
                .collect::<Vec<_>>()
        );

        // 3. 让引擎 execute_plan 驱动循环执行（含进度回传 + 结果注入 + 汇总）
        let progress_tx = exec_ctx.progress_tx.clone();
        let summary = subhuti_core::orchestrator::planner::execute_plan(
            &expert_name,
            &plan,
            move |msg: &str| send_progress(&progress_tx, msg),
            |skill_id, params, prev_input: &str| {
                let skill_id_owned = skill_id.to_string();
                let params_owned = params.to_string();
                let skill_exec_ctx = DomainExecutionContext {
                    ctx: DomainContext {
                        input: prev_input.to_string(),
                        system_prompt: exec_ctx.ctx.system_prompt.clone(),
                        ..exec_ctx.ctx.clone()
                    },
                    skill_id: Some(skill_id_owned.clone()),
                    skill_params: Some(params_owned.clone()),
                    ..exec_ctx.clone()
                };
                let expert = self;
                async move {
                    expert
                        .execute_skill(&skill_id_owned, &params_owned, skill_exec_ctx)
                        .await
                        .map_err(|e| subhuti_core::Error::Expert(e.to_string()))
                }
            },
        )
        .await
        .map_err(|e| DomainError::LlmError(e.to_string()))?;

        tracing::info!("[plan_and_execute] 专家={} 执行完成", expert_name);

        Ok(summary)
    }
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
