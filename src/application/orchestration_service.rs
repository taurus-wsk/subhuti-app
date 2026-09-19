//! # 编排服务
//!
//! 多专家编排入口：持有领域出站端口、实现 3 个入站窄端口。
//!
//! 依赖组装由 `composition_root.rs` 负责，本模块不依赖出站适配层具体类型。
//!
//! 六边形架构：
//! - 入站端口：ChatPort / ExpertQueryPort / SkillPort（入站适配层通过这些接口调用）
//! - 出站端口（领域层定义）：通过出站适配器访问外部资源
//!   - ExpertRepositoryPort：专家管理
//!   - OrchestrationEnginePort：编排引擎
//!   - SkillExecutionPort：技能执行

use serde_json;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::application::memory_consolidation::MemoryConsolidator;
use crate::application::observer::{record_fn_log, FnTracer, LogLevel, TraceObserverPort};
use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort, StreamEvent};
use crate::application::session_manager::SessionManager;
use crate::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};
use crate::domain::ports::{ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort};
use subhuti_core::event::EventBus;
use subhuti_core::progress::ProgressEvent;

/// 编排服务 - 多专家对话编排入口
///
/// 持有 3 个领域出站端口，实现 3 个入站窄端口（ChatPort / ExpertQueryPort / SkillPort）。
/// 不负责依赖组装（由 CompositionRoot 完成）。
pub struct OrchestrationService {
    expert_repository: Arc<dyn ExpertRepositoryPort>,
    orchestration_engine: Arc<dyn OrchestrationEnginePort>,
    skill_executor: Arc<dyn SkillExecutionPort>,
    trace_observer: Option<Arc<dyn TraceObserverPort>>,
    /// 框架级会话上下文管理者（可选）：统一管理会话记忆——
    /// 编排层记录轮次、专家回流记忆，均落在框架上下文里，供任意消费方读取
    session_manager: Option<Arc<SessionManager>>,
    /// 框架事件总线（可选）：用于每次 /orchestrate 请求 per-request 订阅
    /// `ProgressEventBridge`，把框架动作事件路由到本次请求的 SSE 通道。
    /// 用 Option 而非全局注册表，避免跨请求泄漏/误投。
    event_bus: Option<Arc<EventBus>>,
    /// 记忆沉淀器（可选）：每轮对话成功后提炼事实 → 写入藏经阁 → 落库。
    /// 这是"运行中沉淀 → 下次召回"闭环的起点。
    memory_consolidator: Option<Arc<MemoryConsolidator>>,
}

impl OrchestrationService {
    /// 构造函数（由 CompositionRoot 调用）
    pub fn new(
        expert_repository: Arc<dyn ExpertRepositoryPort>,
        orchestration_engine: Arc<dyn OrchestrationEnginePort>,
        skill_executor: Arc<dyn SkillExecutionPort>,
    ) -> Self {
        Self {
            expert_repository,
            orchestration_engine,
            skill_executor,
            trace_observer: None,
            session_manager: None,
            event_bus: None,
            memory_consolidator: None,
        }
    }

    /// 设置 trace_observer（可选，用于函数调用链路追踪）
    pub fn with_trace_observer(mut self, observer: Arc<dyn TraceObserverPort>) -> Self {
        self.trace_observer = Some(observer);
        self
    }

    /// 设置框架级会话上下文管理者（可选）
    pub fn with_session_manager(mut self, manager: Arc<SessionManager>) -> Self {
        self.session_manager = Some(manager);
        self
    }

    /// 设置框架事件总线（可选，用于 per-request 进度桥订阅）
    pub fn with_event_bus(mut self, event_bus: Option<Arc<EventBus>>) -> Self {
        self.event_bus = event_bus;
        self
    }

    /// 装配记忆沉淀器（可选）：藏经阁 + LLM
    pub fn with_memory_consolidator(mut self, consolidator: Arc<MemoryConsolidator>) -> Self {
        self.memory_consolidator = Some(consolidator);
        self
    }

