//! # 领域专家适配器
//!
//! 将领域层定义的 `DomainExpert` 接口转换为 Subhuti 框架的 `ExpertAgent` 接口。
//!
//! 六边形架构：
//! - 领域层定义接口（DomainExpert）
//! - 出站适配层实现适配器（DomainExpertAdapter）
//! - 适配器内部持有 DomainExpert 实例，实现 ExpertAgent 接口
//! - Subhuti 框架通过 ExpertAgent 接口调用专家

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

use subhuti_core::event::EventBus;

use crate::adapter::outbound::event_publisher_adapter::SubhutiEventPublisher;
use crate::domain::events::{DomainEvent, DomainEventPublisher};
use subhuti_core::orchestrator::{
    AgentContext, ExpertAgent, ExpertState, FromState, Llm, SkillInfo,
};
use subhuti_core::runtime::llm::{LLMConfig, LLMProvider, LLMResponse, ToolInfo};
use subhuti_core::Result;

use crate::adapter::outbound::rust_toolchain_adapter::TracedToolchainAdapter;
use crate::application::observer::{record_fn_log, LogLevel};
use crate::application::session_manager::SessionManager;
use crate::domain::ports::CommandPort;
use crate::domain::ports::FileSystemPort;
use crate::domain::ports::ToolchainPort;
use crate::domain::session_context::SessionContext;
use crate::domain::traits::{
    DomainContext, DomainError, DomainExecutionContext, DomainExpert, DomainLlm, DomainMessage,
    DomainRepository, DomainResult, DomainRole, TraceContext,
};

/// 领域专家适配器
///
/// 将领域层的 DomainExpert 转换为 Subhuti 框架的 ExpertAgent。
pub struct DomainExpertAdapter<D: ?Sized>
where
    D: DomainExpert,
{
    domain_expert: Arc<D>,
    /// 缓存的技能信息（在创建时预计算，避免重复转换）
    skills: Vec<SkillInfo>,
    /// 数据仓库（通过依赖注入传入，供领域专家使用）
    repository: Arc<dyn DomainRepository>,
    /// Rust 工具链（可选，供 RustExpert 等需要编译验证的专家使用）
    toolchain: Option<Arc<dyn ToolchainPort>>,
    /// 文件系统操作（可选，用于读写项目文件、搜索文件等）
    file_system: Option<Arc<dyn FileSystemPort>>,
    /// 命令行执行（可选，用于运行 cargo build、git 等命令）
    command: Option<Arc<dyn CommandPort>>,
    /// 框架事件总线（可选，用于把 LLM 调用等动作事件发到 EventBus，
    /// 由 ProgressEventBridge 桥接成 SSE 阶段流）
    event_bus: Option<Arc<EventBus>>,
    /// 框架级会话上下文管理者（可选）：
    /// ① 执行前从框架上下文取历史注入 LLM；
    /// ② 执行后把本专家的问答**回流**进框架上下文，供后续专家/查询接口消费。
    session_manager: Option<Arc<SessionManager>>,
}

impl<D: ?Sized> DomainExpertAdapter<D>
where
    D: DomainExpert,
{
    /// 创建新的适配器实例
    pub fn new(
        domain_expert: Arc<D>,
        repository: Arc<dyn DomainRepository>,
        toolchain: Option<Arc<dyn ToolchainPort>>,
        file_system: Option<Arc<dyn FileSystemPort>>,
        command: Option<Arc<dyn CommandPort>>,
        event_bus: Option<Arc<EventBus>>,
    ) -> Self {
        // 预计算并缓存技能信息，避免每次调用 skills() 时重复转换
        let skills = domain_expert
            .skills()
            .iter()
            .map(|s| SkillInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.parameters.clone(),
            })
            .collect();

        Self {
            domain_expert,
            skills,
            repository,
            toolchain,
            file_system,
            command,
            event_bus,
            session_manager: None,
        }
    }

    /// 设置框架级会话上下文管理者（可选）：注入后专家读写的是框架共享上下文，
    /// 历史注入与专家记忆回流都走它
    pub fn with_session_manager(mut self, manager: Arc<SessionManager>) -> Self {
        self.session_manager = Some(manager);
        self
    }

    /// 获取底层领域专家实例
    pub fn inner(&self) -> &Arc<D> {
        &self.domain_expert
    }

    /// 获取数据仓库实例
    pub fn repository(&self) -> &Arc<dyn DomainRepository> {
        &self.repository
    }
}

