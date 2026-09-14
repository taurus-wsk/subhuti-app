//! # 领域层核心接口
//!
//! 定义领域专家的纯业务接口，不依赖任何外部框架类型。
//!
//! 六边形架构：
//! - 领域层定义接口（DomainExpert）
//! - 出站适配层实现适配器（DomainExpert → subhuti_core::ExpertAgent）
//! - 应用层通过出站端口（ExpertRepositoryPort）管理专家
//!
//! 这样领域层可以完全独立于框架，便于测试和提取为独立 crate。

use async_trait::async_trait;
use std::sync::Arc;

use crate::domain::events::{DomainEvent, DomainEventPublisher};
use crate::domain::ports::CommandPort;
use crate::domain::ports::FileSystemPort;
use crate::domain::ports::ToolchainPort;
use crate::domain::session_context::SessionContext;

/// 进度事件发送端：框架级结构化 `ProgressEvent` 的通道。
///
/// 取代原先「手拼 JSON 字符串」的 `mpsc::Sender<String>`，专家直接发类型化事件，
/// 应用层 `orchestrate_stream` 统一映射为前端 `StreamEvent`。
pub type ProgressTx = tokio::sync::mpsc::Sender<subhuti_core::progress::ProgressEvent>;

/// 领域技能信息（纯领域 DTO）
///
/// 专家直接暴露拥有的技能，Graph 不管理技能。
#[derive(Debug, Clone, serde::Serialize)]
pub struct DomainSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub parameters: Vec<String>,
}

/// 领域专家上下文（纯领域类型）
///
/// 封装专家执行时所需的上下文信息，不依赖框架类型。
#[derive(Debug, Clone)]
pub struct DomainContext {
    pub input: String,
    /// 会话 ID —— 一条会话链路内恒定，供会话归属、进度通道注册、SSE 关联使用。
    pub session_id: Option<String>,
    /// 追踪 ID —— 一次用户请求的全链路标识（TraceAppService 生成）。
    ///
    /// 跨模块串联标准：HTTP 入口 → TraceAppService → OrchestrationService →
    /// 引擎 ctx.metadata["trace_id"] → DomainContext，专家内所有 LLM/工具/事件
    /// 活动都应从本字段派生追踪标识，保证全链路可用 [`trace_id`] 关联。
    pub trace_id: Option<String>,
    pub user_id: Option<String>,
    /// 项目工作目录路径（前端聊天设置传入）
    pub workspace_folder: Option<String>,
    /// 自定义系统提示词（前端聊天设置传入，覆盖专家默认 system prompt）
    pub system_prompt: Option<String>,
    /// 历史消息（从框架 Session 传递，用于多轮对话上下文）
    pub history: Vec<DomainMessage>,
}

/// 领域执行上下文（纯领域类型）
///
/// 封装专家执行时所需的所有依赖，避免参数膨胀。
/// 未来添加新依赖（如 EventBus、Config）只需扩展此结构体。
pub struct DomainExecutionContext {
    /// 业务上下文（输入、会话、用户等）
    pub ctx: DomainContext,
    /// LLM 客户端
    pub llm: Arc<dyn DomainLlm>,
    /// 引擎侧 LLM（用于规划等框架能力，由适配器注入）
    pub engine_llm: Option<Arc<dyn subhuti_core::LLM>>,
    /// 数据仓库
    pub repository: Arc<dyn DomainRepository>,
    /// 技能ID（可选，用于技能执行）
    pub skill_id: Option<String>,
    /// 技能参数（可选）
    pub skill_params: Option<String>,
    /// Rust 工具链（可选，RustExpert 等需要编译验证的专家使用）
    pub toolchain: Option<Arc<dyn ToolchainPort>>,
    /// 藏经阁记忆引擎（可选，专家可访问结构化记忆系统）
    pub sutra_library: Option<Arc<dyn subhuti_core::SutraLibraryPort>>,
    /// 文件系统操作（可选，用于读写项目文件、搜索文件等）
    pub file_system: Option<Arc<dyn FileSystemPort>>,
    /// 命令行执行（可选，用于运行 cargo build、git 等命令）
    pub command: Option<Arc<dyn CommandPort>>,
    /// 进度报告通道（可选，用于实时推送执行进度到 SSE 流）
    /// 类型化为框架级 `ProgressEvent`，取代原先手拼 JSON 字符串的 `mpsc::Sender<String>`。
    pub progress_tx: Option<ProgressTx>,
    /// 领域事件发布端口（可选，用于把记忆检索等动作事件透传为 SSE 阶段流）。
    ///
    /// 取代直接持有框架 `EventBus`——领域层现在只依赖自有抽象 `DomainEventPublisher`，
    /// 由出站适配器把 `DomainEvent` 映射到框架 `EventBus`（依赖倒置，翻转原红色 ③ 可观测性依赖）。
    pub event_publisher: Option<Arc<dyn DomainEventPublisher>>,
    /// **框架级会话上下文**（可选）：专家可读写它，执行中产生的内容回流到框架，
    /// 使后续专家、查询接口、藏经阁沉淀都能看到这位专家说过什么。
    /// 与 `ctx.history`（专家私有、内存态）不同，这是框架共享资产。
    pub session_context: Option<Arc<SessionContext>>,
}