    /// 按专家链推断记忆领域（用于把沉淀挂到正确的领域集合）
    fn domain_of(chain: &[String]) -> String {
        chain
            .last()
            .map(|s| s.to_lowercase())
            .map(|name| {
                if name.contains("blender") {
                    "blender".to_string()
                } else if name.contains("rust") {
                    "rust".to_string()
                } else {
                    "general".to_string()
                }
            })
            .unwrap_or_else(|| "general".to_string())
    }

    /// 取会话上下文管理者（供 HTTP 查询接口等消费方复用同一份上下文）
    pub fn session_manager(&self) -> Option<Arc<SessionManager>> {
        self.session_manager.clone()
    }
}

// ─── ChatPort 实现（聊天/编排调度）───────────────────────────────

impl ChatPort for OrchestrationService {
    fn orchestrate(
        &self,
        request: OrchestrateRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>> {
        let engine = self.orchestration_engine.clone();
        let user_id = request.user_id.unwrap_or_else(|| "default".to_string());
        let message = request.message;
        let chain = request.chain.unwrap_or_default();
        let expert_id = request.expert_id.unwrap_or_default();
        // trace_id / session_id 由 TraceAppService 装饰器注入 request
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();
        let system_prompt = request.system_prompt.unwrap_or_default();
        // extra 任意扩展参数：摊平进 metadata 由出站层处理（见 SubhutiOrchestrationEngine）
        let extra = request.extra.unwrap_or(serde_json::Value::Null);
        let trace_observer = self.trace_observer.clone();
        let session_manager = self.session_manager.clone();
        let memory_consolidator = self.memory_consolidator.clone();

        // 序列化输入用于 FnTracer
        let input_json = serde_json::json!({
            "message": &message,
            "user_id": &user_id,
            "chain": &chain,
            "trace_id": &trace_id,
            "session_id": &session_id,
        })
        .to_string();

        Box::pin(async move {
            // 记录 OrchestrationService::orchestrate 函数调用（根函数）
            let tracer = FnTracer::new(
                "OrchestrationService::orchestrate",
                None, // root
                Some(input_json),
            );

            // 记录本轮用户消息到框架级上下文（后续请求/专家从同一 session_id 读回）
            if let Some(mgr) = &session_manager {
                mgr.record_user(&session_id, &message);
            }

            // 函数执行日志
            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Info,
                    "开始处理请求",
                    Some("OrchestrationService::orchestrate"),
                );
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Debug,
                    format!(
                        "请求参数: message={}, user_id={}, chain={}",
                        &message, &user_id, &chain
                    ),
                    Some("OrchestrationService::orchestrate"),
                );
            }

            let start = std::time::Instant::now();
            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Info,
                    "调用编排引擎 SubhutiOrchestrationEngine",
                    Some("OrchestrationService::orchestrate"),
                );
            }
            let resp = engine
                .orchestrate(
                    &message,
                    &user_id,
                    &chain,
                    &expert_id,
                    &trace_id,
                    &session_id,
                    &system_prompt,
                    &extra,
                    None,
                )
                .await;
            let duration_ms = start.elapsed().as_millis() as u64;

            // 记录最终答案到框架上下文（仅成功时写入，避免污染上下文）；
            // 若专家已回流同内容，自动去重。随后 flush 落盘（含专家回流内容）
            if resp.success {
                if let Some(mgr) = &session_manager {
                    mgr.record_final_answer(
                        &session_id,
                        &resp.output,
                        resp.expert_chain
                            .last()
                            .map(|s| s.as_str())
                            .unwrap_or("框架"),
                    );
                    mgr.flush(&session_id);
                }
            }

            // 记忆沉淀：把本轮值得长期记住的事实写进藏经阁并落库（失败静默）
            if resp.success {
                if let Some(mc) = &memory_consolidator {
                    let domain = Self::domain_of(&resp.expert_chain);
                    mc.consolidate(&session_id, &domain, &message, &resp.output)
                        .await;
                }
            }
            // 反馈闭环：记录本轮召回命中情况（失败也要记，否则命中率被高估）
            if let Some(mc) = &memory_consolidator {
                let domain = Self::domain_of(&resp.expert_chain);
                mc.record_execution(&message, resp.success, &domain, &session_id, &resp.output);
            }

            // 记录函数调用出口
            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    if resp.success {
                        LogLevel::Info
                    } else {
                        LogLevel::Warn
                    },
                    format!(
                        "请求处理完成, success={}, duration_ms={}",
                        resp.success, duration_ms
                    ),
                    Some("OrchestrationService::orchestrate"),
                );
                let output_json = serde_json::to_string(&resp).ok();
                tracer.finish(
                    obs.as_ref(),
                    &trace_id,
                    output_json,
                    duration_ms,
                    Some(resp.success),
                );
            }

            resp
        })
    }

    fn orchestrate_stream(&self, request: OrchestrateRequest) -> mpsc::Receiver<StreamEvent> {
        // 单条结构化进度通道：专家经 ctx.progress 直接发 ProgressEvent；
        // 框架动作事件经 ProgressEventBridge 汇入同一条流。应用层只做 ProgressEvent → StreamEvent 映射。
        let (tx, rx) = mpsc::channel(32);
        let engine = self.orchestration_engine.clone();
        let user_id = request.user_id.unwrap_or_else(|| "default".to_string());
        let message = request.message;
        let chain = request.chain.unwrap_or_default();
        let expert_id = request.expert_id.unwrap_or_default();
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();
        let system_prompt = request.system_prompt.unwrap_or_default();
        // extra 任意扩展参数：整体摊平进 metadata 由出站层处理。
        // （旧版曾从 extra 抠 "graph" 用于前端进度文案；图路由已删除，前端也不再用，此处一并移除）
        let extra_value = request.extra;
        let extra = extra_value.unwrap_or(serde_json::Value::Null);
        let session_manager = self.session_manager.clone();
        let memory_consolidator = self.memory_consolidator.clone();
        // 拷贝 event_bus 引用进构造器（self 不能整体 move 进 task）
        let event_bus = self.event_bus.clone();

        let pipeline = StreamPipeline::new(
            engine,
            session_manager,
            memory_consolidator,
            event_bus,
            message,
            user_id,
            chain,
            expert_id,
            trace_id,
            session_id,
            system_prompt,
            extra,
            tx,
        );

        tokio::spawn(async move {
            pipeline.run().await;
        });

        rx
    }
}

