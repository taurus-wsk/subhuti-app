//! # 应用服务
//!
//! 纯应用服务：只持有领域出站端口、实现 3 个入站窄端口。
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

use crate::application::ports::{ChatPort, ExpertQueryPort, SkillPort, StreamEvent};
use crate::domain::dto::{
    ExpertInfo, OrchestrateRequest, OrchestrateResponse, SkillInfo, SkillResponse,
};
use crate::domain::ports::{ExpertRepositoryPort, OrchestrationEnginePort, SkillExecutionPort};

/// 应用服务 - 纯应用层
///
/// 持有 3 个领域出站端口，实现 3 个入站窄端口（ChatPort / ExpertQueryPort / SkillPort）。
/// 不负责依赖组装（由 CompositionRoot 完成）。
pub struct AppService {
    expert_repository: Arc<dyn ExpertRepositoryPort>,
    orchestration_engine: Arc<dyn OrchestrationEnginePort>,
    skill_executor: Arc<dyn SkillExecutionPort>,
}

impl AppService {
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
        }
    }
}

// ─── ChatPort 实现（聊天/编排调度）───────────────────────────────

impl ChatPort for AppService {
    fn orchestrate(
        &self,
        request: OrchestrateRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrateResponse> + Send>> {
        let engine = self.orchestration_engine.clone();
        let user_id = request.user_id.unwrap_or_else(|| "default".to_string());
        let message = request.message;
        let chain = request.chain.unwrap_or_else(|| "".to_string());
        // trace_id / session_id 由 TraceAppService 注入 request
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();

        Box::pin(async move {
            engine
                .orchestrate(&message, &user_id, &chain, &trace_id, &session_id)
                .await
        })
    }

    fn orchestrate_stream(&self, request: OrchestrateRequest) -> mpsc::Receiver<StreamEvent> {
        let (tx, rx) = mpsc::channel(32);
        let engine = self.orchestration_engine.clone();
        let user_id = request.user_id.unwrap_or_else(|| "default".to_string());
        let message = request.message;
        let chain = request.chain.unwrap_or_default();
        let trace_id = request.trace_id.unwrap_or_default();
        let session_id = request.session_id.unwrap_or_default();

        tokio::spawn(async move {
            let _ = tx.send(StreamEvent::Start).await;
            let response = engine
                .orchestrate(&message, &user_id, &chain, &trace_id, &session_id)
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
                            "chain": response.expert_chain,
                            "duration_ms": response.duration_ms,
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

// ─── ExpertQueryPort 实现（专家查询）─────────────────────────────

impl ExpertQueryPort for AppService {
    fn list_experts(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>> {
        let repo = self.expert_repository.clone();
        Box::pin(async move { repo.get_all().await })
    }

    fn active_expert(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExpertInfo>> + Send>> {
        let repo = self.expert_repository.clone();
        Box::pin(async move { repo.active_expert().await })
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

impl SkillPort for AppService {
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