impl Clone for DomainExecutionContext {
    fn clone(&self) -> Self {
        Self {
            ctx: self.ctx.clone(),
            llm: self.llm.clone(),
            engine_llm: self.engine_llm.clone(),
            repository: self.repository.clone(),
            skill_id: self.skill_id.clone(),
            skill_params: self.skill_params.clone(),
            toolchain: self.toolchain.clone(),
            sutra_library: self.sutra_library.clone(),
            file_system: self.file_system.clone(),
            command: self.command.clone(),
            progress_tx: self.progress_tx.clone(),
            event_publisher: self.event_publisher.clone(),
            session_context: self.session_context.clone(),
        }
    }
}

impl std::fmt::Debug for DomainExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomainExecutionContext")
            .field("ctx", &self.ctx)
            .field("skill_id", &self.skill_id)
            .field("skill_params", &self.skill_params)
            .field("has_sutra_library", &self.sutra_library.is_some())
            .field("has_file_system", &self.file_system.is_some())
            .field("has_command", &self.command.is_some())
            .field("has_progress_tx", &self.progress_tx.is_some())
            .field("has_event_publisher", &self.event_publisher.is_some())
            .field("has_session_context", &self.session_context.is_some())
            .finish()
    }
}

/// 可观测性上下文：把散落在各调用点的 `event_bus` / `trace_id` / `session_id`
/// 收敛为一个结构体，消除「新增 adapter 容易漏接其中一个」的隐患。
///
/// 经由 [`DomainExecutionContext::trace_context`] 从领域上下文构造，传给
/// `TracedEngineLlm` / `TracedToolchainAdapter` 等包裹层，以及记忆检索等事件发射点。
#[derive(Clone)]
pub struct TraceContext {
    /// 领域事件发布端口（抽象）：取代直接持有框架 `EventBus`。
    pub publisher: Option<Arc<dyn DomainEventPublisher>>,
    pub trace_id: Option<String>,
    pub session_id: Option<String>,
}

impl TraceContext {
    /// 仅在「接入了事件发布端口 且 带非空 trace_id」时视为有效（可发射阶段事件）
    pub fn is_active(&self) -> bool {
        match &self.publisher {
            Some(_) => self
                .trace_id
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false),
            None => false,
        }
    }

    /// 把一条领域事件经发布端口发射出去（自动带 trace_id / session_id）。
    /// 未接入或 trace 为空时静默跳过，避免无关联噪声。
    pub async fn emit(&self, event: DomainEvent) {
        if let (Some(p), Some(tid)) = (&self.publisher, &self.trace_id) {
            if !tid.is_empty() {
                p.publish(event, tid, self.session_id.as_deref()).await;
            }
        }
    }
}

impl DomainExecutionContext {
    /// 从领域执行上下文构造可观测性上下文（一次性收敛 publisher / trace_id / session_id）
    pub fn trace_context(&self) -> TraceContext {
        TraceContext {
            publisher: self.event_publisher.clone(),
            trace_id: self.ctx.trace_id.clone(),
            session_id: self.ctx.session_id.clone(),
        }
    }
}

/// 领域专家 trait（纯领域接口）
///
/// 定义领域专家的核心能力：识别、技能暴露、执行。
/// 不依赖 subhuti 框架类型，便于独立测试和提取。
#[async_trait]
pub trait DomainExpert: Send + Sync {
    /// 获取专家唯一标识
    fn id(&self) -> &str;