#[async_trait]
impl<D: ?Sized> ExpertAgent for DomainExpertAdapter<D>
where
    D: DomainExpert,
{
    fn id(&self) -> &str {
        self.domain_expert.id()
    }

    fn name(&self) -> &str {
        self.domain_expert.name()
    }

    fn tags(&self) -> &[String] {
        self.domain_expert.tags()
    }

    fn skills(&self) -> &[SkillInfo] {
        // 返回预计算并缓存的技能信息
        &self.skills
    }

    async fn run(&self, ctx: &mut AgentContext, state: &ExpertState) -> Result<String> {
        // 1. 从框架状态中提取 LLM
        let Llm(llm) = Llm::from_state(state)?;

        // 2. 创建领域 LLM 适配器（携带 trace_id / session_id，使专家内 LLM 调用可与全链路关联）
        let session_id = ctx.session.id().to_string();
        // 框架级会话上下文：历史从这里读，专家记忆也回流到这里（而非专家私有状态）
        let session_context = self
            .session_manager
            .as_ref()
            .map(|m| m.context(&session_id));

        // 3. 构建领域上下文
        // 从框架 Session 提取历史消息并转换为 DomainMessage
        let history = ctx
            .session
            .messages()
            .into_iter()
            .filter_map(|msg| {
                let role = match msg.role {
                    subhuti_core::runtime::llm::Role::System => DomainRole::System,
                    subhuti_core::runtime::llm::Role::User => DomainRole::User,
                    subhuti_core::runtime::llm::Role::Assistant => DomainRole::Assistant,
                    subhuti_core::runtime::llm::Role::Tool => return None, // 跳过 Tool 消息
                };
                Some(DomainMessage {
                    role,
                    content: msg.content,
                })
            })
            .collect();

        let domain_ctx = DomainContext {
            input: ctx.input.clone(),
            session_id: Some(ctx.session.id().to_string()),
            // trace_id 由框架经 ctx.metadata 透传（引擎在 graph_state 中写入），这里注入领域上下文
            trace_id: ctx
                .metadata
                .get("trace_id")
                .cloned()
                .filter(|s| !s.is_empty()),
            user_id: None,
            workspace_folder: ctx
                .metadata
                .get("workspace_folder")
                .cloned()
                .filter(|s| !s.is_empty()),
            system_prompt: ctx
                .metadata
                .get("system_prompt")
                .cloned()
                .filter(|s| !s.is_empty()),
            history,
        };

        // 4. 检查是否指定了技能执行
        // 从上下文中提取 skill_id 和 params（通过 metadata 传递）
        let skill_id = ctx
            .metadata
            .get("skill_id")
            .unwrap_or(&"".to_string())
            .clone();
        let params = ctx
            .metadata
            .get("skill_params")
            .unwrap_or(&"".to_string())
            .clone();

        // 5. 构建领域执行上下文（封装所有依赖，避免参数膨胀）
        // 进度通道从 per-request 的 AgentContext.progress 取（取代旧版全局注册表）
        let progress_tx = ctx.progress.clone();

        // 收敛 publisher / trace_id / session_id 为单一 TraceContext，统一传给包裹层，
        // 消除「新增 adapter 容易漏接其中一个」的隐患（与 TracedEngineLlm 同模式）。
        // 领域层只认识自有抽象 DomainEventPublisher；这里在出站层把框架 EventBus 包成它。
        let event_publisher: Option<Arc<dyn DomainEventPublisher>> =
            self.event_bus.as_ref().map(|b| {
                Arc::new(SubhutiEventPublisher::new(b.clone())) as Arc<dyn DomainEventPublisher>
            });

        let trace_ctx = TraceContext {
            publisher: event_publisher.clone(),
            trace_id: domain_ctx.trace_id.clone(),
            session_id: Some(session_id.clone()),
        };

        // 专家自身的 LLM 出口：携带同一 TraceContext，让领域层每次 LLM 调用都能发射阶段事件
        let domain_llm = Arc::new(SubhutiLlmAdapter {
            llm: llm.clone(),
            trace: trace_ctx.clone(),
            session_context: session_context.clone(),
            // 专家身份：回流到框架上下文时标记「这条记忆是谁产出的」
            expert_id: self.domain_expert.id().to_string(),
            expert_name: self.domain_expert.name().to_string(),
            inject_limit: self
                .session_manager
                .as_ref()
                .map(|m| m.inject_limit())
                .unwrap_or(6),
        });

        // 若本请求带 trace_id 且框架已接入 EventBus，用包裹层把工具调用事件透传为 SSE 的 tool 阶段。
        // RustToolchainAdapter 是单例，不能让它持有 trace_id，否则并发请求会互相覆盖；
        // 这里每次请求新建一个包裹层，携带本请求的 TraceContext。
        let toolchain: Option<Arc<dyn ToolchainPort>> = if trace_ctx.is_active() {
            self.toolchain.as_ref().map(|t| {
                Arc::new(TracedToolchainAdapter::new(t.clone(), trace_ctx.clone()))
                    as Arc<dyn ToolchainPort>
            })
        } else {
            self.toolchain.clone()
        };

        // 规划器走 engine_llm，同样要接事件总线，否则规划期（十几秒）前端完全静默
        let exec_ctx = DomainExecutionContext {
            ctx: domain_ctx,
            llm: domain_llm,
            engine_llm: Some(Arc::new(TracedEngineLlm::new(
                llm.clone(),
                trace_ctx.clone(),
            ))),
            repository: self.repository.clone(),
            skill_id: if skill_id.is_empty() {
                None
            } else {
                Some(skill_id.clone())
            },
            skill_params: if params.is_empty() {
                None
            } else {
                Some(params.clone())
            },
            toolchain,
            sutra_library: state.sutra_library_cloned(),
            file_system: self.file_system.clone(),
            command: self.command.clone(),
            progress_tx,
            event_publisher: event_publisher.clone(),
            // 框架级会话上下文：专家可直接读写（除 LLM 出口自动回流外，
            // 专家也能主动 `push_tool` / `set_metadata` 沉淀内容给其他消费方）
            session_context,
        };

        // 6. 根据是否有技能ID选择执行方式
        let result = if !skill_id.is_empty() {
            // 执行指定技能
            record_fn_log(
                None,
                "",
                LogLevel::Debug,
                format!("执行技能: {}，参数: {}", skill_id, params),
                None,
            );
            self.domain_expert
                .execute_skill(&skill_id, &params, exec_ctx)
                .await
        } else {
            // 执行通用逻辑
            self.domain_expert.run(exec_ctx).await
        };

        // 7. 转换结果（在适配器中手动转换领域错误为框架错误）
        match result {
            Ok(output) => Ok(output),
            Err(e) => Err(match e {
                // 前置条件是**可分类**的确定性失败，不能退化成不透明的 Any
                //（否则框架层会把「缺必需配置」当成未知错误，丢掉「重试无意义」这一判断）
                DomainError::Precondition(m) => subhuti_core::Error::Precondition(m),
                other => subhuti_core::Error::Any(anyhow::anyhow!(other)),
            }),
        }
    }
}

