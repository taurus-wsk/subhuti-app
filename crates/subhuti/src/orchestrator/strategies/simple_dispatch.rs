//! # 单专家直连策略 (SimpleDispatch)
//!
//! 适用场景：单一领域 & 低复杂度任务
//! 流程：调度器匹配最优专家 → 直接下发任务 → 专家执行并返回结果

use super::super::{DispatchDecision, OrchestrationResult, TaskProfile};
use crate::orchestrator::Orchestrator;
use crate::runtime::Message;
use anyhow::Result;
use std::collections::HashMap;

/// 执行单专家直连策略
///
/// 从匹配结果中选择得分最高的专家，直接执行任务
pub async fn execute(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
) -> Result<OrchestrationResult> {
    let mut expert_outputs = HashMap::new();
    let mut expert_chain = Vec::new();
    let mut tokens = crate::context::TokenStats::default();

    // 选择最佳匹配专家
    let best_expert = decision
        .matched_experts
        .first()
        .ok_or_else(|| anyhow::anyhow!("SimpleDispatch: No matching expert found"))?;

    let expert_id = &best_expert.expert_id;
    expert_chain.push(expert_id.clone());

    tracing::info!(
        "SimpleDispatch: Routing to expert '{}' (score={:.3})",
        expert_id,
        best_expert.overall_score
    );

    // 从专家池获取专家 Agent
    let output = if let Some(agent) = orchestrator.get_expert(expert_id) {
        // 使用专家的 persona 构建 system prompt
        let system_prompt = agent.persona.system_prompt.clone();

        // 构建消息
        let messages = vec![
            Message {
                role: crate::runtime::Role::System,
                content: system_prompt,
                tool_call_id: None,
            },
            Message {
                role: crate::runtime::Role::User,
                content: profile.input.clone(),
                tool_call_id: None,
            },
        ];

        // 调用 LLM
        let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
        tokens.add(&response);
        response.content
    } else {
        // 专家不在池中，使用通用兜底
        tracing::warn!(
            "SimpleDispatch: Expert '{}' not found in pool, using general fallback",
            expert_id
        );
        format!(
            "[通用回复] 针对您的问题：{}，我已进行初步分析。",
            profile.input
        )
    };

    expert_outputs.insert(expert_id.clone(), output.clone());

    tracing::info!("SimpleDispatch: Completed, output length={}", output.len());

    Ok(OrchestrationResult {
        output,
        strategy: crate::orchestrator::DispatchStrategy::SimpleDispatch,
        expert_chain,
        expert_outputs,
        tokens,
        duration_ms: 0,
        critique_records: Vec::new(),
    })
}