    /// 获取专家名称
    fn name(&self) -> &str;

    /// 获取专家标签（用于匹配和分类）
    fn tags(&self) -> &[String];

    /// 获取专家拥有的技能列表
    fn skills(&self) -> &[DomainSkill];

    /// 执行专家逻辑（通用入口）
    ///
    /// # 参数
    /// - `exec_ctx`: 领域执行上下文，封装所有依赖
    ///
    /// # 返回
    /// - 执行结果字符串
    async fn run(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String>;

    /// 执行指定技能
    ///
    /// 根据技能ID执行对应的技能逻辑，每个专家可以实现多个技能。
    /// 默认实现：构建技能执行上下文，调用通用 run 方法。
    ///
    /// # 参数
    /// - `skill_id`: 技能ID
    /// - `params`: 技能参数（JSON格式字符串）
    /// - `exec_ctx`: 领域执行上下文
    ///
    /// # 返回
    /// - 技能执行结果字符串
    async fn execute_skill(
        &self,
        skill_id: &str,
        params: &str,
        exec_ctx: DomainExecutionContext,
    ) -> DomainResult<String> {
        // 默认实现：将技能信息附加到输入中，构建技能执行上下文
        let enhanced_input = format!(
            "技能ID: {}, 参数: {}\n\n{}",
            skill_id, params, exec_ctx.ctx.input
        );
        let skill_exec_ctx = DomainExecutionContext {
            ctx: DomainContext {
                input: enhanced_input,
                ..exec_ctx.ctx
            },
            skill_id: Some(skill_id.to_string()),
            skill_params: Some(params.to_string()),
            ..exec_ctx
        };
        self.run(skill_exec_ctx).await
    }

    /// 使用 LLM 自动规划并执行多技能组合
    ///
    /// 通用默认实现：让引擎 planner 生成技能执行计划，再由引擎按顺序驱动执行。
    /// 领域层只负责把"单步技能执行"供给引擎（通过 run_step 闭包），
    /// 规划的生成与循环驱动机制全部收敛在引擎。
    ///
    /// # 参数
    /// - `exec_ctx`: 领域执行上下文
    ///
    /// # 返回
    /// - 所有技能执行结果的汇总
    async fn plan_and_execute(&self, exec_ctx: DomainExecutionContext) -> DomainResult<String> {
        let input = exec_ctx.ctx.input.clone();
        let skills = self.skills().to_vec();
        let expert_name = self.name().to_string();

        tracing::info!(
            "[plan_and_execute] 专家={}, 输入长度={}, 可用技能数={}",
            expert_name,
            input.len(),
            skills.len()
        );

        // 2. 让引擎 planner 用 LLM 生成执行计划。
        //    （不发「正在分析需求」状态 plan——只透出最终生成的计划交付物，
        //      避免前端出现两个相邻 plan 徽标。）
        //    规划器可能返回「执行计划」或「提问」，若是提问则通过主动提问机制阻塞等待用户答复，
        //    把答复拼回输入后再次规划，直到得到明确的执行计划（信息收齐后再执行）。
        let skill_infos: Vec<subhuti_core::orchestrator::SkillInfo> = skills
            .iter()
            .map(|s| subhuti_core::orchestrator::SkillInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.parameters.clone(),
            })
            .collect();

        let engine_llm = exec_ctx
            .engine_llm
            .clone()
            .ok_or_else(|| DomainError::LlmError("缺少引擎 LLM，无法进行计划规划".to_string()))?;

        let progress_tx = exec_ctx.progress_tx.clone();
        let mut planning_input = input.clone();
        // 主动提问轮次上限：避免 LLM 反复要求提问（或前端未答复）导致无限循环。
        // 一旦超过上限，追加指令强制不再提问、直接基于已有信息产出执行计划。
        const MAX_ASK_ROUNDS: u32 = 2;
        let mut ask_rounds: u32 = 0;
        let plan = loop {
            let planned = subhuti_core::orchestrator::planner::generate_plan(
                &engine_llm,
                &planning_input,
                &skill_infos,
                &expert_name,
            )
            .await;

            // 规划失败（LLM 输出重试后仍不合法 / LLM 调用异常）**不再让整个请求崩掉**：
            // 降级为「单步直答」——挑一个纯 LLM 的 `*-chat` 技能跑一步，保证有交付物。
            // 之所以限定 chat 技能：它是纯推理、无副作用（不写文件、不跑命令），
            // 降级不会有破坏性；若专家没有 chat 技能则如实上抛错误。
            let planned = match planned {
                Ok(p) => p,
                Err(e) => match skills.iter().find(|s| s.id.ends_with("-chat")) {
                    Some(chat_skill) => {
                        tracing::warn!(
                            "[plan_and_execute] 专家={} 规划失败({}) → 降级为单步直答（技能={}）",
                            expert_name,
                            e,
                            chat_skill.id
                        );
                        send_phase_progress(
                            &progress_tx,
                            "plan",
                            &expert_name,
                            &format!("⚠️ 规划失败，降级为单步直答（{}）", chat_skill.id),
                        );
                        break subhuti_core::orchestrator::SkillPlan::new(
                            "规划失败，降级为单步直答",
                        )
                        .add_step(subhuti_core::orchestrator::PlanStep {
                            order: 1,
                            skill_id: chat_skill.id.clone(),
                            description: format!("直接回答用户需求：{}", input),
                            params: input.clone(),
                        });
                    }
                    None => return Err(DomainError::LlmError(e.to_string())),
                },
            };

            match planned {
                subhuti_core::orchestrator::PlanOrAsk::Plan(p) => break p,
                subhuti_core::orchestrator::PlanOrAsk::Ask(ask) => {
                    ask_rounds += 1;
                    if ask_rounds > MAX_ASK_ROUNDS {
                        tracing::warn!(
                            "[plan_and_execute] 专家={} 提问次数超过上限({})，强制停止提问直接规划",
                            expert_name,
                            MAX_ASK_ROUNDS
                        );
                        planning_input =
                            format!("{}（请直接给出执行计划，不要提问）", planning_input);
                        continue;
                    }
                    tracing::info!(
                        "[plan_and_execute] 专家={} 触发主动提问: {}",
                        expert_name,
                        ask.question
                    );
                    // 阻塞等待用户答复（前端单选卡片 → /ask-resolve 投递）
                    let answer = crate::domain::pending_ask::ask_user(
                        &progress_tx,
                        &ask.question,
                        ask.options.clone(),
                    )
                    .await;
                    if answer.is_empty() {
                        tracing::warn!(
                            "[plan_and_execute] 专家={} 提问超时/无答复，跳过本次提问继续规划",
                            expert_name
                        );
                        // 超时无答复：设置标记避免再次卡在同一提问，直接继续
                        planning_input = format!(
                            "{}（用户未答复上述提问，请基于已有信息直接规划）",
                            planning_input
                        );
                    } else {
                        // 把答复拼回输入，带着答复再次进入规划
                        planning_input = format!(
                            "{}\n\n用户选择: {}\n{}",
                            planning_input,
                            answer,
                            ask.context.clone().unwrap_or_default()
                        );
                    }
                }
            }
        };

        tracing::info!(
            "[plan_and_execute] 生成计划: description={}, steps={:?}",
            plan.description,
            plan.steps
                .iter()
                .map(|s| format!("{}:{:?}", s.skill_id, s.description))
                .collect::<Vec<_>>()
        );

        // 若 planner 判定无需执行任何技能（0 步骤，典型如问候语/普通对话），
        // 不再空跑执行链（否则会输出「共执行 0 个步骤」），而是退化为 LLM 直接对话回答。
        if plan.steps.is_empty() {
            send_phase_progress(
                &exec_ctx.progress_tx,
                "answer",
                &expert_name,
                &format!("💬 {} 正在回答...", expert_name),
            );
            tracing::info!(
                "[plan_and_execute] 专家={} 计划为空(0 步骤)，退化为直接对话回答",
                expert_name
            );
            let mut msgs = exec_ctx.ctx.history.clone();
            msgs.push(DomainMessage {
                role: DomainRole::System,
                content: format!(
                    "你是{}，请以该专家的身份直接、自然、简洁地回答用户。",
                    expert_name
                ),
            });
            msgs.push(DomainMessage {
                role: DomainRole::User,
                content: input.clone(),
            });
            // 真流式：逐 delta 推给前端（首字即出），同时累积为完整答案返回
            let answer =
                chat_stream_to_progress(&exec_ctx.llm, msgs, &exec_ctx.progress_tx).await?;
            send_phase_progress(
                &exec_ctx.progress_tx,
                "done",
                &expert_name,
                &format!("✅ {} 回答完成", expert_name),
            );
            return Ok(answer);
        }

        // 3. 进入自适应执行链：L1 常态 → L2 反馈重试 → L3 领域层降级兜底。
        //    L3 由领域层自实现 StepFallback（tool_fallback::DomainLlmFallback）提供：
        //    它与普通技能共享同一套上下文，直接完成失败步骤并输出与普通技能一致的结果，
        //    有 port 时附带文件/命令工具，否则纯文本兜底，且绝不触发递归。
        let fallback = crate::domain::tool_fallback::assemble_fallback(&exec_ctx, &expert_name);

        // 用引擎 LLM 编排出的步骤构造「待办清单」，先以未完成态推给前端，
        // 让用户在执行前就能看到一步步待办，随后随执行逐个打勾推进（进度式显示）。
        let total_steps = plan.steps.len();
        let base_todo: Vec<String> = plan
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| format!("- [ ] {}. [{}] {}", i + 1, s.skill_id, s.description))
            .collect();
        send_struct_progress(
            &progress_tx,
            "plan",
            &expert_name,
            &base_todo.join("\n"),
            0,
            total_steps,
        );