/// 引擎侧 LLM 包裹层（per-request，不污染全局单例）
///
/// 规划器（planner / plan_and_execute）用的是框架直接注入的 `engine_llm`，
/// 它**不走** `SubhutiLlmAdapter`，因此规划阶段（常常十几秒）此前不会发出任何事件，
/// 前端从「开始执行」到「已生成执行计划」之间一片空白，看起来像没有流式输出。
///
/// 这里按与 `TracedToolchainAdapter` 相同的模式包一层：每次 LLM 调用前发
/// `LLMCalling` 事件 → 经 ProgressEventBridge 映射为 SSE 的 `think` 阶段。
struct TracedEngineLlm {
    inner: Arc<dyn subhuti_core::LLM>,
    trace: TraceContext,
}

impl TracedEngineLlm {
    fn new(inner: Arc<dyn subhuti_core::LLM>, trace: TraceContext) -> Self {
        Self { inner, trace }
    }

    /// 发射 LLMCalling（think 阶段）：仅在接入 EventBus 且带 trace_id 时
    async fn emit_calling(&self, messages_count: usize) {
        self.trace
            .emit(DomainEvent::LlmCalling {
                messages_count,
                model: None,
            })
            .await;
    }

    /// 发射 LLMResponded（think 收尾 + token 用量）。
    ///
    /// 这是成本核算链路的源头：只有这条事件会把 `tokens` 填进 `SpanData`。
    async fn emit_responded(
        &self,
        response: &str,
        tokens_used: Option<u64>,
        started: std::time::Instant,
    ) {
        self.trace
            .emit(DomainEvent::LlmResponded {
                response: truncate_for_telemetry(response),
                tokens_used,
                duration_ms: started.elapsed().as_millis() as u64,
            })
            .await;
    }
}

