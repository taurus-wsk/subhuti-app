//! # 自适应执行链（Adaptive Execution）
//!
//! 在 `planner::execute_plan` 的基础上，为每个步骤提供三档自适应策略：
//!
//! ```text
//! 对每个 step：
//!   L1 常态      直接调用已注册技能（确定性、质量最高）
//!      │ 成功 → 下一个 step
//!      │ 失败 ↓
//!   L2 反馈重试   把上一次失败结果回喂，重试（最多 max_retry 次）
//!      │ 成功 → 下一个 step
//!      │ 仍失败 ↓
//!   L3 降级      阈值已到 → 由 fallback 用 LLM 看工具清单 + 失败历史，临时编排
//!      │ 成功 → 下一个 step
//!      │ 仍失败 → 作为失败步骤记录（带失败历史），继续后续步骤
//! ```
//!
//! 设计边界（保持引擎纯净）：
//! - 引擎 `execute_plan_adaptive` 只做"三档调度 + 反馈注入 + 进度 + 汇总"这些纯机制
//! - 单步执行 `run_step` 与 L3 降级 `StepFallback` 均为**回调/接口注入**，引擎不感知任何业务
//! - L3 的 LLM-tool 协议由 `StepFallback` 实现者决定（引擎只面向接口）

use std::future::Future;
use std::pin::Pin;

use crate::orchestrator::planner::PlanStep;
use crate::orchestrator::SkillPlan;
use crate::runtime::llm::{LLMResponse, Message, Role, ToolInfo, LLM};
use crate::{Error, Result};

/// 自适应执行策略参数
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveOptions {
    /// L2：技能失败后携带反馈的重试次数（默认 2，即最多调用 3 次）
    pub max_retry: usize,
    /// L3：降级后 LLM 多轮工具编排的迭代上限（默认 3）
    pub max_fallback_iters: usize,
}

impl Default for AdaptiveOptions {
    fn default() -> Self {
        Self {
            max_retry: 2,
            max_fallback_iters: 3,
        }
    }
}

/// L3 降级执行器（trait 对象）
///
/// 由领域层/上层实现：拿到失败步骤与失败历史后，用 LLM 按已注册工具临时编排并执行。
/// 引擎只调用它，不关心内部协议。
pub trait StepFallback: Send + Sync {
    /// 对失败步骤做降级编排
    fn execute<'a>(
        &'a self,
        step: &'a PlanStep,
        failure_history: &'a [String],
    ) -> BoxFuture<'a, Result<String>>;
}

/// 工具执行器：按名称 + JSON 参数执行一个已注册工具
///
/// 领域层把 `exec_ctx` 里的 port（file_system / command / toolchain ...）包装成执行器注入即可。
pub trait ToolExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        name: &'a str,
        args: serde_json::Value,
    ) -> BoxFuture<'a, Result<String>>;
}

/// 引擎内部使用的动态 future 别名（`Send`，供异步 trait / async_trait 场景跨线程 await）
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// L3 的 **默认参考实现**：直接复用 LLM 原生的 `chat_with_tools`（function calling）
/// 做多轮工具编排，直到 LLM 给出最终结果或达到迭代上限。
///
/// 这是"引擎内引用工具"的落点：引擎自己不造工具，而是由领域层把已注册的 port
/// 渲染成 `Vec<ToolInfo>` 并提供 `ToolExecutor`，本结构体只负责 ReAct 式循环。
pub struct LlmToolFallback {
    llm: std::sync::Arc<dyn LLM>,
    tools: Vec<ToolInfo>,
    executor: std::sync::Arc<dyn ToolExecutor>,
    expert_desc: String,
    max_iters: usize,
}

impl LlmToolFallback {
    pub fn new(
        llm: std::sync::Arc<dyn LLM>,
        tools: Vec<ToolInfo>,
        executor: std::sync::Arc<dyn ToolExecutor>,
        expert_desc: &str,
    ) -> Self {
        Self {
            llm,
            tools,
            executor,
            expert_desc: expert_desc.to_string(),
            max_iters: 0, // 使用方通常在构造后覆盖
        }
    }