        let mut run_step = |skill_id: &str, params: &str, prev_input: &str| {
            let skill_id_owned = skill_id.to_string();
            let params_owned = params.to_string();
            let skill_exec_ctx = DomainExecutionContext {
                ctx: DomainContext {
                    input: prev_input.to_string(),
                    system_prompt: exec_ctx.ctx.system_prompt.clone(),
                    ..exec_ctx.ctx.clone()
                },
                skill_id: Some(skill_id_owned.clone()),
                skill_params: Some(params_owned.clone()),
                ..exec_ctx.clone()
            };
            let expert = self;
            async move {
                expert
                    .execute_skill(&skill_id_owned, &params_owned, skill_exec_ctx)
                    .await
                    .map_err(|e| match e {
                        // 前置条件未满足（配置缺失/端口未注入）：重试无意义，
                        // 映射为框架的 Precondition，使自适应链跳过 L2 直接 L3 降级。
                        DomainError::Precondition(m) => subhuti_core::Error::Precondition(m),
                        other => subhuti_core::Error::Expert(other.to_string()),
                    })
            }
        };

        let execution = match fallback {
            Some(fb) => {
                let tx = progress_tx.clone();
                let name = expert_name.clone();
                subhuti_core::orchestrator::execute_plan_adaptive(
                    &expert_name,
                    &plan,
                    &subhuti_core::orchestrator::AdaptiveOptions::default(),
                    &*fb,
                    move |msg: &str| send_step_progress(&tx, &name, msg),
                    &mut run_step,
                )
                .await
                .map_err(|e| DomainError::LlmError(e.to_string()))?
            }
            None => {
                let tx = progress_tx.clone();
                let name = expert_name.clone();
                subhuti_core::orchestrator::planner::execute_plan(
                    &expert_name,
                    &plan,
                    move |msg: &str| send_step_progress(&tx, &name, msg),
                    &mut run_step,
                )
                .await
                .map_err(|e| DomainError::LlmError(e.to_string()))?
            }
        };

