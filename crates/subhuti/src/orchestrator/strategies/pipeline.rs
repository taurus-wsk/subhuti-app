//! # 串行流水线策略 (Pipeline)
//!
//! 适用场景：有明确前后依赖的多阶段任务
//! 流程：任务拆解为阶段 → 每个阶段匹配对应专家 → 前序输出作为后序输入

use super::super::{DispatchDecision, OrchestrationResult, SubTask, SubTaskStatus, TaskProfile};
use crate::orchestrator::Orchestrator;
use crate::runtime::Message;
use anyhow::Result;
use std::collections::HashMap;

/// 执行串行流水线策略
///
/// 按顺序执行每个阶段的专家，前一个阶段的输出作为下一个阶段的上下文输入
pub async fn execute(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
) -> Result<OrchestrationResult> {
    let mut expert_outputs = HashMap::new();
    let mut expert_chain = Vec::new();
    let mut tokens = crate::context::TokenStats::default();

    // 1. 将任务拆解为流水线阶段
    let pipeline_stages = build_pipeline_stages(profile, decision);

    tracing::info!(
        "Pipeline: Executing {} stages for input: {}",
        pipeline_stages.len(),
        profile.input
    );

    // 2. 串行执行每个阶段
    let mut previous_output = String::new();

    for (idx, stage) in pipeline_stages.iter().enumerate() {
        let expert_id = match &stage.assigned_expert {
            Some(id) => id.clone(),
            None => {
                // 自动匹配该阶段的专家
                match orchestrator.best_expert(profile) {
                    Some(m) => m.expert_id,
                    None => {
                        tracing::warn!("Pipeline: No expert for stage {}, skipping", idx);
                        continue;
                    }
                }
            }
        };

        expert_chain.push(expert_id.clone());

        tracing::info!(
            "Pipeline: Stage {}/{} - '{}' -> expert '{}'",
            idx + 1,
            pipeline_stages.len(),
            stage.description,
            expert_id
        );

        // 构建该阶段的输入（包含前一阶段输出）
        let stage_input = if previous_output.is_empty() {
            format!(
                "任务阶段：{}\n\n原始问题：{}",
                stage.description, profile.input
            )
        } else {
            format!(
                "任务阶段：{}\n\n原始问题：{}\n\n前一阶段结果：{}",
                stage.description, profile.input, previous_output
            )
        };

        // 获取专家并执行
        let output = if let Some(agent) = orchestrator.get_expert(&expert_id) {
            let system_prompt = agent.persona.system_prompt.clone();

            let messages = vec![
                Message {
                    role: crate::runtime::Role::System,
                    content: format!(
                        "{} 你现在正在执行一个流水线任务的第 {} 阶段。请基于前面的结果继续处理。",
                        system_prompt,
                        idx + 1
                    ),
                    tool_call_id: None,
                },
                Message {
                    role: crate::runtime::Role::User,
                    content: stage_input,
                    tool_call_id: None,
                },
            ];

            let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
            tokens.add(&response);
            response.content
        } else {
            format!("[阶段 {}] 专家 {} 不可用，跳过此阶段", idx + 1, expert_id)
        };

        expert_outputs.insert(expert_id, output.clone());
        previous_output = output;
    }

    // 最终输出是最后一个阶段的结果
    let final_output = previous_output;

    tracing::info!(
        "Pipeline: Completed {} stages, final output length={}",
        pipeline_stages.len(),
        final_output.len()
    );

    Ok(OrchestrationResult {
        output: final_output,
        strategy: crate::orchestrator::DispatchStrategy::Pipeline,
        expert_chain,
        expert_outputs,
        tokens,
        duration_ms: 0,
        critique_records: Vec::new(),
    })
}

/// 根据任务画像构建流水线阶段
fn build_pipeline_stages(profile: &TaskProfile, decision: &DispatchDecision) -> Vec<SubTask> {
    // 如果已有子任务，直接使用
    if !profile.sub_tasks.is_empty() {
        let mut stages = profile.sub_tasks.clone();

        // 根据匹配的专家分配子任务
        for (i, stage) in stages.iter_mut().enumerate() {
            if stage.assigned_expert.is_none() && i < decision.matched_experts.len() {
                stage.assigned_expert = Some(decision.matched_experts[i].expert_id.clone());
            }
        }

        return stages;
    }

    // 否则根据领域自动构建阶段
    let mut stages = Vec::new();

    for (i, domain) in profile.domains.iter().enumerate() {
        let description = match domain {
            crate::orchestrator::TaskDomain::Development => "代码开发阶段",
            crate::orchestrator::TaskDomain::Database => "数据库设计阶段",
            crate::orchestrator::TaskDomain::Architecture => "架构设计阶段",
            crate::orchestrator::TaskDomain::Psychology => "心理分析阶段",
            _ => "任务处理阶段",
        };

        let assigned_expert = if i < decision.matched_experts.len() {
            Some(decision.matched_experts[i].expert_id.clone())
        } else {
            None
        };

        stages.push(SubTask {
            id: format!("stage-{}", i + 1),
            description: format!("{}：{}", description, profile.input),
            domain: domain.clone(),
            dependencies: if i > 0 {
                vec![format!("stage-{}", i)]
            } else {
                vec![]
            },
            assigned_expert,
            result: None,
            status: SubTaskStatus::Pending,
        });
    }

    stages
}
