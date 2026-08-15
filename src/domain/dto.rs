//! # 领域 DTO（数据传输对象）
//!
//! 这些 DTO 表达领域概念（专家、技能、调度结果），不属于应用层。
//! 领域层允许 derive(Serialize) 用于 API 响应序列化。
//!
//! 与 domain::traits::DomainSkill 的区别：
//! - `DomainSkill` 是专家"声明拥有"的技能（无 expert_id，隐含在 DomainExpert 实例中）
//! - `SkillInfo` 是"展开后供外部消费"的快照（显式带 expert_id/expert_name）

use serde::Serialize;

/// 调度请求（领域 DTO）
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrateRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub chain: Option<String>,
    /// 指定要使用的图名称（为空时自动匹配）
    pub graph: Option<String>,
    /// 追踪 ID（TraceAppService 装饰器生成并注入，一路透传到框架 ctx.metadata）
    pub trace_id: Option<String>,
}

/// 调度响应（领域 DTO）
#[derive(Debug, Clone, Serialize)]
pub struct OrchestrateResponse {
    pub success: bool,
    pub output: String,
    pub chain: Vec<String>,
    pub expert_chain: Vec<String>,
    pub expert_outputs: Vec<String>,
    pub duration_ms: u64,
    pub error: Option<String>,
    /// 追踪 ID（出站适配器 orchestrate 执行完后注入，便于调用方直接 /traces/:id/tree 查询）
    pub trace_id: String,
    /// 会话 ID（同上，与 trace_id 配对）
    pub session_id: String,
}

/// 专家信息（领域 DTO）
#[derive(Debug, Clone, Serialize)]
pub struct ExpertInfo {
    pub id: String,
    pub name: String,
    pub tags: Vec<String>,
    pub skills: Vec<SkillInfo>,
}

/// 技能信息（领域 DTO）
#[derive(Debug, Clone, Serialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
    pub expert_id: String,
    pub expert_name: String,
}

/// 技能执行响应（领域 DTO）
#[derive(Debug, Clone)]
pub struct SkillResponse {
    pub success: bool,
    pub output: String,
    pub skill_id: String,
    pub expert_id: String,
    pub error: Option<String>,
}

// ─── Rust 编程专家 DTO ───────────────────────────────────────────

/// 工具链检查结果
#[derive(Debug, Clone, Serialize)]
pub struct ToolchainResult {
    pub success: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub output: String,
}

/// Rust 代码生成请求
#[derive(Debug, Clone)]
pub struct RustCodeRequest {
    /// 需求描述
    pub requirement: String,
    /// 目标文件路径（可选，用于增量修改）
    pub target_file: Option<String>,
    /// 是否启用验证修复循环
    pub verify: bool,
    /// 最大修复轮次（默认 3）
    pub max_retries: usize,
}

/// Rust 代码生成结果
#[derive(Debug, Clone, Serialize)]
pub struct RustCodeResult {
    pub success: bool,
    /// 生成的代码
    pub code: String,
    /// 文件路径
    pub file_path: Option<String>,
    /// 编译检查结果
    pub toolchain_result: Option<ToolchainResult>,
    /// 修复轮次
    pub retries: usize,
    /// 设计说明
    pub explanation: String,
    /// 适用的设计范式
    pub patterns_applied: Vec<String>,
    /// 错误信息
    pub error: Option<String>,
}