// ─── StreamPipeline（per-request SSE 生命周期封装）───────────────
//
// Facade：`orchestrate_stream` 只负责参数摊平与 spawn；模板方法 `run`
// 承载单次 /orchestrate 请求从 Start 到 Done/Error 的完整生命周期骨架，
// `pump` 作为汇流器合并统一 ProgressEvent 流与引擎最终响应。

/// per-request SSE 生命周期处理器
struct StreamPipeline {
    engine: Arc<dyn OrchestrationEnginePort>,
    session_manager: Option<Arc<SessionManager>>,
    memory_consolidator: Option<Arc<MemoryConsolidator>>,
    event_bus: Option<Arc<EventBus>>,
    message: String,
    user_id: String,
    chain: String,
    expert_id: String,
    trace_id: String,
    session_id: String,
    system_prompt: String,
    extra: serde_json::Value,
    tx: mpsc::Sender<StreamEvent>,
    p_tx: mpsc::Sender<ProgressEvent>,
    p_rx: mpsc::Receiver<ProgressEvent>,
}

impl StreamPipeline {
    fn new(
        engine: Arc<dyn OrchestrationEnginePort>,
        session_manager: Option<Arc<SessionManager>>,
        memory_consolidator: Option<Arc<MemoryConsolidator>>,
        event_bus: Option<Arc<EventBus>>,
        message: String,
        user_id: String,
        chain: String,
        expert_id: String,
        trace_id: String,
        session_id: String,
        system_prompt: String,
        extra: serde_json::Value,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Self {
        let (p_tx, p_rx) = mpsc::channel::<ProgressEvent>(128);
        Self {
            engine,
            session_manager,
            memory_consolidator,
            event_bus,
            message,
            user_id,
            chain,
            expert_id,
            trace_id,
            session_id,
            system_prompt,
            extra,
            tx,
            p_tx,
            p_rx,
        }
    }

    /// 模板方法：per-request 生命周期骨架（Start → 订阅 → 框架 Step → 引擎 → 汇流 → 注销 → 收尾）
    async fn run(mut self) {
        // 本请求 token 用量累计：桥在 LLMResponded 时累加，Done.meta 透出给调用方
        let token_counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let llm_calls = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

        // 生命期起点：Start
        let _ = self.tx.send(StreamEvent::Start).await;

        // per-request 订阅：框架动作事件 → 同一条 ProgressEvent 流（取代全局 stream_registry）
        let mut progress_sub_id: Option<String> = None;
        if !self.trace_id.is_empty() {
            if let Some(bus) = &self.event_bus {
                let bridge = Arc::new(
                    crate::adapter::outbound::event_bridge::ProgressEventBridge::new(
                        self.trace_id.clone(),
                        self.p_tx.clone(),
                        token_counter.clone(),
                        llm_calls.clone(),
                    ),
                );
                progress_sub_id = Some(bus.subscribe(bridge).await);
            }
        }

        // 记录本轮用户消息（框架级上下文，专家随后从这里读历史）
        if let Some(mgr) = &self.session_manager {
            mgr.record_user(&self.session_id, &self.message);
        }

        // 编排生命期步骤（analyze / run）以 ProgressEvent::Step 注入同一条流，
        // 与专家/框架事件统一经 p_rx → map_progress → StreamEvent，杜绝双通道。
        let routing_msg = if !self.expert_id.is_empty() {
            format!("指定专家: {}", self.expert_id)
        } else {
            "分析任务中...".to_string()
        };
        self.inject_step("analyze", routing_msg);
        self.inject_step("run", "开始执行...".to_string());

        // 启动编排执行（把 p_tx 经引擎注入 agent ctx.progress，专家直接发 ProgressEvent）
        let engine_handle = tokio::spawn({
            let engine = self.engine.clone();
            let message = self.message.clone();
            let user_id = self.user_id.clone();
            let chain = self.chain.clone();
            let expert_id = self.expert_id.clone();
            let trace_id = self.trace_id.clone();
            let session_id = self.session_id.clone();
            let system_prompt = self.system_prompt.clone();
            let extra = self.extra.clone();
            let p_tx = self.p_tx.clone();
            async move {
                engine
                    .orchestrate(
                        &message,
                        &user_id,
                        &chain,
                        &expert_id,
                        &trace_id,
                        &session_id,
                        &system_prompt,
                        &extra,
                        Some(p_tx),
                    )
                    .await
            }
        });

        // 单源合并：统一 ProgressEvent 流（p_rx）与引擎最终响应
        let (final_response, streamed_chunks) = self.pump(Some(engine_handle)).await;

        // 注销本次请求的进度桥订阅（per-request，非全局注册表），避免泄漏与跨请求误投
        if let (Some(bus), Some(id)) = (&self.event_bus, progress_sub_id.as_ref()) {
            bus.unsubscribe(id).await;
        }

        // EventBus 对 handler 是 tokio::spawn 派发（fire-and-forget），
        // 等 final LLMResponded 的累计落地后再读 token 计数器，避免 Done.meta 少计最后一次调用
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;

        // 消费剩余的进度事件（确保不丢失最后的进度更新）
        let mut streamed_chunks = streamed_chunks;
        while let Ok(ev) = self.p_rx.try_recv() {
            let stream_ev = Self::map_progress(ev);
            if matches!(stream_ev, StreamEvent::Chunk { .. }) {
                streamed_chunks = true;
            }
            let _ = self.tx.send(stream_ev).await;
        }

        if final_response.success {
            // 记录最终答案（专家回流过同内容则去重）+ flush 整个上下文增量
            if let Some(mgr) = &self.session_manager {
                mgr.record_final_answer(
                    &self.session_id,
                    &final_response.output,
                    final_response
                        .expert_chain
                        .last()
                        .map(|s| s.as_str())
                        .unwrap_or("框架"),
                );
                mgr.flush(&self.session_id);
            }

            // 记忆沉淀：提炼本轮事实 → 藏经阁 → 落库（失败静默，不阻塞响应）
            if let Some(mc) = &self.memory_consolidator {
                let domain = Self::domain_of(&final_response.expert_chain);
                mc.consolidate(
                    &self.session_id,
                    &domain,
                    &self.message,
                    &final_response.output,
                )
                .await;
                mc.record_execution(
                    &self.message,
                    final_response.success,
                    &domain,
                    &self.session_id,
                    &final_response.output,
                );
            }

            // 发送中间步骤：专家执行完成
            if !final_response.expert_chain.is_empty() {
                let _ = self
                    .tx
                    .send(StreamEvent::Step {
                        message: format!(
                            "专家执行完成: {}",
                            final_response.expert_chain.join(" → ")
                        ),
                        source: "框架".to_string(),
                        phase: Some("done".to_string()),
                        todo_state: None,
                    })
                    .await;
            }

            // 已经真流式下发过分片时，不再重复整块下发；
            // `Done` 仍携带完整 output，作为前端权威结果（可据此覆盖/校正）。
            if !streamed_chunks {
                let _ = self
                    .tx
                    .send(StreamEvent::Chunk {
                        content: final_response.output.clone(),
                    })
                    .await;
            }
            let _ = self
                .tx
                .send(StreamEvent::Done {
                    output: final_response.output,
                    meta: serde_json::json!({
                        "chain": final_response.expert_chain,
                        "duration_ms": final_response.duration_ms,
                        "tokens_used": token_counter.load(std::sync::atomic::Ordering::Relaxed),
                        "llm_calls": llm_calls.load(std::sync::atomic::Ordering::Relaxed),
                    }),
                })
                .await;
        } else {
            let _ = self
                .tx
                .send(StreamEvent::Error {
                    error: final_response.error.unwrap_or_default(),
                })
                .await;
        }
    }

    /// 框架动作步骤注入（analyze / run / done phase）
    fn inject_step(&self, phase: &str, msg: String) {
        let _ = self.p_tx.try_send(ProgressEvent::Step {
            message: msg,
            source: "框架".to_string(),
            phase: Some(phase.to_string()),
            todo_state: None,
            done: None,
            total: None,
        });
    }

    /// 汇流器：合并统一 ProgressEvent 流（p_rx）与引擎最终响应。
    /// 返回 `(final_response, streamed_chunks)`。Channel 关闭兜底、JoinHandle await 成功/Err、
    /// `pending()` 分支逻辑在此逐字保留。
    async fn pump(
        &mut self,
        mut engine_handle: Option<tokio::task::JoinHandle<OrchestrateResponse>>,
    ) -> (OrchestrateResponse, bool) {
        // 单源合并：统一 ProgressEvent 流（p_rx）与引擎最终响应。
        // 框架 AgentEventData 与专家 ProgressEvent 已汇入同一条流，无需多通道 select。
        let mut streamed_chunks = false;
        let final_response = loop {
            tokio::select! {
                progress = self.p_rx.recv() => {
                    match progress {
                        Some(ev) => {
                            let stream_ev = Self::map_progress(ev);
                            if matches!(stream_ev, StreamEvent::Chunk { .. }) {
                                streamed_chunks = true;
                            }
                            let _ = self.tx.send(stream_ev).await;
                        }
                        None => {
                            // 通道关闭：引擎已完成并丢弃其 p_tx。
                            // 兜底收割引擎结果（正常路径下 result 分支先命中）。
                            if let Some(h) = engine_handle.take() {
                                match h.await {
                                    Ok(r) => break r,
                                    Err(e) => {
                                        let _ = self
                                            .tx
                                            .send(StreamEvent::Error {
                                                error: format!("任务执行错误: {}", e),
                                            })
                                            .await;
                                        break Self::err_response(
                                            &self.trace_id,
                                            &self.session_id,
                                            format!("任务执行错误: {}", e),
                                        );
                                    }
                                }
                            } else {
                                break Self::err_response(
                                    &self.trace_id,
                                    &self.session_id,
                                    "进度通道异常关闭".to_string(),
                                );
                            }
                        }
                    }
                }
                result = async {
                    if let Some(ref mut h) = engine_handle {
                        h.await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    match result {
                        Ok(response) => break response,
                        Err(e) => {
                            let _ = self
                                .tx
                                .send(StreamEvent::Error {
                                    error: format!("任务执行错误: {}", e),
                                })
                                .await;
                            break Self::err_response(
                                &self.trace_id,
                                &self.session_id,
                                format!("任务执行错误: {}", e),
                            );
                        }
                    }
                }
            }
        };
        (final_response, streamed_chunks)
    }

    /// 把框架/专家统一的 ProgressEvent 映射为前端 StreamEvent（适配层塌缩到这里）
    fn map_progress(ev: ProgressEvent) -> StreamEvent {
        match ev {
            ProgressEvent::Step {
                message,
                source,
                phase,
                todo_state,
                done,
                total,
            } => {
                let message = match (done, total) {
                    (Some(d), Some(t)) if t > 0 => format!("{} ({}/{})", message, d, t),
                    _ => message,
                };
                StreamEvent::Step {
                    message,
                    source,
                    phase,
                    todo_state,
                }
            }
            ProgressEvent::Chunk { content } => StreamEvent::Chunk { content },
            ProgressEvent::Ask {
                ask_id,
                question,
                options,
            } => StreamEvent::Ask {
                ask_id,
                question,
                options,
            },
        }
    }

    /// 兜底失败响应（替换三处重复的 break 构造）
    fn err_response(trace_id: &str, session_id: &str, msg: String) -> OrchestrateResponse {
        OrchestrateResponse {
            success: false,
            output: String::new(),
            chain: vec![],
            expert_chain: vec![],
            expert_outputs: vec![],
            duration_ms: 0,
            error: Some(msg),
            trace_id: trace_id.to_string(),
            session_id: session_id.to_string(),
        }
    }

    /// 按专家链推断记忆领域（与 OrchestrationService::domain_of 语义一致，
    /// 用于把沉淀挂到正确的领域集合）
    fn domain_of(chain: &[String]) -> String {
        chain
            .last()
            .map(|s| s.to_lowercase())
            .map(|name| {
                if name.contains("blender") {
                    "blender".to_string()
                } else if name.contains("rust") {
                    "rust".to_string()
                } else {
                    "general".to_string()
                }
            })
            .unwrap_or_else(|| "general".to_string())
    }
}

// ─── ExpertQueryPort 实现（专家查询）─────────────────────────────

impl ExpertQueryPort for OrchestrationService {
    fn list_experts(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>> {
        let repo = self.expert_repository.clone();
        Box::pin(async move { repo.get_all().await })
    }

    fn match_expert(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>> {
        let engine = self.orchestration_engine.clone();
        let message = message.to_string();
        Box::pin(async move { engine.match_expert(&message).await })
    }

    fn analyze_task(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send>> {
        let engine = self.orchestration_engine.clone();
        let message = message.to_string();
        Box::pin(async move { engine.analyze_task(&message).await })
    }
}

// ─── SkillPort 实现（技能操作）───────────────────────────────────

impl SkillPort for OrchestrationService {
    fn skill_list(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<SkillInfo>> + Send>> {
        let executor = self.skill_executor.clone();
        Box::pin(async move { executor.skill_list().await })
    }

    fn execute_skill(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
        extra: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillResponse> + Send>> {
        let executor = self.skill_executor.clone();
        let skill_id = skill_id.to_string();
        let args = args.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();
        let extra = extra.to_string();
        Box::pin(async move {
            executor
                .execute_skill(&skill_id, &args, &trace_id, &session_id, &extra)
                .await
        })
    }
}
