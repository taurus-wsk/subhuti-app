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
#[derive(Debug, Clone)]
pub struct OrchestrateRequest {
    pub message: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub chain: Option<String>,
    /// 追踪 ID（TraceAppService 装饰器生成并注入，一路透传到框架 ctx.metadata）
    pub trace_id: Option<String>,
}

/// 调度响应（领域 DTO）
#[derive(Debug, Clone)]
pub struct OrchestrateResponse {
    pub success: bool,
    pub output: String,
    pub chain: Vec<String>,
    pub expert_chain: Vec<String>,
    pub expert_outputs: Vec<String>,
    pub duration_ms: u64,
    pub error: Option<String>,
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
