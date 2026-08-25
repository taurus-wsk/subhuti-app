//! # Subhuti 专家仓库适配器
//!
//! 实现应用层定义的 ExpertRepositoryPort 出站端口，将 Subhuti 框架的专家管理功能转换为应用层接口。
//!
//! 六边形架构：
//! - 应用层（端口定义）← 出站适配层（适配器实现）← Subhuti 框架

use std::sync::Arc;

use subhuti_core::engine::Subhuti;

use crate::adapter::outbound::domain_expert_adapter::DomainExpertAdapter;
use crate::adapter::outbound::framework_to_app_expert;
use crate::domain::dto::ExpertInfo;
use crate::domain::ports::ExpertRepositoryPort;
use crate::domain::traits::{DomainExpert, DomainRepository};

/// Subhuti 框架的专家仓库适配器
///
/// 将 Subhuti 框架的专家管理能力适配为应用层的 ExpertRepositoryPort 接口。
pub struct SubhutiExpertRepository {
    subhuti: Arc<Subhuti>,
}

impl SubhutiExpertRepository {
    /// 创建新的适配器实例
    pub fn new(subhuti: Arc<Subhuti>) -> Self {
        Self { subhuti }
    }
}

impl ExpertRepositoryPort for SubhutiExpertRepository {
    fn get_all(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<ExpertInfo>> + Send>> {
        let subhuti = self.subhuti.clone();
        Box::pin(async move {
            subhuti
                .list_orchestrator_experts()
                .await
                .into_iter()
                .map(framework_to_app_expert)
                .collect()
        })
    }

    fn get_by_id(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<ExpertInfo>> + Send>> {
        let subhuti = self.subhuti.clone();
        let id = id.to_string();
        Box::pin(async move {
            subhuti
                .list_orchestrator_experts()
                .await
                .into_iter()
                .find(|e| e.id == id)
                .map(framework_to_app_expert)
        })
    }

    fn register(
        &self,
        expert: Arc<dyn DomainExpert>,
        repository: Arc<dyn DomainRepository>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let subhuti = self.subhuti.clone();
        // 将领域专家转换为框架专家（传递 repository）
        let framework_expert = Arc::new(DomainExpertAdapter::new(
            expert, repository, None, None, None,
        ));
        Box::pin(async move {
            subhuti.register_orchestrator_expert(framework_expert).await;
        })
    }
}
