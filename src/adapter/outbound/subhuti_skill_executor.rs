//! # Subhuti 技能执行适配器
//!
//! 实现应用层定义的 SkillExecutionPort 出站端口，
//! 将 Subhuti 框架的技能执行能力转换为应用层接口。

use std::sync::Arc;

use subhuti_core::engine::Subhuti;

use crate::adapter::outbound::framework_to_app_expert;
use crate::domain::dto::{SkillInfo, SkillResponse};
use crate::domain::ports::SkillExecutionPort;

/// Subhuti 框架的技能执行适配器
pub struct SubhutiSkillExecutor {
    subhuti: Arc<Subhuti>,
}

impl SubhutiSkillExecutor {
    /// 创建新的适配器实例
    pub fn new(subhuti: Arc<Subhuti>) -> Self {
        Self { subhuti }
    }
}

impl SkillExecutionPort for SubhutiSkillExecutor {
    fn skill_list(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<SkillInfo>> + Send>> {
        let subhuti = self.subhuti.clone();

        Box::pin(async move {
            // 1. 拿到强类型专家快照（不再是 JSON Value）
            let experts = subhuti.list_orchestrator_experts().await;

            // 2. 用共享转换函数转成应用层 ExpertInfo（自动填充 skills[].expert_id / expert_name）
            // 3. flat_map 打平所有技能
            experts
                .into_iter()
                .flat_map(|f| framework_to_app_expert(f).skills)
                .collect()
        })
    }

    fn execute_skill(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillResponse> + Send>> {
        let subhuti = self.subhuti.clone();
        let skill_id = skill_id.to_string();
        let args = args.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();

        Box::pin(async move {
            // 使用新增的门面方法 find_agent_by_skill：一次查找，直接拿到 (expert_id, Arc<Agent>)
            match subhuti.find_agent_by_skill(&skill_id).await {
                Some((expert_id, _agent)) => {
                    // 创建上下文并设置技能信息到 metadata
                    let mut ctx = subhuti_core::orchestrator::AgentContext::new(
                        &format!("执行技能 {}，参数：{}", skill_id, args),
                        "skill_executor",
                    );
                    ctx.set_metadata("skill_id", &skill_id);
                    ctx.set_metadata("skill_params", &args);
                    if !trace_id.is_empty() {
                        ctx.set_metadata("trace_id", &trace_id);
                    }
                    if !session_id.is_empty() {
                        ctx.set_metadata("session_id", &session_id);
                    }

                    // 使用自定义上下文执行编排（会经过 Orchestrator 找到对应专家并调用 run）
                    let result = subhuti.dispatch_with_context(ctx).await;

                    SkillResponse {
                        success: result.success,
                        output: result.output,
                        skill_id: skill_id.clone(),
                        expert_id,
                        error: None,
                    }
                }
                None => SkillResponse {
                    success: false,
                    output: String::new(),
                    error: Some(format!("技能 {} 未找到", skill_id)),
                    skill_id,
                    expert_id: String::new(),
                },
            }
        })
    }
}