        tracing::info!("[plan_and_execute] 专家={} 执行完成", expert_name);

        // 产品规则（2026-09-13 定稿）：**任一步骤失败 → 整体失败**。
        // 执行链本身不判成败（它如实返回每步结果），成败在这里定：
        // 有失败步骤时不再当成功返回，而是以错误上抛 →
        // 框架 success=false → 编排层发 StreamEvent::Error → MCP/HTTP 侧
        // isError=true，调用方可程序化判别（而不是把失败文案当成功结果）。
        //
        // 为什么取严（而非"全部步骤均失败才算失败"）：
        // ① 与框架多专家路径（dispatch_with_plan 的 all_ok）语义一致，全局只有一条口径；
        // ② 不随 planner 拆步粒度抖动——同一请求可能被拆成 1/2/3 步，
        //    "全败才算失败"会让同一请求时而成功时而失败；
        // ③ 失败必须可程序化判别，不能只埋在产物正文里。
        // 代价（已知并接受）：`[生成代码][编译验证]` 里编译失败会把"已生成代码"的
        // 部分交付一并判为整体失败——产物仍完整带出，调用方可读细节。
        //
        // 注意：产物（含每步失败原因与可操作提示）随错误一同带出，信息不丢。
        if execution.has_failed_steps() {
            tracing::warn!(
                "[plan_and_execute] 专家={} {}/{} 个步骤失败（任一步失败即整体判失败）",
                expert_name,
                execution.failed_steps(),
                execution.total_steps
            );
            send_phase_progress(
                &progress_tx,
                "failed",
                &expert_name,
                &format!(
                    "❌ {} 有 {}/{} 个步骤失败，整体判为失败",
                    expert_name,
                    execution.failed_steps(),
                    execution.total_steps
                ),
            );
            return Err(DomainError::ExecutionError(execution.output));
        }

