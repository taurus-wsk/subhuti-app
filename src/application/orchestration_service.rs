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

use crate::application::observer::{record_fn_log, FnTracer, LogLevel, TraceObserverPort};
use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort, StreamEvent};
use crate::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};
use crate::domain::ports::{ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort};

/// 编排服务 - 多专家对话编排入口
///
/// 持有 3 个领域出站端口，实现 3 个入站窄端口（ChatPort / ExpertQueryPort / SkillPort）。
/// 不负责依赖组装（由 CompositionRoot 完成）。
pub struct OrchestrationService {
    expert_repository: Arc<dyn ExpertRepositoryPort>,
    orchestration_engine: Arc<dyn OrchestrationEnginePort>,
    skill_executor: Arc<dyn SkillExecutionPort>,
    trace_observer: Option<Arc<dyn TraceObserverPort>>,
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
        }
    }

    /// 设置 trace_observer（可选，用于函数调用链路追踪）
    pub fn with_trace_observer(mut self, observer: Arc<dyn TraceObserverPort>) -> Self {
        self.trace_observer = Some(observer);
        self
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
        let chain = request.chain.unwrap_or_else(|| "".to_string());
        let graph = request.graph.unwrap_or_else(|| "".to_string());
        let expert_id = request.expert_id.unwrap_or_else(|| "".to_string());
        // trace_id / session_id 由 TraceAppService 装饰器注入 request
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();
        let workspace_folder = request.workspace_folder.unwrap_or_default();
        let system_prompt = request.system_prompt.unwrap_or_default();
        let trace_observer = self.trace_observer.clone();

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
                    &graph,
                    &expert_id,
                    &trace_id,
                    &session_id,
                    &workspace_folder,
                    &system_prompt,
                )
                .await;
            let duration_ms = start.elapsed().as_millis() as u64;

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
        let (tx, rx) = mpsc::channel(32);
        let engine = self.orchestration_engine.clone();
        let user_id = request.user_id.unwrap_or_else(|| "default".to_string());
        let message = request.message;
        let chain = request.chain.unwrap_or_default();
        let graph = request.graph.unwrap_or_default();
        let expert_id = request.expert_id.unwrap_or_default();
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();
        let workspace_folder = request.workspace_folder.unwrap_or_default();
        let system_prompt = request.system_prompt.unwrap_or_default();

        // 创建进度通道并注册到全局注册表
        let (progress_tx, mut progress_rx) = mpsc::channel::<String>(100);
        crate::adapter::outbound::domain_expert_adapter::register_progress_tx(
            &session_id,
            progress_tx,
        );

        tokio::spawn(async move {
            let _ = tx.send(StreamEvent::Start).await;

            // 思考阶段：分析任务、路由专家
            let routing_msg = if !expert_id.is_empty() {
                format!("指定专家: {}", expert_id)
            } else if !graph.is_empty() && graph != "default" {
                format!("指定图/知识库: {}", graph)
            } else {
                "分析任务中...".to_string()
            };
            let _ = tx
                .send(StreamEvent::Thought {
                    message: routing_msg,
                })
                .await;

            // 计划阶段
            let plan_msg = if !expert_id.is_empty() {
                format!("路由到专家: {}，制定执行计划", expert_id)
            } else if !graph.is_empty() && graph != "default" {
                format!("基于 {} 制定执行计划", graph)
            } else {
                "自动匹配专家和图".to_string()
            };
            let _ = tx.send(StreamEvent::Plan { message: plan_msg }).await;

            // 执行阶段
            let _ = tx
                .send(StreamEvent::Step {
                    message: "开始执行...".to_string(),
                    expert: if !expert_id.is_empty() {
                        Some(expert_id.clone())
                    } else {
                        None
                    },
                    todo_state: None,
                })
                .await;

            // 同时启动：编排执行 + 进度监听
            let response_handle = tokio::spawn({
                let engine = engine.clone();
                let message = message.clone();
                let user_id = user_id.clone();
                let chain = chain.clone();
                let graph = graph.clone();
                let expert_id = expert_id.clone();
                let trace_id = trace_id.clone();
                let session_id = session_id.clone();
                let workspace_folder = workspace_folder.clone();
                let system_prompt = system_prompt.clone();
                async move {
                    engine
                        .orchestrate(
                            &message,
                            &user_id,
                            &chain,
                            &graph,
                            &expert_id,
                            &trace_id,
                            &session_id,
                            &workspace_folder,
                            &system_prompt,
                        )
                        .await
                }
            });

            // 使用 select 同时监听进度和最终结果
            // 将 response_handle 包装在 Option 中以支持循环内多次 select
            let mut response_handle = Some(response_handle);
            let final_response = loop {
                tokio::select! {
                    // 监听进度事件
                    progress = progress_rx.recv() => {
                        if let Some(progress_str) = progress {
                            // 解析进度 JSON 并转换为 StreamEvent
                            if let Ok(progress_json) = serde_json::from_str::<serde_json::Value>(&progress_str) {
                                match progress_json.get("type").and_then(|t| t.as_str()) {
                                    Some("plan") => {
                                        if let Some(plan_msg) = progress_json.get("message").and_then(|m| m.as_str()) {
                                            let _ = tx.send(StreamEvent::Plan {
                                                message: plan_msg.to_string(),
                                            }).await;
                                        }
                                    }
                                    Some("ask") => {
                                        // 主动提问：透传给前端渲染单选卡片，ask_id 用于 /ask-resolve
                                        let ask_id = progress_json.get("ask_id").and_then(|m| m.as_str()).unwrap_or_default().to_string();
                                        let question = progress_json.get("question").and_then(|m| m.as_str()).unwrap_or_default().to_string();
                                        let options = progress_json
                                            .get("options")
                                            .and_then(|m| m.as_array())
                                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                            .unwrap_or_default();
                                        let _ = tx.send(StreamEvent::Ask {
                                            ask_id,
                                            question,
                                            options,
                                        }).await;
                                    }
                                    Some("step") => {
                                        let step_msg = progress_json.get("message").and_then(|m| m.as_str()).unwrap_or("执行中");
                                        let done_count = progress_json.get("done_count").and_then(|m| m.as_u64()).unwrap_or(0);
                                        let total_count = progress_json.get("total_count").and_then(|m| m.as_u64()).unwrap_or(0);
                                        let todo_state = progress_json
                                            .get("todo_state")
                                            .and_then(|m| m.as_str())
                                            .map(|s| s.to_string());

                                        let _ = tx.send(StreamEvent::Step {
                                            message: format!("{} ({}/{})", step_msg, done_count, total_count),
                                            expert: Some("rust-expert".to_string()),
                                            todo_state,
                                        }).await;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    // 监听最终结果
                    result = async {
                        if let Some(ref mut handle) = response_handle {
                            handle.await
                        } else {
                            // response_handle 已完成，永远等待
                            std::future::pending().await
                        }
                    } => {
                        match result {
                            Ok(response) => break response,
                            Err(e) => {
                                let _ = tx.send(StreamEvent::Error {
                                    error: format!("任务执行错误: {}", e),
                                }).await;
                                break crate::domain::dto::OrchestrateResponse {
                                    success: false,
                                    output: String::new(),
                                    chain: vec![],
                                    expert_chain: vec![],
                                    expert_outputs: vec![],
                                    duration_ms: 0,
                                    error: Some(format!("任务执行错误: {}", e)),
                                    trace_id: trace_id.clone(),
                                    session_id: session_id.clone(),
                                };
                            }
                        }
                    }
                }
            };

            // 注销进度通道
            crate::adapter::outbound::domain_expert_adapter::unregister_progress_tx(&session_id);

            // 消费剩余的进度事件（确保不丢失最后的进度更新）
            while let Ok(progress_str) = progress_rx.try_recv() {
                if let Ok(progress_json) = serde_json::from_str::<serde_json::Value>(&progress_str)
                {
                    if let Some(step_msg) = progress_json.get("message").and_then(|m| m.as_str()) {
                        let _ = tx
                            .send(StreamEvent::Step {
                                message: step_msg.to_string(),
                                expert: Some("rust-expert".to_string()),
                                todo_state: None,
                            })
                            .await;
                    }
                }
            }

            if final_response.success {
                // 发送中间步骤：专家执行完成
                if !final_response.expert_chain.is_empty() {
                    let _ = tx
                        .send(StreamEvent::Step {
                            message: format!(
                                "专家执行完成: {}",
                                final_response.expert_chain.join(" → ")
                            ),
                            expert: Some(
                                final_response
                                    .expert_chain
                                    .last()
                                    .unwrap_or(&"".to_string())
                                    .clone(),
                            ),
                            todo_state: None,
                        })
                        .await;
                }

                let _ = tx
                    .send(StreamEvent::Chunk {
                        content: final_response.output.clone(),
                    })
                    .await;
                let _ = tx
                    .send(StreamEvent::Done {
                        output: final_response.output,
                        meta: serde_json::json!({
                            "chain": final_response.expert_chain,
                            "duration_ms": final_response.duration_ms,
                        }),
                    })
                    .await;
            } else {
                let _ = tx
                    .send(StreamEvent::Error {
                        error: final_response.error.unwrap_or_default(),
                    })
                    .await;
            }
        });

        rx
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
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = SkillResponse> + Send>> {
        let executor = self.skill_executor.clone();
        let skill_id = skill_id.to_string();
        let args = args.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();
        Box::pin(async move {
            executor
                .execute_skill(&skill_id, &args, &trace_id, &session_id)
                .await
        })
    }

    fn execute_skill_stream(
        &self,
        skill_id: &str,
        args: &str,
        trace_id: &str,
        session_id: &str,
    ) -> mpsc::Receiver<StreamEvent> {
        let (tx, rx) = mpsc::channel(32);
        let executor = self.skill_executor.clone();
        let skill_id = skill_id.to_string();
        let args = args.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();

        tokio::spawn(async move {
            let _ = tx.send(StreamEvent::Start).await;
            let response = executor
                .execute_skill(&skill_id, &args, &trace_id, &session_id)
                .await;
            if response.success {
                let _ = tx
                    .send(StreamEvent::Chunk {
                        content: response.output.clone(),
                    })
                    .await;
                let _ = tx
                    .send(StreamEvent::Done {
                        output: response.output,
                        meta: serde_json::json!({
                            "skill_id": skill_id,
                            "expert_id": response.expert_id,
                        }),
                    })
                    .await;
            } else {
                let _ = tx
                    .send(StreamEvent::Error {
                        error: response.error.unwrap_or_default(),
                    })
                    .await;
            }
        });

        rx
    }
}
