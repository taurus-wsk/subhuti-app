//! # 主管-工人策略 (ManagerWorker)
//!
//! 适用场景：超复杂任务，需动态拆解
//! 流程：调度器任命一名主管专家 → 主管自主拆解子任务 → 分发给工人专家执行 → 主管汇总结果

use super::super::{DispatchDecision, OrchestrationResult, SubTask, SubTaskStatus, TaskProfile};
use crate::orchestrator::Orchestrator;
use crate::runtime::Message;
use anyhow::Result;
use std::collections::HashMap;

/// 执行主管-工人策略
///
/// 1. 选择一个主管专家
/// 2. 主管分析任务并拆解为子任务
/// 3. 工人专家并行执行子任务
/// 4. 主管汇总结果
pub async fn execute(
    orchestrator: &Orchestrator,
    profile: &TaskProfile,
    decision: &DispatchDecision,
) -> Result<OrchestrationResult> {
    let mut expert_outputs = HashMap::new();
    let mut expert_chain = Vec::new();
    let tokens = crate::context::TokenStats::default();

    // 1. 选择主管专家（综合得分最高的专家）
    let manager = decision
        .matched_experts
        .first()
        .ok_or_else(|| anyhow::anyhow!("ManagerWorker: No manager expert available"))?;

    let manager_id = &manager.expert_id;
    expert_chain.push(manager_id.clone());

    tracing::info!(
        "ManagerWorker: Appointing '{}' as manager (score={:.3})",
        manager_id,
        manager.overall_score
    );

    // 2. 主管分析任务，拆解为子任务
    let sub_tasks = decompose_by_manager(orchestrator, manager_id, profile).await?;

    tracing::info!(
        "ManagerWorker: Manager decomposed task into {} sub-tasks",
        sub_tasks.len()
    );

    // 3. 分配工人并并行执行子任务
    let workers: Vec<String> = decision
        .matched_experts
        .iter()
        .map(|m| m.expert_id.clone())
        .collect();

    let mut handles = Vec::new();

    for (i, sub_task) in sub_tasks.iter().enumerate() {
        let worker_id = if i < workers.len() {
            workers[i].clone()
        } else {
            // 工人不够，使用通用专家
            orchestrator.get_general_expert_id()
        };

        expert_chain.push(worker_id.clone());

        let task_desc = sub_task.description.clone();
        let original_input = profile.input.clone();
        let runtime = orchestrator.runtime.clone();

        let system_prompt = orchestrator
            .get_expert(&worker_id)
            .map(|a| a.persona.system_prompt.clone())
            .unwrap_or_else(|| "你是一个AI助手。".to_string());

        let handle = tokio::spawn(async move {
            let messages = vec![
                Message {
                    role: crate::runtime::Role::System,
                    content: format!(
                        "{} 你是主管专家分配的工人，请完成以下子任务。",
                        system_prompt
                    ),
                    tool_call_id: None,
                },
                Message {
                    role: crate::runtime::Role::User,
                    content: format!(
                        "原始任务：{}\n\n你的子任务：{}\n\n请专注完成你的子任务，不需要考虑其他部分。",
                        original_input, task_desc
                    ),
                    tool_call_id: None,
                },
            ];

            match runtime.call_llm_with_stats(messages).await {
                Ok(response) => (worker_id, Ok(response.content)),
                Err(e) => (worker_id, Err(e.to_string())),
            }
        });

        handles.push(handle);
    }

    // 4. 收集工人结果
    let mut worker_outputs = HashMap::new();

    for handle in handles {
        match handle.await {
            Ok((worker_id, Ok(output))) => {
                expert_outputs.insert(worker_id.clone(), output.clone());
                worker_outputs.insert(worker_id, output);
            }
            Ok((worker_id, Err(e))) => {
                tracing::warn!("ManagerWorker: Worker '{}' failed: {}", worker_id, e);
                expert_outputs.insert(worker_id, format!("[错误] {}", e));
            }
            Err(e) => {
                tracing::error!("ManagerWorker: Task join error: {}", e);
            }
        }
    }

    // 5. 主管汇总所有工人结果
    let final_output =
        summarize_by_manager(orchestrator, manager_id, profile, &worker_outputs).await?;

    expert_outputs.insert(manager_id.clone(), final_output.clone());

    tracing::info!(
        "ManagerWorker: Completed, final output length={}",
        final_output.len()
    );

    Ok(OrchestrationResult {
        output: final_output,
        strategy: crate::orchestrator::DispatchStrategy::ManagerWorker,
        expert_chain,
        expert_outputs,
        tokens,
        duration_ms: 0,
        critique_records: Vec::new(),
    })
}

/// 主管专家拆解任务
async fn decompose_by_manager(
    orchestrator: &Orchestrator,
    manager_id: &str,
    profile: &TaskProfile,
) -> Result<Vec<SubTask>> {
    let system_prompt = orchestrator
        .get_expert(manager_id)
        .map(|a| a.persona.system_prompt.clone())
        .unwrap_or_else(|| "你是一个任务规划专家。".to_string());

    let messages = vec![
        Message {
            role: crate::runtime::Role::System,
            content: format!(
                "{} 你是一个任务规划专家。请将复杂任务拆解为独立的子任务。\n\n\
                 输出格式：每行一个子任务，格式为 \"[编号]. [描述]\"。\n\
                 每个子任务应该是独立的、可并行执行的。\n\
                 只需要输出子任务列表，不要其他内容。",
                system_prompt
            ),
            tool_call_id: None,
        },
        Message {
            role: crate::runtime::Role::User,
            content: format!("请将以下任务拆解为子任务：\n{}", profile.input),
            tool_call_id: None,
        },
    ];

    let response = orchestrator.runtime.call_llm_with_stats(messages).await?;

    // 解析子任务列表
    let sub_tasks: Vec<SubTask> = response
        .content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(i, line)| {
            // 去除编号前缀
            let desc = line
                .trim()
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
                .to_string();

            SubTask {
                id: format!("sub-{}", i + 1),
                description: desc,
                domain: profile
                    .domains
                    .first()
                    .cloned()
                    .unwrap_or(crate::orchestrator::TaskDomain::General),
                dependencies: vec![],
                assigned_expert: None,
                result: None,
                status: SubTaskStatus::Pending,
            }
        })
        .collect();

    Ok(sub_tasks)
}

/// 主管汇总工人结果
async fn summarize_by_manager(
    orchestrator: &Orchestrator,
    manager_id: &str,
    profile: &TaskProfile,
    worker_outputs: &HashMap<String, String>,
) -> Result<String> {
    let system_prompt = orchestrator
        .get_expert(manager_id)
        .map(|a| a.persona.system_prompt.clone())
        .unwrap_or_else(|| "你是一个综合分析师。".to_string());

    let mut workers_summary = String::new();
    for (worker_id, output) in worker_outputs {
        workers_summary.push_str(&format!(
            "\n### 工人 [{}] 的输出：\n{}\n",
            worker_id, output
        ));
    }

    let messages = vec![
        Message {
            role: crate::runtime::Role::System,
            content: format!(
                "{} 你是主管专家。请整合以下工人的输出，给出最终的综合答案。\
                 需要确保答案连贯、完整，解决原始问题。",
                system_prompt
            ),
            tool_call_id: None,
        },
        Message {
            role: crate::runtime::Role::User,
            content: format!(
                "原始任务：{}\n\n各工人的输出：{}\n\n请整合以上结果，给出最终答案。",
                profile.input, workers_summary
            ),
            tool_call_id: None,
        },
    ];

    let response = orchestrator.runtime.call_llm_with_stats(messages).await?;
    Ok(response.content)
}