        Ok(execution.output)
    }
}

/// 推送一个结构化步骤事件
///
/// `source`：框架级步骤为 `"框架"`，专家级步骤为专家名。
/// `done`/`total` 仅在有真实计数时传入，前端据此渲染 `(done/total)`。
pub fn emit_step(
    tx: &Option<ProgressTx>,
    source: &str,
    phase: Option<&str>,
    message: &str,
    todo_state: Option<&str>,
    done: usize,
    total: usize,
) {
    if let Some(sender) = tx {
        let _ = sender.try_send(subhuti_core::progress::ProgressEvent::Step {
            message: message.to_string(),
            source: source.to_string(),
            phase: phase.map(|p| p.to_string()),
            todo_state: todo_state.map(|t| t.to_string()),
            done: if done > 0 { Some(done) } else { None },
            total: if total > 0 { Some(total) } else { None },
        });
    }
}

/// 推送流式文本分片（模型真实输出的增量）
pub fn emit_chunk(tx: &Option<ProgressTx>, content: &str) {
    if content.is_empty() {
        return;
    }
    if let Some(sender) = tx {
        let _ = sender.try_send(subhuti_core::progress::ProgressEvent::Chunk {
            content: content.to_string(),
        });
    }
}

/// 推送主动提问事件
pub fn emit_ask(tx: &Option<ProgressTx>, ask_id: &str, question: &str, options: &[String]) {
    if let Some(sender) = tx {
        let _ = sender.try_send(subhuti_core::progress::ProgressEvent::Ask {
            ask_id: ask_id.to_string(),
            question: question.to_string(),
            options: options.to_vec(),
        });
    }
}

/// 推送结构化进度事件（前端 SSE 渲染的关键载荷）
///
/// - `type_name = "plan"`：待办清单（`message` 为 `- [ ]` markdown），`todo_state` 携带它，
///   前端据此原位替换渲染打勾清单；展示文案另给「已生成执行计划 (N 步)」。
/// - 其他：视作带 phase 的步骤事件。
fn send_struct_progress(
    tx: &Option<ProgressTx>,
    type_name: &str,
    source: &str,
    message: &str,
    done: usize,
    total: usize,
) {
    if type_name == "plan" {
        emit_step(
            tx,
            source,
            Some("plan"),
            &format!("已生成执行计划 ({} 步)", total),
            Some(message),
            done,
            total,
        );
    } else {
        emit_step(tx, source, None, message, None, done, total);
    }
}

/// 把核心编排层的原始进度文案包装成 step 事件（带 source）后推送。
///
/// core 的 `execute_plan`/`execute_plan_adaptive` 通过回调发的是**纯文本**
/// （如「⚡ 步骤 1/3: …」「✅ 步骤 1/3 完成: …」）。包装成 step 事件后即可透传。
fn send_step_progress(tx: &Option<ProgressTx>, source: &str, message: &str) {
    emit_step(tx, source, None, message, None, 0, 0);
}

/// 推送带阶段标识的执行进度事件。
///
/// 强调「当前处于哪个 workflow 阶段」(plan/answer/done 等)，
/// 并携带 `source` 真实专家名，让编排层能透传 `phase`/`source` 给前端做分类渲染。
fn send_phase_progress(tx: &Option<ProgressTx>, phase: &str, expert: &str, message: &str) {
    emit_step(tx, expert, Some(phase), message, None, 0, 0);
}

