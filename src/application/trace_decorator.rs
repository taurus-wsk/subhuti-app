//! # Trace 装饰器
//!
//! 包装 OrchestrationService，为 3 个入站端口自动记录 trace + session。
//! 所有 inbound adapter（HTTP/测试/未来 gRPC）拿到的是 TraceAppService，
//! trace 自动生效，无需任何 adapter 手写 trace 逻辑。
//!
//! 替代原 HTTP 专属的 TraceSessionLayer（已删除）——trace 从协议层下沉到应用层装饰器，
//! 不再绑定 HTTP，所有端口调用自动记录。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::application::observer::{
    SessionObserverPort, SessionRecordParams, TraceHandle, TraceObserverPort,
};
use crate::application::orchestration_service::OrchestrationService;
use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort, StreamEvent};
use crate::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};

/// Trace 装饰器：包装 OrchestrationService，自动记录 trace + session
///
/// 一个结构体实现 3 个入站端口（ChatPort + ExpertQueryPort + SkillPort）：
/// - 执行类方法（orchestrate/execute_skill 及其 stream 版本）自动加 trace + session
/// - 查询类方法（list_experts/match_expert 等）直接委托，不加 trace
///
/// 组合根产出 `Arc<TraceAppService>`，clone 成 3 个 trait object 分发给各 adapter。
pub struct TraceAppService {
    inner: Arc<OrchestrationService>,
    trace_observer: Arc<dyn TraceObserverPort>,
    session_observer: Arc<dyn SessionObserverPort>,
}

impl TraceAppService {
    pub fn new(
        inner: Arc<OrchestrationService>,
        trace_observer: Arc<dyn TraceObserverPort>,
        session_observer: Arc<dyn SessionObserverPort>,
    ) -> Self {
        Self {
            inner,
            trace_observer,
            session_observer,
        }
    }
}

