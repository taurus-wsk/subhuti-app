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
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use subhuti_core::event::{AgentEventData, EventBus};
use subhuti_core::orchestrator::{
    AgentContext, ExpertAgent, ExpertState, FromState, Llm, SkillInfo,
};
use subhuti_core::Result;

use crate::adapter::outbound::rust_toolchain_adapter::TracedToolchainAdapter;
use crate::application::observer::{record_fn_log, LogLevel};
use crate::domain::ports::CommandPort;
use crate::domain::ports::FileSystemPort;
use crate::domain::ports::ToolchainPort;
use crate::domain::traits::{
    DomainContext, DomainError, DomainExecutionContext, DomainExpert, DomainLlm, DomainMessage,
    DomainRepository, DomainResult, DomainRole,
};

// ─── 全局进度通道注册表 ──────────────────────────────────────────
//
// 用于在请求级别将 progress_tx 传递给 DomainExpertAdapter，
// 避免修改 Orchestrator 接口。key 为 session_id。

use std::sync::OnceLock;

static PROGRESS_TX_REGISTRY: OnceLock<Mutex<HashMap<String, mpsc::Sender<String>>>> =
    OnceLock::new();

fn get_registry() -> &'static Mutex<HashMap<String, mpsc::Sender<String>>> {
    PROGRESS_TX_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 注册进度通道（在请求开始时调用）
pub fn register_progress_tx(session_id: &str, tx: mpsc::Sender<String>) {
    if let Ok(mut map) = get_registry().lock() {
        map.insert(session_id.to_string(), tx);
    }
}

/// 注销进度通道（在请求结束时调用）
pub fn unregister_progress_tx(session_id: &str) {
    if let Ok(mut map) = get_registry().lock() {
        map.remove(session_id);
    }
}

/// 获取进度通道（在 DomainExpertAdapter::dispatch 中调用）
fn get_progress_tx(session_id: &str) -> Option<mpsc::Sender<String>> {
    get_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(session_id).cloned())
}

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
    /// 进度报告通道（可选，用于实时推送执行进度）
    progress_tx: Option<mpsc::Sender<String>>,
    /// 框架事件总线（可选，用于把 LLM 调用等动作事件发到 EventBus，
    /// 由 ProgressEventBridge 桥接成 SSE 阶段流）
    event_bus: Option<Arc<EventBus>>,
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
            progress_tx: None,
            event_bus,
        }
    }

    /// 设置进度报告通道（用于实时推送执行进度到 SSE 流）
    pub fn with_progress_tx(mut self, tx: mpsc::Sender<String>) -> Self {
        self.progress_tx = Some(tx);
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
        let domain_llm = Arc::new(SubhutiLlmAdapter {
            llm: llm.clone(),
            event_bus: self.event_bus.clone(),
            trace_id: ctx
                .metadata
                .get("trace_id")
                .cloned()
                .filter(|s| !s.is_empty()),
            session_id: Some(ctx.session.id().to_string()),
        });

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
        // 从全局注册表获取 progress_tx（按 session_id 查找）
        let session_id = ctx.session.id().to_string();
        let progress_tx = get_progress_tx(&session_id).or_else(|| self.progress_tx.clone());
        // 若本请求带 trace_id 且框架已接入 EventBus，用包裹层把工具调用事件透传为 SSE 的 tool 阶段。
        // RustToolchainAdapter 是单例，不能让它持有 trace_id，否则并发请求会互相覆盖；
        // 这里每次请求新建一个包裹层，携带本请求的 trace_id / session_id（与 SubhutiLlmAdapter 同模式）。
        let toolchain: Option<Arc<dyn ToolchainPort>> =
            match (&self.event_bus, &domain_ctx.trace_id) {
                (Some(bus), Some(tid)) if !tid.is_empty() => self.toolchain.as_ref().map(|t| {
                    Arc::new(TracedToolchainAdapter::new(
                        t.clone(),
                        Some(bus.clone()),
                        Some(tid.clone()),
                        Some(session_id.clone()),
                    )) as Arc<dyn ToolchainPort>
                }),
                _ => self.toolchain.clone(),
            };

        let exec_ctx = DomainExecutionContext {
            ctx: domain_ctx,
            llm: domain_llm,
            engine_llm: Some(llm.clone()),
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
            event_bus: self.event_bus.clone(),
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
            Err(e) => Err(subhuti_core::Error::Any(anyhow::anyhow!(e))),
        }
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
    event_bus: Option<Arc<EventBus>>,
    trace_id: Option<String>,
    session_id: Option<String>,
}

/// 领域消息 → 框架消息（`chat` / `chat_stream` 共用，避免两处重复转换）
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
            trace_id = %self.trace_id.as_deref().unwrap_or("-"),
            session_id = %self.session_id.as_deref().unwrap_or("-"),
        );
        let _enter = span.enter();

        // 发射 LLMCalling（think 阶段）：仅在带 trace_id 时，避免无关联噪声
        if let (Some(bus), Some(tid)) = (&self.event_bus, &self.trace_id) {
            if !tid.is_empty() {
                bus.emit_with_trace(
                    AgentEventData::LLMCalling {
                        messages_count: messages.len(),
                        model: None,
                    },
                    tid.clone(),
                    self.session_id.clone(),
                )
                .await;
            }
        }

        // 调用框架 LLM
        let response = self.llm.chat(domain_to_framework_messages(messages)).await;

        // 转换结果
        match response {
            Ok(output) => Ok(output),
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
            trace_id = %self.trace_id.as_deref().unwrap_or("-"),
            session_id = %self.session_id.as_deref().unwrap_or("-"),
        );
        let _enter = span.enter();

        // 发射 LLMCalling（think 阶段）：仅在带 trace_id 时，避免无关联噪声
        if let (Some(bus), Some(tid)) = (&self.event_bus, &self.trace_id) {
            if !tid.is_empty() {
                bus.emit_with_trace(
                    AgentEventData::LLMCalling {
                        messages_count: messages.len(),
                        model: None,
                    },
                    tid.clone(),
                    self.session_id.clone(),
                )
                .await;
            }
        }

        // callback 是 `Fn`（不可变），用 Arc<Mutex> 共享累积缓冲
        let acc = Arc::new(Mutex::new(String::new()));
        let acc_cb = acc.clone();
        let cb: Box<dyn Fn(String) + Send> = Box::new(move |delta: String| {
            if let Ok(mut s) = acc_cb.lock() {
                s.push_str(&delta);
            }
            on_delta(delta);
        });

        self.llm
            .chat_streaming(domain_to_framework_messages(messages), cb)
            .await
            .map_err(|e| DomainError::LlmError(e.to_string()))?;

        let full = acc
            .lock()
            .map(|s| s.clone())
            .map_err(|_| DomainError::LlmError("流式文本累积锁被污染".to_string()))?;
        Ok(full)
    }

    fn model_name(&self) -> &str {
        self.llm.config().model.as_str()
    }
}
