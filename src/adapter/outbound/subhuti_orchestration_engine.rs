//! # Subhuti 编排引擎适配器
//!
//! 实现应用层定义的 OrchestrationEnginePort 出站端口，
//! 将 Subhuti 框架的编排能力转换为应用层接口。

use serde_json;
use std::sync::Arc;

use subhuti::Subhuti;

use crate::adapter::outbound::framework_to_app_expert;
use crate::domain::dto::{ExpertInfo, OrchestrateResponse};
use crate::domain::ports::OrchestrationEnginePort;

/// Subhuti 框架的编排引擎适配器
pub struct SubhutiOrchestrationEngine {
    subhuti: Arc<Subhuti>,
}

impl SubhutiOrchestrationEngine {
    /// 创建新的适配器实例
    pub fn new(subhuti: Arc<Subhuti>) -> Self {
        Self { subhuti }
    }
}

impl OrchestrationEnginePort for SubhutiOrchestrationEngine {
    fn orchestrate(
        &self,
        message: &str,
        user_id: &str,
        chain: &str,
        trace_id: &str,
        session_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>> {
        let subhuti = self.subhuti.clone();
        let message = message.to_string();
        let user_id = user_id.to_string();
        let chain = chain.to_string();
        let trace_id = trace_id.to_string();
        let session_id = session_id.to_string();

        Box::pin(async move {
            // 创建上下文并设置技能信息到 metadata
            let mut ctx = subhuti::orchestrator::AgentContext::new(&message, &user_id);

            // 如果指定了技能链，设置技能信息
            if !chain.is_empty() {
                ctx.set_metadata("skill_id", &chain);
            }

            // 注入 trace_id / session_id 供框架 emit_event 使用（事件 emit 时带 trace 上下文）
            if !trace_id.is_empty() {
                ctx.set_metadata("trace_id", &trace_id);
            }
            if !session_id.is_empty() {
                ctx.set_metadata("session_id", &session_id);
            }

            // 使用自定义上下文执行编排
            let result = subhuti.dispatch_with_context(ctx).await;

            OrchestrateResponse {
                success: result.success,
                output: result.output,
                chain: vec![result.strategy],
                expert_chain: result.expert_chain,
                expert_outputs: result.expert_outputs,
                duration_ms: 0,
                error: None,
            }
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
