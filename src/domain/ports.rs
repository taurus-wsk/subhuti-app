//! # 领域出站端口（Driven Port）
//!
//! 由出站适配层实现，应用层通过这些接口调用外部资源。
//! 移到领域层后，端口签名引用的类型全部位于领域层，依赖闭环。
//!
//! 六边形架构：领域层定义接口，出站适配层实现接口，依赖倒置。

use std::sync::Arc;

use crate::domain::dto::{ExpertInfo, OrchestrateResponse, SkillInfo, SkillResponse};
use crate::domain::traits::{DomainExpert, DomainRepository};

/// 专家仓库端口（出站端口）
///
/// 应用层通过此接口管理专家，具体实现由出站适配层提供（如 Subhuti 框架、数据库）。
///
/// ⚠️ 不参与 HTTP 注入——HTTP 层通过 ExpertQueryPort.list_experts() 获取专家列表。
pub trait ExpertRepositoryPort: Send + Sync + 'static {
    /// 获取所有专家信息（返回领域 DTO，不泄漏框架类型）
    fn get_all(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>>;

    /// 根据 ID 获取专家信息
    fn get_by_id(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExpertInfo>> + Send>>;

    /// 注册专家（接收领域层接口，不依赖框架类型）
    ///
    /// # 参数
    /// - `expert`: 领域专家实例
    /// - `repository`: 数据仓库（供专家运行时使用）
    fn register(
        &self,
        expert: Arc<dyn DomainExpert>,
        repository: Arc<dyn DomainRepository>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
}

/// 编排引擎端口（出站端口）
///
/// 应用层通过此接口执行调度、分析任务和匹配专家，具体实现由出站适配层提供。
pub trait OrchestrationEnginePort: Send + Sync + 'static {
    /// 执行调度
    ///
    /// - `trace_id`: 追踪 ID（写入框架 ctx.metadata，事件 emit 时带 trace 上下文）
    /// - `session_id`: 会话 ID（同上，写入 ctx.metadata 供事件关联）
    /// - `graph`: 指定图名称（为空时自动匹配）
    /// - `expert_id`: 指定专家 ID（优先级最高，直接路由到该专家）
    /// - `workspace_folder`: 项目工作目录路径（透传给专家）
    /// - `system_prompt`: 自定义系统提示词（覆盖专家默认 system prompt）
    #[allow(clippy::too_many_arguments)]
    fn orchestrate(
        &self,
        message: &str,
        user_id: &str,
        chain: &str,
        graph: &str,
        expert_id: &str,
        trace_id: &str,
        session_id: &str,
        workspace_folder: &str,
        system_prompt: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>>;

    /// 分析任务
    fn analyze_task(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send>>;

    /// 匹配专家
    fn match_expert(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>>;
}

/// 工具链端口（出站端口）
///
/// Rust 工具链调用能力：编译检查、Clippy、格式化。
/// 由出站适配层实现，供领域专家在代码生成验证闭环中使用。
pub trait ToolchainPort: Send + Sync + 'static {
    /// 运行 cargo check（快速编译检查，不生成二进制）
    fn check(
        &self,
        project_path: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::domain::dto::ToolchainResult> + Send>,
    >;

    /// 运行 cargo clippy（代码质量检查）
    fn clippy(
        &self,
        project_path: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::domain::dto::ToolchainResult> + Send>,
    >;

    /// 格式化代码（rustfmt）
    fn format(
        &self,
        code: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = String> + Send>>;
}

/// 技能执行端口（出站端口）
///
/// 应用层通过此接口执行技能，具体实现由出站适配层提供。
pub trait SkillExecutionPort: Send + Sync + 'static {
    /// 获取所有技能（从专家聚合）
    fn skill_list(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<SkillInfo>> + Send>>;

    /// 执行技能（通过专家）
    ///
    /// - `trace_id`: 追踪 ID（写入框架 ctx.metadata，事件 emit 时带 trace 上下文）
    /// - `session_id`: 会话 ID（同上）
    fn execute_skill(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillResponse> + Send>>;
}

/// 文件系统操作端口（出站端口）
///
/// 供领域专家在项目工作目录中读写文件、搜索文件。
/// 由出站适配层实现，使用本地文件系统或远程存储。
pub trait FileSystemPort: Send + Sync + 'static {
    /// 读取文件内容
    fn read_file(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;

    /// 写入文件内容（自动创建父目录）
    fn write_file(
        &self,
        path: &str,
        content: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;

    /// 列出目录内容（仅文件名，不递归）
    fn list_dir(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send>>;

    /// 搜索文件（glob 模式匹配，如 "**/*.rs"）
    fn search_files(
        &self,
        pattern: &str,
        root: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>, String>> + Send>>;

    /// 检查路径是否存在
    fn exists(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>;

    /// 创建目录（递归）
    fn create_dir(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;

    /// 删除文件
    fn delete_file(
        &self,
        path: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>;
}

/// 命令执行结果
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// 命令行执行端口（出站端口）
///
/// 供领域专家在工作目录中执行 shell 命令（如 cargo build、git 等）。
/// 由出站适配层实现，使用 tokio::process::Command。
pub trait CommandPort: Send + Sync + 'static {
    /// 执行命令并返回输出
    ///
    /// - `command`: 命令名称（如 "cargo", "git"）
    /// - `args`: 命令参数列表
    /// - `cwd`: 工作目录
    fn run_command(
        &self,
        command: &str,
        args: &[String],
        cwd: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<CommandOutput, String>> + Send>>;
}