    pub fn with_max_iters(mut self, max_iters: usize) -> Self {
        self.max_iters = max_iters;
        self
    }
}

impl StepFallback for LlmToolFallback {
    fn execute<'a>(
        &'a self,
        step: &'a PlanStep,
        failure_history: &'a [String],
    ) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let goal = format!(
                "步骤：{}（skill_id={}），参数：{}",
                step.description, step.skill_id, step.params
            );
            let history = failure_history.join("\n");
            let system_prompt = format!(
                "你是 {}，现在你的预定义技能执行失败，请改用给定的工具直接完成任务，不要调用预定义技能。\n\n目标：{}",
                self.expert_desc, goal
            );
            let user_prompt = format!(
                "历史上预定义技能尝试后的失败信息（按顺序排列）：\n{}\n\n请使用提供的工具完成目标。当完成时，直接用文字输出结果，不要再调用工具。",
                history
            );

            let mut messages = vec![
                Message {
                    role: Role::System,
                    content: system_prompt,
                    tool_call_id: None,
                },
                Message {
                    role: Role::User,
                    content: user_prompt,
                    tool_call_id: None,
                },
            ];

            let max_iters = if self.max_iters == 0 {
                3
            } else {
                self.max_iters
            };
            for _ in 0..max_iters {
                let resp: LLMResponse = self
                    .llm
                    .chat_with_tools(messages.clone(), self.tools.clone())
                    .await?;
                if let Some(tc) = resp.tool_call {
                    let out = self.executor.execute(&tc.name, tc.arguments).await?;
                    messages.push(Message {
                        role: Role::Tool,
                        content: out,
                        tool_call_id: Some(tc.id),
                    });
                    continue;
                }
                // 无工具调用 → LLM 已给出最终结果
                return Ok(resp.content.trim().to_string());
            }
            Err(Error::Expert(
                "L3 降级工具编排达到迭代上限，仍未完成目标".into(),
            ))
        })
    }
}

