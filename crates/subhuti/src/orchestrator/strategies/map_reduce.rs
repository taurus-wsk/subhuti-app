//! # 并行发散-汇总策略 (MapReduce)
//!
//! 适用场景：独立子任务并行执行
//! 流程：任务拆解为 N 个独立子任务 → 同时分发给不同专家并行执行 → 调度器汇总所有结果

use super::super::{DispatchDecision, OrchestrationResult, SubTask, SubTaskStatus, TaskProfile};
use crate::orchestrator::Orchestrator;
use crate::runtime::Message;
use anyhow::Result;
use std::collections::HashMap;

/// 执行并行发散-汇总策略
///
/// 将任务拆解为独立子任务，并行分发给不同专家，最后汇总结果
pub async fn execute(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
) -> Result<OrchestrationResult> {
    // 1. 构建独立子任务
    let sub_tasks = build_map_tasks(profile, decision);

    tracing::info!(
        "MapReduce: Executing {} parallel sub-tasks for input: {}",
        sub_tasks.len(),
        profile.input
    );

    // 2. 并行执行所有子任务
    let mut handles = Vec::new();

    for sub_task in &sub_tasks {
        let expert_id = sub_task.assigned_expert.clone().unwrap_or_default();
        let task_description = sub_task.description.clone();
        let task_input = profile.input.clone();
        let runtime = orchestrator.runtime.clone();

        // 获取专家 system prompt（需要在 async 块外获取）
        let system_prompt = orchestrator
            .get_expert(&expert_id)
            .map(|a| a.persona.system_prompt.clone())
            .unwrap_or_else(|| "你是一个AI助手。".to_string());

        let handle = tokio::spawn(async move {
            let messages = vec![
                Message {
                    role: crate::runtime::Role::System,
                    content: format!("{} 你专注于处理以下子任务。", system_prompt),
                    tool_call_id: None,
                },
                Message {
                    role: crate::runtime::Role::User,
                    content: format!(
                        "子任务描述：{}\n\n原始问题：{}",
                        task_description, task_input
                    ),
                    tool_call_id: None,
                },
            ];

            match runtime.call_llm_with_stats(messages).await {
                Ok(response) => (expert_id, Ok(response.content)),
                Err(e) => (expert_id, Err(e.to_string())),
            }
        });

        handles.push(handle);
    }

    // 3. 收集所有结果
    let mut expert_outputs = HashMap::new();
    let mut expert_chain = Vec::new();
    let tokens = crate::context::TokenStats::default();

    for handle in handles {
        match handle.await {
            Ok((expert_id, Ok(output))) => {
                expert_chain.push(expert_id.clone());
                expert_outputs.insert(expert_id, output);
            }
            Ok((expert_id, Err(e))) => {
                tracing::warn!(
                    "MapReduce: Sub-task for expert '{}' failed: {}",
                    expert_id,
                    e
                );
                expert_outputs.insert(expert_id, format!("[错误] {}", e));
            }
            Err(e) => {
                tracing::error!("MapReduce: Task join error: {}", e);
            }
        }
    }

    // 4. 汇总所有专家的输出
    let summary = build_summary(orchestrator, profile, &expert_outputs).await?;

    tracing::info!(
        "MapReduce: Completed {} sub-tasks, summary length={}",
        sub_tasks.len(),
        summary.len()
    );

    Ok(OrchestrationResult {
        output: summary,
        strategy: crate::orchestrator::DispatchStrategy::MapReduce,
        expert_chain,
        expert_outputs,
        tokens,
        duration_ms: 0,
        critique_records: Vec::new(),
    })
}

/// 构建 Map 阶段的子任务
fn build_map_tasks(profile: &TaskProfile, decision: &DispatchDecision) -> Vec<SubTask> {
    if !profile.sub_tasks.is_empty() {
        return profile.sub_tasks.clone();
    }

    // 根据领域拆解子任务
    let mut sub_tasks = Vec::new();

    for (i, domain) in profile.domains.iter().enumerate() {
        let description = match domain {
            crate::orchestrator::TaskDomain::Development => "从编程开发角度分析",
            crate::orchestrator::TaskDomain::Database => "从数据存储角度分析",
            crate::orchestrator::TaskDomain::Architecture => "从架构设计角度分析",
            crate::orchestrator::TaskDomain::Psychology => "从心理学角度分析",
            crate::orchestrator::TaskDomain::Education => "从教育学习角度分析",
            crate::orchestrator::TaskDomain::Business => "从商业角度分析",
            crate::orchestrator::TaskDomain::Writing => "从写作表达角度分析",
            crate::orchestrator::TaskDomain::General => "从通用角度分析",
            _ => "从专业角度分析",
        };

        let assigned_expert = if i < decision.matched_experts.len() {
            Some(decision.matched_experts[i].expert_id.clone())
        } else {
            None
        };

        sub_tasks.push(SubTask {
            id: format!("map-task-{}", i + 1),
            description: format!("{}：{}", description, profile.input),
            domain: domain.clone(),
            dependencies: vec![],
            assigned_expert,
            result: None,
            status: SubTaskStatus::Pending,
        });
    }

    sub_tasks
}

/// 汇总所有专家的输出（Reduce 阶段）
async fn build_summary(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    expert_outputs: &HashMap<String, String>,
) -> Result<String> {
    if expert_outputs.is_empty() {
        return Ok("无法获取任何专家的分析结果。".to_string());
    }

    // 构建汇总 prompt
    let mut outputs_text = String::new();
    for (expert_id, output) in expert_outputs {
        outputs_text.push_str(&format!(
            "\n### 专家 [{}] 的分析：\n{}\n",
            expert_id, output
        ));
    }

    let messages = vec![
        Message {
            role: crate::runtime::Role::System,
            content:
                "你是一个综合分析师。请整合以下不同专家的分析结果，给出一个全面、连贯的最终答案。\
                     请指出各专家观点的一致性和分歧点，给出综合建议。"
                    .to_string(),
            tool_call_id: None,
        },
        Message {
            role: crate::runtime::Role::User,
            content: format!(
                "原始问题：{}\n\n各专家分析结果：{}\n\n请整合以上分析，给出综合答案。",
                profile.input, outputs_text
            ),
            tool_call_id: None,
        },
    ];

    let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
    Ok(response.content)
}
