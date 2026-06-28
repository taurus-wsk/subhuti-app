//! # 评审迭代策略 (CritiqueRevise)
//!
//! 适用场景：需要质量把控的任务
//! 流程：生成专家输出初稿 → 评审专家给出修改意见 → 生成专家迭代优化 → 循环直到达标
//!
//! 可作为叠加模式：在其他策略执行完后，用评审迭代优化结果

use super::super::{CritiqueRecord, DispatchDecision, OrchestrationResult, TaskProfile};
use crate::orchestrator::Orchestrator;
use crate::runtime::Message;
use anyhow::Result;
use std::collections::HashMap;

/// 执行评审迭代策略（独立模式）
///
/// 对给定的内容进行多轮评审和迭代优化
pub async fn execute(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
) -> Result<OrchestrationResult> {
    // 先让生成专家输出初稿
    let initial_result = super::simple_dispatch::execute(orchestrator, profile, decision).await?;

    // 在初稿基础上进行评审迭代
    execute_on_result(orchestrator, profile, decision, &initial_result.output).await
}

/// 在已有结果上叠加评审迭代（叠加模式）
///
/// 用于在其他策略执行完后，对结果进行质量评审和迭代优化
pub async fn execute_on_result(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
    initial_output: &str,
) -> Result<OrchestrationResult> {
    let mut current_output = initial_output.to_string();
    let mut critique_records = Vec::new();
    let max_rounds = orchestrator.config().max_critique_rounds;
    let mut expert_chain = vec!["critique-system".to_string()];
    let mut expert_outputs = HashMap::new();
    let tokens = crate::context::TokenStats::default();

    // 找到评审专家（使用匹配结果中的第二个专家，如果没有就用通用专家）
    let reviewer_id = if decision.matched_experts.len() > 1 {
        decision.matched_experts[1].expert_id.clone()
    } else if let Some(first) = decision.matched_experts.first() {
        first.expert_id.clone()
    } else {
        "general-reviewer".to_string()
    };

    expert_chain.push(reviewer_id.clone());

    tracing::info!(
        "CritiqueRevise: Starting review process, max rounds={}, reviewer='{}'",
        max_rounds,
        reviewer_id
    );

    for round in 1..=max_rounds {
        tracing::info!("CritiqueRevise: Round {}/{}", round, max_rounds);

        // 1. 评审当前输出
        let critique =
            review_output(orchestrator, &reviewer_id, profile, &current_output, round).await?;

        // 检查是否达到满意标准
        if is_satisfactory(&critique) {
            tracing::info!("CritiqueRevise: Satisfactory after round {}", round);

            critique_records.push(CritiqueRecord {
                reviewer: reviewer_id.clone(),
                content: current_output.clone(),
                feedback: format!("[第{}轮] 评审通过", round),
                round,
            });

            expert_outputs.insert(
                reviewer_id.clone(),
                format!("[第{}轮] 评审通过：{}", round, critique),
            );
            break;
        }

        // 2. 根据评审意见修改
        let revised = revise_output(
            orchestrator,
            decision,
            profile,
            &current_output,
            &critique,
            round,
        )
        .await?;

        // 3. 记录评审记录
        critique_records.push(CritiqueRecord {
            reviewer: reviewer_id.clone(),
            content: current_output.clone(),
            feedback: critique.clone(),
            round,
        });

        current_output = revised;

        if round == max_rounds {
            tracing::warn!(
                "CritiqueRevise: Reached max rounds ({}) without satisfaction",
                max_rounds
            );
        }
    }

    expert_outputs.insert(reviewer_id, current_output.clone());

    tracing::info!(
        "CritiqueRevise: Completed with {} critique rounds",
        critique_records.len()
    );

    Ok(OrchestrationResult {
        output: current_output,
        strategy: crate::orchestrator::DispatchStrategy::CritiqueRevise,
        expert_chain,
        expert_outputs,
        tokens,
        duration_ms: 0,
        critique_records,
    })
}

/// 评审当前输出
async fn review_output(
    orchestrator: &Orchestrator,
    reviewer_id: &str,
    profile: &TaskProfile,
    current_output: &str,
    round: u32,
) -> Result<String> {
    let system_prompt = orchestrator
        .get_expert(reviewer_id)
        .map(|a| a.persona.system_prompt.clone())
        .unwrap_or_else(|| "你是一个严格的评审专家。".to_string());

    let messages = vec![
        Message {
            role: crate::runtime::Role::System,
            content: format!(
                "{} 你现在是评审专家。请严格审查以下回答，从准确性、完整性、清晰度、\
                 实用性等维度进行评估。\n\n\
                 如果回答已经很好，请回复 \"SATISFACTORY\"。\n\
                 如果需要改进，请具体指出问题并给出修改建议。\n\
                 只需输出评审意见，不要输出修改后的内容。",
                system_prompt
            ),
            tool_call_id: None,
        },
        Message {
            role: crate::runtime::Role::User,
            content: format!(
                "原始问题：{}\n\n当前回答（第{}轮）：\n{}\n\n请评审以上回答。",
                profile.input, round, current_output
            ),
            tool_call_id: None,
        },
    ];

    let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
    Ok(response.content)
}

/// 根据评审意见修改输出
async fn revise_output(
    orchestrator: &Orchestrator,
    decision: &DispatchDecision,
    profile: &TaskProfile,
    current_output: &str,
    critique: &str,
    _round: u32,
) -> Result<String> {
    // 使用原始生成专家的 persona
    let system_prompt = if let Some(first) = decision.matched_experts.first() {
        orchestrator
            .get_expert(&first.expert_id)
            .map(|a| a.persona.system_prompt.clone())
            .unwrap_or_else(|| "你是一个AI助手。".to_string())
    } else {
        "你是一个AI助手。".to_string()
    };

    let messages = vec![
        Message {
            role: crate::runtime::Role::System,
            content: format!(
                "{} 请根据评审意见修改你的回答。你需要解决评审中指出的所有问题，\
                 确保修改后的回答更加准确、完整、清晰。\n\n\
                 只需输出修改后的完整回答，不要提及评审过程。",
                system_prompt
            ),
            tool_call_id: None,
        },
        Message {
            role: crate::runtime::Role::User,
            content: format!(
                "原始问题：{}\n\n你的原始回答：\n{}\n\n评审意见：\n{}\n\n请根据评审意见修改你的回答。",
                profile.input, current_output, critique
            ),
            tool_call_id: None,
        },
    ];

    let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
    Ok(response.content)
}

/// 判断评审是否满意
fn is_satisfactory(critique: &str) -> bool {
    let lower = critique.to_lowercase();
    lower.contains("satisfactory")
        || lower.contains("满意")
        || lower.contains("很好")
        || lower.contains("无需修改")
        || lower.contains("通过")
        || lower.contains("pass")
        || lower.contains("approved")
}

/// 测试辅助函数：判断评审是否满意（公开用于测试）
#[doc(hidden)]
pub fn is_satisfactory_test(critique: &str) -> bool {
    is_satisfactory(critique)
}