#[async_trait]
impl subhuti_core::LLM for TracedEngineLlm {
    fn provider(&self) -> LLMProvider {
        self.inner.provider()
    }

    fn config(&self) -> &LLMConfig {
        self.inner.config()
    }

    async fn chat(&self, messages: Vec<subhuti_core::Message>) -> Result<String> {
        self.emit_calling(messages.len()).await;
        let started = std::time::Instant::now();
        // 走 chat_counted 以便把 provider 的 usage 透出来（默认实现退化为 chat，语义不变）
        let (content, tokens) = self.inner.chat_counted(messages).await?;
        self.emit_responded(&content, tokens, started).await;
        Ok(content)
    }

    async fn chat_with_tools(
        &self,
        messages: Vec<subhuti_core::Message>,
        tools: Vec<ToolInfo>,
    ) -> Result<LLMResponse> {
        self.emit_calling(messages.len()).await;
        let started = std::time::Instant::now();
        let resp = self.inner.chat_with_tools(messages, tools).await?;
        self.emit_responded(&resp.content, resp.total_tokens.map(|t| t as u64), started)
            .await;
        Ok(resp)
    }

    async fn chat_streaming(
        &self,
        messages: Vec<subhuti_core::Message>,
        callback: Box<dyn Fn(String) + Send>,
    ) -> Result<()> {
        self.emit_calling(messages.len()).await;
        let started = std::time::Instant::now();
        // 走带用量的流式路径；末条分片的 usage.total_tokens 会被回填。
        // 供应商若不实现 chat_streaming_counted，默认实现退化为 chat_streaming 并返回 Ok(None)，
        // 行为与之前完全一致（仅 token 上报层增加）。
        let r = self.inner.chat_streaming_counted(messages, callback).await;
        let tokens = match &r {
            Ok(t) => *t,
            Err(_) => None,
        };
        // 成功 / 失败都发一条 responded span（耗时已知）；失败时 tokens=None 仍反映真实情况。
        self.emit_responded("", tokens, started).await;
        r.map(|_| ())
    }

    async fn health_check(&self) -> Result<bool> {
        self.inner.health_check().await
    }
}

/// Subhuti LLM 适配器
///
/// 将 Subhuti 框架的 LLM 转换为领域层的 DomainLlm 接口。
///
/// 携带 trace_id / session_id（跨模块串联标准），在 `chat()` 内建立隔离 tracing span，
/// 使专家内的每次 LLM 调用日志都能与 `trace_id`、`session_id` 关联。
struct SubhutiLlmAdapter {
    llm: Arc<dyn subhuti_core::LLM>,
    /// 可观测性上下文（收敛 event_bus / trace_id / session_id），用于发射阶段事件
    trace: TraceContext,
    /// 框架级会话上下文（可选）：历史从这里读、专家记忆回流到这里。
    /// 这是「框架级上下文管理」的落点——不经过任何专家私有状态。
    session_context: Option<Arc<SessionContext>>,
    /// 本专家 ID / 名称（回流时标记产出者）
    expert_id: String,
    expert_name: String,
    /// 注入 LLM 的历史条数上限
    inject_limit: usize,
}

