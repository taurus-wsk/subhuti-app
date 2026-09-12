//! # Subhuti 编排引擎适配器
//!
//! 实现应用层定义的 OrchestrationEnginePort 出站端口，
//! 将 Subhuti 框架的编排能力转换为应用层接口。

use serde_json;
use std::sync::Arc;

use subhuti_core::engine::Subhuti;

use crate::adapter::outbound::framework_to_app_expert;
use crate::application::observer::{record_fn_log, FnTracer, LogLevel, TraceObserverPort};
use crate::domain::dto::{ExpertInfo, OrchestrateResponse};
use crate::domain::ports::OrchestrationEnginePort;

/// Subhuti 框架的编排引擎适配器
pub struct SubhutiOrchestrationEngine {
    subhuti: Arc<Subhuti>,
    trace_observer: Option<Arc<dyn TraceObserverPort>>,
}

impl SubhutiOrchestrationEngine {
    /// 创建新的适配器实例
    pub fn new(subhuti: Arc<Subhuti>) -> Self {
        Self {
            subhuti,
            trace_observer: None,
        }
    }

    /// 设置 trace_observer（可选，用于函数调用链路追踪）
    pub fn with_trace_observer(mut self, observer: Arc<dyn TraceObserverPort>) -> Self {
        self.trace_observer = Some(observer);
        self
    }
}

impl OrchestrationEnginePort for SubhutiOrchestrationEngine {
    fn orchestrate(
        &self,
        message: &str,
        user_id: &str,
        chain: &str,
        graph: &str,
        expert_id: &str,
        trace_id: &str,
        session_id: &str,
        workspace_folder: &str,
        system_prompt: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>> {
        let subhuti = self.subhuti.clone();
        let message = message.to_string();
        let user_id = user_id.to_string();
        let chain = chain.to_string();
        let graph = graph.to_string();
        let expert_id = expert_id.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();
        let workspace_folder = workspace_folder.to_string();
        let system_prompt = system_prompt.to_string();
        let trace_observer = self.trace_observer.clone();

        // 序列化输入用于 FnTracer
        let input_json = serde_json::json!({
            "message": &message,
            "user_id": &user_id,
            "chain": &chain,
            "graph": &graph,
            "trace_id": &trace_id,
            "session_id": &session_id,
            "workspace_folder": &workspace_folder,
            "system_prompt": &system_prompt,
        })
        .to_string();

        Box::pin(async move {
            // 记录 SubhutiOrchestrationEngine::orchestrate 函数调用
            let tracer = FnTracer::new(
                "SubhutiOrchestrationEngine::orchestrate",
                Some("OrchestrationService::orchestrate".into()), // parent
                Some(input_json),
            );

            // 函数执行日志
            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Info,
                    "接收到编排请求",
                    Some("SubhutiOrchestrationEngine::orchestrate"),
                );
            }

            let start = std::time::Instant::now();

            // 获取或创建 Session（用于会话历史持久化）
            let session = if !session_id.is_empty() {
                subhuti.get_or_create_session(&session_id).await
            } else {
                subhuti_core::runtime::session::Session::new(&user_id)
            };

            // 创建上下文并设置技能信息到 metadata
            let mut ctx =
                subhuti_core::orchestrator::AgentContext::with_session(&message, &user_id, session);

            // 如果指定了技能链，设置技能信息
            if !chain.is_empty() {
                ctx.set_metadata("skill_id", &chain);
            }

            // 如果指定了图名称，设置图信息（dispatch 时优先使用指定图）
            //
            // 过去这里写的是 `graph != "default"`，把 "default" 当作「前端未选择」
            // 静默忽略，副作用是调用方无法明确表达「别猜图」，只能听任关键词
            // 路由误判（实测把 Blender 问题判进了 rust_edit，跑了 4 分钟）。
            // 现在显式指定的图一律生效；"default" 由 Orchestrator 解释为
            // 「跳过关键词匹配，直接交给最相关的专家」。
            if !graph.is_empty() {
                ctx.set_metadata("graph_name", &graph);
            }

            // 如果指定了专家 ID，设置专家信息（dispatch 时直接路由到该专家）
            if !expert_id.is_empty() {
                ctx.set_metadata("expert_id", &expert_id);
            }

            // 注入 trace_id / session_id 供框架 emit_event 使用（事件 emit 时带 trace 上下文）
            if !trace_id.is_empty() {
                ctx.set_metadata("trace_id", &trace_id);
            }
            if !session_id.is_empty() {
                ctx.set_metadata("session_id", &session_id);
            }

            // 注入 workspace_folder / system_prompt（前端聊天设置传入，透传给专家）
            if !workspace_folder.is_empty() {
                ctx.set_metadata("workspace_folder", &workspace_folder);
            }
            if !system_prompt.is_empty() {
                ctx.set_metadata("system_prompt", &system_prompt);
            }

            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Debug,
                    format!(
                        "上下文: trace_id={}, session_id={}, chain={}",
                        &trace_id, &session_id, &chain
                    ),
                    Some("SubhutiOrchestrationEngine::orchestrate"),
                );
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    LogLevel::Info,
                    "调用 dispatch_with_context 执行编排",
                    Some("SubhutiOrchestrationEngine::orchestrate"),
                );
            }

            // 使用自定义上下文执行编排
            let result = subhuti.dispatch_with_context(ctx).await;

            let duration_ms = start.elapsed().as_millis() as u64;

            let response = OrchestrateResponse {
                success: result.success,
                output: result.output.clone(),
                chain: vec![result.strategy],
                expert_chain: result.expert_chain,
                expert_outputs: result.expert_outputs,
                duration_ms,
                error: if result.success {
                    None
                } else {
                    Some(result.output)
                },
                trace_id: trace_id.clone(),
                session_id: session_id.clone(),
            };

            // 记录函数调用出口
            if let Some(ref obs) = trace_observer {
                record_fn_log(
                    Some(obs.as_ref()),
                    &trace_id,
                    if response.success {
                        LogLevel::Info
                    } else {
                        LogLevel::Error
                    },
                    format!(
                        "编排完成, success={}, duration_ms={}",
                        response.success, duration_ms
                    ),
                    Some("SubhutiOrchestrationEngine::orchestrate"),
                );
                let output_json = serde_json::to_string(&response).ok();
                tracer.finish(
                    obs.as_ref(),
                    &trace_id,
                    output_json,
                    duration_ms,
                    Some(response.success),
                );
            }

            response
        })
    }

    fn analyze_task(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send>> {
        let subhuti = self.subhuti.clone();
        let message = message.to_string();

        Box::pin(async move { subhuti.analyze_task(&message).await })
    }

    fn match_expert(
        &self,
        message: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>> {
        let subhuti = self.subhuti.clone();
        let message = message.to_string();

        Box::pin(async move {
            // match_expert 现在是 async（要走 orchestrator 锁 + rule_engine 三层调度）
            subhuti
                .match_expert(&message)
                .await
                .into_iter()
                .map(framework_to_app_expert)
                .collect()
        })
    }
}
