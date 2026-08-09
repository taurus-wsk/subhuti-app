//! # 出站适配器层
//!
//! 六边形架构的适配器层：将外部框架（Subhuti）的实现转换为应用层定义的接口。
//!
//! 设计原则：
//! - 适配器实现应用层的出站端口（Outbound Port）
//! - 适配器内部持有外部框架实例
//! - 应用层仅依赖端口接口，不依赖具体实现

use crate::domain::dto::{ExpertInfo, SkillInfo};
use subhuti::FrameworkExpertInfo;

/// 将框架层 `FrameworkExpertInfo` → 应用层 `ExpertInfo` 的共享转换
///
/// 框架快照本身不冗余存储 `skills[].expert_id / expert_name`（因为已隐含所属专家），
/// 在此统一填充这两个字段，供 3 个适配器（ExpertRepository / OrchestrationEngine / SkillExecutor）共用。
pub(crate) fn framework_to_app_expert(framework: FrameworkExpertInfo) -> ExpertInfo {
    let expert_id = framework.id.clone();
    let expert_name = framework.name.clone();
    ExpertInfo {
        id: expert_id.clone(),
        name: expert_name.clone(),
        tags: framework.tags,
        skills: framework
            .skills
            .into_iter()
            .map(|s| SkillInfo {
                id: s.id,
                name: s.name,
                description: s.description,
                parameters: s.parameters,
                expert_id: expert_id.clone(),
                expert_name: expert_name.clone(),
            })
            .collect(),
    }
}

pub mod domain_expert_adapter;
pub mod subhuti_expert_repository;
pub mod subhuti_orchestration_engine;
pub mod subhuti_skill_executor;
// 已删除：subhuti_plugin_repository（框架无真实实现，待插件生命周期管理实现后加回）
pub mod event_bridge;
pub mod graphs;
pub mod observer_adapters;
pub mod postgres_repository;
pub mod rules;
pub mod subhuti_framework_initializer;