/// 完成 trace + 记录 session（独立函数，不依赖 self，可在 spawn task 里调用）
///
/// 对齐原 TraceSessionLayer 的记录逻辑：
/// 1. complete_success/complete_failed（含 tracing 日志）
/// 2. store_trace（持久化 trace）
/// 3. record_request（记录会话）
#[allow(clippy::too_many_arguments)]
fn finalize_trace(
    mut trace: TraceHandle,
    trace_observer: &dyn TraceObserverPort,
    session_observer: &dyn SessionObserverPort,
    user_id: &str,
    session_id: &str,
    message: &str,
    success: bool,
    output: &str,
    error: Option<&str>,
    duration_ms: u64,
    chain_name: Option<String>,
    expert_chain: Option<Vec<String>>,
) {
    // 写入链信息到 trace handle
    trace.set_chain_name(chain_name);
    trace.set_expert_chain(expert_chain);

    if success {
        trace.complete_success(output.to_string(), duration_ms);
    } else {
        trace.complete_failed(error.unwrap_or_default().to_string(), duration_ms);
    }
    let trace_id = trace.trace_id.clone();
    // 真实 token 成本：汇总本次请求所有 LLM 调用 span 的 tokens（取代硬编码 0）
    let total_tokens = trace_observer.total_tokens(&trace_id);
    trace_observer.store_trace(trace);

    session_observer.record_request(SessionRecordParams {
        session_id: session_id.to_string(),
        user_id: Some(user_id.to_string()),
        message: String::new(),
        timestamp: chrono::Utc::now(),
        trace_id,
        input: message.to_string(),
        output: if success {
            Some(output.to_string())
        } else {
            None
        },
        duration_ms: Some(duration_ms),
        matched_skill: None,
        token_usage: Some(format!(r#"{{"total_tokens": {}}}"#, total_tokens)),
        status: if success {
            "Success".to_string()
        } else {
            "Failed".to_string()
        },
    });
}

/// 生成 session ID（无 user 显式传入时用）
fn gen_session_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ─── ChatPort：执行类加 trace + session ───────────────────────────

impl ChatPort for TraceAppService {
    fn orchestrate(
        &self,
        request: OrchestrateRequest,
    ) -> Pin<Box<dyn Future<Output = OrchestrateResponse> + Send>> {
        let inner = self.inner.clone();
        let trace_observer = self.trace_observer.clone();
        let session_observer = self.session_observer.clone();
        let user_id = request
            .user_id
            .clone()
            .unwrap_or_else(|| "anonymous".into());
        let session_id = request.session_id.clone().unwrap_or_else(gen_session_id);
        let message = request.message.clone();

        Box::pin(async move {
            let trace = trace_observer.create_trace(&user_id, &session_id, &message);
            // 生成 trace_id 并注入 request，供 OrchestrationService → 出站适配器 → 框架 ctx 使用
            let trace_id = trace.trace_id.clone();
            let mut request = request;
            request.trace_id = Some(trace_id);
            request.session_id = Some(session_id.clone());

            let start = Instant::now();
            let resp = inner.orchestrate(request).await;
            let duration_ms = start.elapsed().as_millis() as u64;

            let chain_name = resp.chain.first().cloned();
            let expert_chain = if resp.expert_chain.is_empty() {
                None
            } else {
                Some(resp.expert_chain.clone())
            };

            finalize_trace(
                trace,
                &*trace_observer,
                &*session_observer,
                &user_id,
                &session_id,
                &message,
                resp.success,
                &resp.output,
                resp.error.as_deref(),
                duration_ms,
                chain_name,
                expert_chain,
            );
            resp
        })
    }

    fn orchestrate_stream(&self, request: OrchestrateRequest) -> mpsc::Receiver<StreamEvent> {
        let user_id = request
            .user_id
            .clone()
            .unwrap_or_else(|| "anonymous".into());
        let session_id = request.session_id.clone().unwrap_or_else(gen_session_id);
        let message = request.message.clone();

        // 在发送给 inner 之前生成 trace_id 并注入 request
        let trace = self
            .trace_observer
            .create_trace(&user_id, &session_id, &message);
        let trace_id = trace.trace_id.clone();
        let mut request = request;
        request.trace_id = Some(trace_id);
        request.session_id = Some(session_id.clone());

        // 原始流（应用层产出语义事件）
        let mut inner_rx = self.inner.orchestrate_stream(request);
        let (tx, rx) = mpsc::channel(32);
        let trace_observer = self.trace_observer.clone();
        let session_observer = self.session_observer.clone();

        // spawn task：转发事件 + 流结束记录 trace（用外部 pre-create 好的 trace）
        tokio::spawn(async move {
            let start = Instant::now();
            let mut final_output = String::new();
            let mut success = false;
            let mut error_msg: Option<String> = None;
            let mut expert_chain: Option<Vec<String>> = None;

            while let Some(event) = inner_rx.recv().await {
                match &event {
                    StreamEvent::Done { output, meta } => {
                        final_output = output.clone();
                        success = true;
                        // 从 meta 中提取链信息
                        if let Some(c) = meta.get("chain").and_then(|v| v.as_array()) {
                            expert_chain = Some(
                                c.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect(),
                            );
                        }
                    }
                    StreamEvent::Error { error } => {
                        error_msg = Some(error.clone());
                    }
                    _ => {}
                }
                // 透传事件给消费者（SSE/测试/gRPC）
                let _ = tx.send(event).await;
            }

            let duration_ms = start.elapsed().as_millis() as u64;
            finalize_trace(
                trace,
                &*trace_observer,
                &*session_observer,
                &user_id,
                &session_id,
                &message,
                success,
                &final_output,
                error_msg.as_deref(),
                duration_ms,
                None,
                expert_chain,
            );
        });

        rx
    }
}

// ─── SkillPort：执行类加 trace + session ──────────────────────────

impl SkillPort for TraceAppService {
    fn execute_skill(
        &self,
        skill_id: &str,
        args: &str,
        _trace_id: &str,
        _session_id: &str,
        extra: &str,
    ) -> Pin<Box<dyn Future<Output = SkillResponse> + Send>> {
        let inner = self.inner.clone();
        let trace_observer = self.trace_observer.clone();
        let session_observer = self.session_observer.clone();
        let skill_id = skill_id.to_string();
        let args = args.to_string();
        let user_id = "anonymous".to_string();
        let session_id = gen_session_id();
        let message = format!("skill={}, args={}", skill_id, args);
        let extra = extra.to_string();

        Box::pin(async move {
            let trace = trace_observer.create_trace(&user_id, &session_id, &message);
            // 装饰器自己的 trace_id：如果上游调用传了空，用自己生成的
            let decorator_trace_id = trace.trace_id.clone();
            let start = Instant::now();
            let resp = inner
                .execute_skill(&skill_id, &args, &decorator_trace_id, &session_id, &extra)
                .await;
            let duration_ms = start.elapsed().as_millis() as u64;

            finalize_trace(
                trace,
                &*trace_observer,
                &*session_observer,
                &user_id,
                &session_id,
                &message,
                resp.success,
                &resp.output,
                resp.error.as_deref(),
                duration_ms,
                None,
                None,
            );
            resp
        })
    }

    fn skill_list(&self) -> Pin<Box<dyn Future<Output = Vec<SkillInfo>> + Send>> {
        // 查询类：直接委托，不加 trace
        self.inner.skill_list()
    }
}

// ─── ExpertQueryPort：查询类全部直接委托，不加 trace ──────────────

impl ExpertQueryPort for TraceAppService {
    fn list_experts(&self) -> Pin<Box<dyn Future<Output = Vec<ExpertInfo>> + Send>> {
        self.inner.list_experts()
    }

    fn match_expert(&self, message: &str) -> Pin<Box<dyn Future<Output = Vec<ExpertInfo>> + Send>> {
        self.inner.match_expert(message)
    }

    fn analyze_task(
        &self,
        message: &str,
    ) -> Pin<Box<dyn Future<Output = serde_json::Value> + Send>> {
        self.inner.analyze_task(message)
    }
}