/// 以**流式**方式调用 LLM：每个 delta 既通过进度通道增量下发（真流式），
/// 又累积成完整文本返回；未配置进度通道时只是多一层回调，无额外开销。
///
/// 仅用于「输出即为最终回答」的场景（闲聊、代码审查/修复/重构、0 步骤直接对话）。
/// 中间产物（计划、代码生成、修复回灌）不要走这里，否则会把过程文本也吐给用户。
pub(crate) async fn chat_stream_to_progress(
    llm: &Arc<dyn DomainLlm>,
    messages: Vec<DomainMessage>,
    tx: &Option<ProgressTx>,
) -> DomainResult<String> {
    let tx_owned = tx.clone();
    let on_delta: Box<dyn Fn(String) + Send> = Box::new(move |delta: String| {
        emit_chunk(&tx_owned, &delta);
    });
    llm.chat_stream(messages, on_delta).await
}

/// 领域 LLM 接口（纯领域类型）
///
/// 定义领域层使用的 LLM 能力，不依赖框架实现。
#[async_trait]
pub trait DomainLlm: Send + Sync {
    /// 发送消息并获取响应
    async fn chat(&self, messages: Vec<DomainMessage>) -> DomainResult<String>;

    /// 流式对话：模型每产出一个 delta 就回调一次 `on_delta`
    ///
    /// 默认实现**退化为一次性 [`chat`]**：拿到全文后作为单个 delta 回调一次。
    /// 这样未支持流式的适配器（测试替身、其他 provider）无需改动即可继续编译运行，
    /// 前端也会退化成「一次性显示」，不会报错。
    ///
    /// 返回值为**完整文本**，便于调用方在流结束后仍能拿到全文。
    async fn chat_stream(
        &self,
        messages: Vec<DomainMessage>,
        on_delta: Box<dyn Fn(String) + Send>,
    ) -> DomainResult<String> {
        let full = self.chat(messages).await?;
        on_delta(full.clone());
        Ok(full)
    }

    /// 获取模型名称
    fn model_name(&self) -> &str;
}

/// 领域消息类型
#[derive(Debug, Clone)]
pub struct DomainMessage {
    pub role: DomainRole,
    pub content: String,
}

/// 领域消息角色
#[derive(Debug, Clone, PartialEq)]
pub enum DomainRole {
    System,
    User,
    Assistant,
}

/// 领域结果类型
pub type DomainResult<T> = Result<T, DomainError>;

/// 领域数据仓库接口（纯领域接口）
///
/// 定义领域层使用的数据访问能力，不依赖具体数据库实现。
/// 支持键值存储模式，便于专家存储和读取数据。
#[async_trait]
pub trait DomainRepository: Send + Sync {
    /// 保存数据
    async fn save(&self, key: &str, value: &str) -> DomainResult<()>;

    /// 加载数据
    async fn load(&self, key: &str) -> DomainResult<Option<String>>;

    /// 删除数据
    async fn delete(&self, key: &str) -> DomainResult<()>;

    /// 批量保存数据
    async fn save_batch(&self, items: &[(&str, &str)]) -> DomainResult<()>;

    /// 批量加载数据
    async fn load_batch(&self, keys: &[&str]) -> DomainResult<Vec<(String, String)>>;
}

/// 领域错误类型
#[derive(Debug)]
pub enum DomainError {
    LlmError(String),
    ContextError(String),
    ExecutionError(String),
    /// 前置条件未满足（如缺少必需配置、必要端口未注入）。
    ///
    /// 与 `ExecutionError` 的区别在于**语义**：这类失败是确定性的，
    /// 回喂失败反馈再重试也不会成功，执行链应直接降级而非空转重试。
    Precondition(String),
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainError::LlmError(e) => write!(f, "LLM 错误: {}", e),
            DomainError::ContextError(e) => write!(f, "上下文错误: {}", e),
            DomainError::ExecutionError(e) => write!(f, "执行错误: {}", e),
            DomainError::Precondition(e) => write!(f, "前置条件未满足: {}", e),
        }
    }
}

impl std::error::Error for DomainError {}
