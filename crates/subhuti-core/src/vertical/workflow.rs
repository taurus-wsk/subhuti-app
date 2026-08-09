//! # Workflow Interface
//!
//! 工作流接口定义。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub definition: serde_json::Value,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait WorkflowStore: Send + Sync {
    async fn save(&self, workflow: Workflow) -> crate::Result<String>;
    async fn get(&self, id: &str) -> Option<Workflow>;
    async fn delete(&self, id: &str) -> crate::Result<()>;
    async fn list(&self) -> Vec<Workflow>;
    async fn search(&self, query: &str) -> Vec<Workflow>;
}
