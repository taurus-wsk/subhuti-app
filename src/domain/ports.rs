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

    /// 获取当前激活的专家（返回 None = 未激活）
    fn active_expert(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExpertInfo>> + Send>>;
}

/// 编排引擎端口（出站端口）
///
/// 应用层通过此接口执行调度、分析任务和匹配专家，具体实现由出站适配层提供。
pub trait OrchestrationEnginePort: Send + Sync + 'static {
    /// 执行调度
    ///
    /// - `trace_id`: 追踪 ID（写入框架 ctx.metadata，事件 emit 时带 trace 上下文）
    /// - `session_id`: 会话 ID（同上，写入 ctx.metadata 供事件关联）
    fn orchestrate(
        &self,
        message: &str,
        user_id: &str,
        chain: &str,
        trace_id: &str,
        session_id: &str,
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
