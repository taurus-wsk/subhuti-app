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
        expert_id: &str,
        trace_id: &str,
        session_id: &str,
        system_prompt: &str,
        extra: &serde_json::Value,
        progress_tx: Option<crate::domain::traits::ProgressTx>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>> {
        let subhuti = self.subhuti.clone();
        let message = message.to_string();
        let user_id = user_id.to_string();
        let chain = chain.to_string();
        let expert_id = expert_id.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();
        let system_prompt = system_prompt.to_string();
        let extra = extra.clone();
        let trace_observer = self.trace_observer.clone();

        // 序列化输入用于 FnTracer
        let input_json = serde_json::json!({
            "message": &message,
            "user_id": &user_id,
            "chain": &chain,
            "trace_id": &trace_id,
            "session_id": &session_id,
            "system_prompt": &system_prompt,
            "extra": &extra,
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

            // 注入 system_prompt（前端聊天设置传入，覆盖专家默认 system prompt）
            if !system_prompt.is_empty() {
                ctx.set_metadata("system_prompt", &system_prompt);
            }

            // 摊平 extra（任意扩展参数）进 ctx.metadata：未来传什么配置都行。
            // 例如 {"workspace_folder": "/path"} 会被展开为 metadata["workspace_folder"]，
            // 领域专家（如 Rust 专家）照常从 metadata 读取，无需改领域代码。
            if let Some(obj) = extra.as_object() {
                for (k, v) in obj {
                    let v_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    ctx.set_metadata(k, &v_str);
                }
            }

            // 注入 per-request 结构化进度事件通道（取代旧版全局注册表）。
            // orchestrate_stream 创建 (p_tx, p_rx) 并把 p_tx 经这里注入 AgentContext.progress，
            // 专家与框架动作事件统一汇聚为单条 ProgressEvent 流。
            ctx.progress = progress_tx;

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