impl SubhutiLlmAdapter {
    /// 把**框架上下文**中的历史前插到 messages：位于 system 之后、本轮 user 之前。
    ///
    /// 与旧实现的区别：历史来源是 `SessionContext`（框架级，含专家产出的记忆），
    /// 而不是专家私有 history 或裸 SQLite——所有专家、所有消费方看的是同一份上下文。
    ///
    /// 去重：专家（如 rust_expert）可能已自行拼接 `ctx.history`（框架 Session 内存态），
    /// 内容会与上下文重叠；这里按「role + content 是否已存在」跳过，避免重复注入。
    fn with_history(&self, messages: Vec<DomainMessage>) -> Vec<DomainMessage> {
        let ctx = match &self.session_context {
            Some(c) => c,
            None => return messages,
        };

        let limit = if self.inject_limit > 0 {
            self.inject_limit
        } else {
            6
        };
        // 多取一些再裁剪：pending 里可能含本轮已由编排层写入的 user 消息
        let history = ctx.recent(limit + 4);
        if history.is_empty() {
            return messages;
        }

        // 已有消息指纹（role + content），用于去重
        let existing: std::collections::HashSet<(String, String)> = messages
            .iter()
            .map(|m| (format!("{:?}", m.role), m.content.clone()))
            .collect();

        let mut merged = Vec::with_capacity(messages.len() + history.len());
        // 1) 先把开头的 system 消息原样放在最前
        let mut rest = messages.into_iter().peekable();
        while let Some(m) = rest.peek() {
            if m.role == DomainRole::System {
                merged.push(rest.next().unwrap());
            } else {
                break;
            }
        }
        // 2) 插入历史（跳过已存在的、跳过空内容），最多注入 limit 条
        let mut injected = 0usize;
        for h in history {
            if injected >= limit {
                break;
            }
            let msg = h.to_domain_message();
            if msg.content.trim().is_empty() {
                continue;
            }
            if existing.contains(&(format!("{:?}", msg.role), msg.content.clone())) {
                continue;
            }
            merged.push(msg);
            injected += 1;
        }
        // 3) 剩余（本轮 user 等）原样追加
        merged.extend(rest);

        if injected > 0 {
            tracing::info!(
                "框架上下文注入: session={}, 来源={}, 注入 {} 条（上限 {}）",
                self.trace.session_id.as_deref().unwrap_or("-"),
                self.expert_name,
                injected,
                limit
            );
        }
        merged
    }

    /// **记忆回流**：把本轮问答写回框架级上下文，标记产出专家。
    ///
    /// 写入后，后续专家、HTTP 查询接口、藏经阁沉淀都能看到「这位专家说过什么」。
    fn sync_back_to_context(&self, question: &str, answer: &str) {
        let ctx = match &self.session_context {
            Some(c) => c,
            None => return,
        };
        if answer.trim().is_empty() {
            return;
        }
        ctx.push_expert_exchange(&self.expert_id, &self.expert_name, question, answer);
    }

    /// 从待发送消息里取最后一条 user 内容（作为回流时的「问题」侧）
    fn last_user_text(messages: &[DomainMessage]) -> String {
        messages
            .iter()
            .rev()
            .find(|m| m.role == DomainRole::User)
            .map(|m| m.content.clone())
            .unwrap_or_default()
    }
}

/// 领域消息 → 框架消息（`chat` / `chat_stream` 共用，避免两处重复转换）
/// telemetry 用的响应截断：span 里只留前 200 字符，避免整篇回答塞进链路库。
///
/// 用 `chars()` 而非字节切片，防止在多字节字符中间截断导致 panic／乱码。
fn truncate_for_telemetry(s: &str) -> String {
    const MAX_CHARS: usize = 200;
    let total = s.chars().count();
    if total <= MAX_CHARS {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_CHARS).collect();
    format!("{}…（共 {} 字）", head, total)
}

fn domain_to_framework_messages(messages: Vec<DomainMessage>) -> Vec<subhuti_core::Message> {
    messages
        .into_iter()
        .map(|m| {
            let role = match m.role {
                DomainRole::System => subhuti_core::Role::System,
                DomainRole::User => subhuti_core::Role::User,
                DomainRole::Assistant => subhuti_core::Role::Assistant,
            };
            subhuti_core::Message {
                role,
                content: m.content,
                tool_call_id: None,
            }
        })
        .collect()
}