/// 按顺序执行计划，并为每个步骤应用 L1 → L2 → L3 自适应策略。
///
/// # 参数
/// - `expert_name`: 专家名称（用于结果汇总文案）
/// - `plan`: 已生成的执行计划
/// - `options`: 自适应策略参数（重试次数 / 降级迭代上限）
/// - `fallback`: L3 降级执行器（接口注入，引擎不感知其实现）
/// - `on_progress`: 进度回调（SSE 推送等）
/// - `run_step`: 单步执行器 `FnMut(skill_id, params, input_with_feedback) -> Future`
///   - 第 1 次调用传入原始输入；后续重试会在输入末尾追加"上一次失败反馈"
///
/// # 返回
/// - 汇总后的执行结果 markdown（含降级/重试标记）
pub async fn execute_plan_adaptive<F, Fut>(
    expert_name: &str,
    plan: &SkillPlan,
    options: &AdaptiveOptions,
    fallback: &dyn StepFallback,
    mut on_progress: impl FnMut(&str),
    mut run_step: F,
) -> Result<String>
where
    F: FnMut(&str, &str, &str) -> Fut,
    Fut: std::future::Future<Output = Result<String>>,
{
    let total_steps = plan.steps.len();

    on_progress(&format!(
        "📋 执行计划：{}\n共 {} 个步骤（自适应模式）",
        plan.description,
        plan.step_count()
    ));

    let mut results: Vec<String> = Vec::new();
    let mut prev_output: Option<String> = None;

    for (idx, step) in plan.steps.iter().enumerate() {
        let step_num = idx + 1;
        let input = prev_output.clone().unwrap_or_default();

        on_progress(&format!(
            "⚡ 步骤 {}/{}: [{}] {} - 执行中...",
            step_num, total_steps, step.skill_id, step.description
        ));

        // ── L1 + L2：带反馈重试 ──
        let mut failures: Vec<String> = Vec::new();
        let mut final_output: Option<String> = None;
        let mut last_err: Option<String> = None;

        for attempt in 0..=options.max_retry {
            let run_input = if failures.is_empty() {
                input.clone()
            } else {
                format!(
                    "{}\n\n==== 上一次失败反馈 ====\n{}",
                    input,
                    failures.last().map(String::as_str).unwrap_or_default()
                )
            };

            if attempt > 0 {
                on_progress(&format!(
                    "🔄 步骤 {}/{} 重试 {}/{}（携带上次失败反馈）...",
                    step_num, total_steps, attempt, options.max_retry
                ));
            }

            match run_step(&step.skill_id, &step.params, &run_input).await {
                Ok(out) => {
                    final_output = Some(out);
                    last_err = None;
                    break;
                }
                Err(e) => {
                    failures.push(e.to_string());
                    last_err = Some(e.to_string());
                }
            }
        }

        // ── L2 耗尽 → L3 降级 ──
        if final_output.is_none() && failures.len() > 0 {
            on_progress(&format!(
                "⚠️ 步骤 {}/{} 技能执行失败 {} 次，降级为 LLM 按工具临时编排...",
                step_num,
                total_steps,
                failures.len()
            ));
            match fallback.execute(step, &failures).await {
                Ok(out) => {
                    final_output = Some(out);
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                }
            }
        }

        // ── 记录结果，注入下一步输入 ──
        match final_output {
            Some(output) => {
                on_progress(&format!(
                    "✅ 步骤 {}/{} 完成: {}",
                    step_num, total_steps, step.description
                ));
                results.push(format!(
                    "## 步骤 {}: {}\n\n{}",
                    step_num, step.description, output
                ));
                prev_output = Some(output);
            }
            None => {
                let err = last_err.unwrap_or_else(|| "未知错误".to_string());
                on_progress(&format!(
                    "❌ 步骤 {}/{} 失败: {} - 错误: {}",
                    step_num, total_steps, step.description, err
                ));
                results.push(format!(
                    "## 步骤 {}: {} [失败]\n\n错误: {}",
                    step_num, step.description, err
                ));
                prev_output = Some(format!("上一步失败: {}", err));
            }
        }
    }

    Ok(format!(
        "# {} 执行结果\n\n{}\n\n---\n共执行 {} 个步骤（自适应模式）",
        expert_name,
        results.join("\n\n---\n\n"),
        total_steps
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::planner::PlanStep;
    use crate::orchestrator::SkillPlan;
    use std::sync::{Arc, Mutex};

    struct FakeFallback {
        should_succeed: bool,
        called: Mutex<usize>,
    }

    impl StepFallback for FakeFallback {
        fn execute<'a>(
            &'a self,
            step: &'a PlanStep,
            history: &'a [String],
        ) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                *self.called.lock().unwrap() += 1;
                if self.should_succeed {
                    Ok(format!(
                        "fallback-ok:{}:hist={}",
                        step.skill_id,
                        history.len()
                    ))
                } else {
                    Err(Error::Expert("fallback 也失败".into()))
                }
            })
        }
    }

    #[tokio::test]
    async fn L1_直接成功不走L2L3() {
        let plan = SkillPlan::new("L1").add_step(PlanStep {
            order: 1,
            skill_id: "ok".into(),
            description: "一次即成".into(),
            params: "".into(),
        });
        let fallback = FakeFallback {
            should_succeed: false,
            called: Mutex::new(0),
        };
        let mut steps = 0usize;
        let result = execute_plan_adaptive(
            "测试",
            &plan,
            &AdaptiveOptions::default(),
            &fallback,
            |_| {},
            |_, _, _| {
                steps += 1;
                async move { Ok("ok-output".into()) }
            },
        )
        .await
        .unwrap();
        assert_eq!(steps, 1);
        assert_eq!(*fallback.called.lock().unwrap(), 0);
        assert!(result.contains("ok-output"));
        assert!(!result.contains("降级"));
    }

    #[tokio::test]
    async fn L2_首败二次带反馈成功() {
        let plan = SkillPlan::new("L2").add_step(PlanStep {
            order: 1,
            skill_id: "s".into(),
            description: "重试".into(),
            params: "".into(),
        });
        let fallback = FakeFallback {
            should_succeed: false,
            called: Mutex::new(0),
        };
        let inputs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
        let inputs2 = inputs.clone();
        let calls: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
        let calls2 = calls.clone();
        let result = execute_plan_adaptive(
            "测试",
            &plan,
            &AdaptiveOptions::default(),
            &fallback,
            |_| {},
            move |_, _, input: &str| {
                let inputs3 = inputs2.clone();
                let calls3 = calls2.clone();
                let input = input.to_string();
                async move {
                    *calls3.lock().unwrap() += 1;
                    let n = *calls3.lock().unwrap();
                    inputs3.lock().unwrap().push(input.clone());
                    if n < 2 {
                        Err(Error::Expert("第一次失败".into()))
                    } else {
                        Ok("retry-ok".into())
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(
            *calls.lock().unwrap(),
            2,
            "应调用两次：一次失败一次重试成功"
        );
        assert_eq!(*fallback.called.lock().unwrap(), 0, "不应触发 L3");
        // 第二次输入应包含失败反馈
        let ins = inputs.lock().unwrap();
        assert!(ins[1].contains("第一次失败"));
        assert!(result.contains("retry-ok"));
    }

    #[tokio::test]
    async fn L3_重试耗尽后降级成功() {
        let plan = SkillPlan::new("L3").add_step(PlanStep {
            order: 1,
            skill_id: "bad".into(),
            description: "总会失败".into(),
            params: "P".into(),
        });
        let fallback = FakeFallback {
            should_succeed: true,
            called: Mutex::new(0),
        };
        let mut state = 0usize;
        let result = execute_plan_adaptive(
            "测试",
            &plan,
            &AdaptiveOptions::default(), // max_retry=2 → 共尝试3次
            &fallback,
            |_| {},
            |_, _, _| {
                state += 1;
                async move { Err(Error::Expert("技能始终失败".into())) }
            },
        )
        .await
        .unwrap();

        assert_eq!(state, AdaptiveOptions::default().max_retry + 1);
        assert_eq!(*fallback.called.lock().unwrap(), 1, "应触发一次 L3");
        assert!(result.contains("fallback-ok"), "降级结果应进入产物");
        // 失败历史条数 = max_retry+1
        assert!(result.contains("hist=3"));
    }

    #[tokio::test]
    async fn L3_降级也失败则记为失败步骤并继续() {
        let plan = SkillPlan::new("L3fail")
            .add_step(PlanStep {
                order: 1,
                skill_id: "bad".into(),
                description: "全败".into(),
                params: "".into(),
            })
            .add_step(PlanStep {
                order: 2,
                skill_id: "next".into(),
                description: "后续步".into(),
                params: "".into(),
            });
        let fallback = FakeFallback {
            should_succeed: false,
            called: Mutex::new(0),
        };
        let result = execute_plan_adaptive(
            "测试",
            &plan,
            &AdaptiveOptions::default(),
            &fallback,
            |_| {},
            |skill_id, _, _| {
                let sid = skill_id.to_string();
                async move {
                    if sid == "bad" {
                        Err(Error::Expert("技能始终失败".into()))
                    } else {
                        Ok("next-ok".into())
                    }
                }
            },
        )
        .await
        .unwrap();

        // 只有失败的第一个步骤触发降级（且降级也失败）
        assert_eq!(*fallback.called.lock().unwrap(), 1);
        assert!(result.contains("[失败]"));
        assert!(result.contains("后续步"));
        assert!(result.contains("next-ok"), "后续步骤正常执行");
        assert!(!result.contains("fallback-ok"));
    }
}