#[async_trait]
impl DomainLlm for SubhutiLlmAdapter {
    async fn chat(&self, messages: Vec<DomainMessage>) -> DomainResult<String> {
        // 建立带 trace/session 上下文的 span，贯穿本次 LLM 调用的全部日志
        let span = tracing::info_span!(
            "domain_llm_chat",
            trace_id = %self.trace.trace_id.as_deref().unwrap_or("-"),
            session_id = %self.trace.session_id.as_deref().unwrap_or("-"),
        );
        let _enter = span.enter();

        // 发射 LLMCalling（think 阶段）：仅在接入 EventBus 且带 trace_id 时
        self.trace
            .emit(DomainEvent::LlmCalling {
                messages_count: messages.len(),
                model: None,
            })
            .await;

        // 多轮历史注入（读框架级上下文，跨进程/重启生效；已存在的内容会被去重跳过）
        let messages = self.with_history(messages);
        // 取本轮用户问题（历史插在前面，最后一条 user 即本轮输入）
        let question = Self::last_user_text(&messages);

        // 调用框架 LLM（走 chat_counted：能拿到 provider 的 usage 就报真实 token）
        let started = std::time::Instant::now();
        let response = self
            .llm
            .chat_counted(domain_to_framework_messages(messages))
            .await;

        // 转换结果（成功则把本轮问答回流进框架上下文，供后续专家与其他消费方使用）
        match response {
            Ok((output, tokens)) => {
                self.trace
                    .emit(DomainEvent::LlmResponded {
                        response: truncate_for_telemetry(&output),
                        tokens_used: tokens,
                        duration_ms: started.elapsed().as_millis() as u64,
                    })
                    .await;
                self.sync_back_to_context(&question, &output);
                Ok(output)
            }
            Err(e) => Err(DomainError::LlmError(e.to_string())),
        }
    }

    /// 真流式：透传框架 `chat_streaming` 的逐 delta 回调
    ///
    /// - 每个 delta 既转发给 `on_delta`（供领域层增量下发给前端，实现首字即出）
    /// - 又累积进 `Arc<Mutex<String>>`，流结束后作为**完整文本**返回
    async fn chat_stream(
        &self,
        messages: Vec<DomainMessage>,
        on_delta: Box<dyn Fn(String) + Send>,
    ) -> DomainResult<String> {
        let span = tracing::info_span!(
            "domain_llm_chat_stream",
            trace_id = %self.trace.trace_id.as_deref().unwrap_or("-"),
            session_id = %self.trace.session_id.as_deref().unwrap_or("-"),
        );
        let _enter = span.enter();

        // 发射 LLMCalling（think 阶段）：仅在接入 EventBus 且带 trace_id 时
        self.trace
            .emit(DomainEvent::LlmCalling {
                messages_count: messages.len(),
                model: None,
            })
            .await;

        // 多轮历史注入（与 chat 同逻辑，流式路径同样生效）
        let messages = self.with_history(messages);
        let question = Self::last_user_text(&messages);

        // callback 是 `Fn`（不可变），用 Arc<Mutex> 共享累积缓冲
        let acc = Arc::new(Mutex::new(String::new()));
        let acc_cb = acc.clone();
        let cb: Box<dyn Fn(String) + Send> = Box::new(move |delta: String| {
            if let Ok(mut s) = acc_cb.lock() {
                s.push_str(&delta);
            }
            on_delta(delta);
        });

        let started = std::time::Instant::now();
        // 走带用量的流式路径；供应商在末条分片携带的 usage.total_tokens 会被回填。
        // 若供应商未实现 chat_streaming_counted，默认实现退化为 chat_streaming 并返回 Ok(None)，
        // 行为与之前完全一致（仅多一次 token 上报）。
        let tokens = self
            .llm
            .chat_streaming_counted(domain_to_framework_messages(messages), cb)
            .await
            .map_err(|e| DomainError::LlmError(e.to_string()))?;

        let full = acc
            .lock()
            .map(|s| s.clone())
            .map_err(|_| DomainError::LlmError("流式文本累积锁被污染".to_string()))?;

        // tokens 是真实用量（流式末条分片携带）；缓存命中或 Ollama 等未实现时为 None
        self.trace
            .emit(DomainEvent::LlmResponded {
                response: truncate_for_telemetry(&full),
                tokens_used: tokens,
                duration_ms: started.elapsed().as_millis() as u64,
            })
            .await;

        // 流式同样回流：完整文本落进框架上下文
        self.sync_back_to_context(&question, &full);
        Ok(full)
    }

    fn model_name(&self) -> &str {
        self.llm.config().model.as_str()
    }
}
