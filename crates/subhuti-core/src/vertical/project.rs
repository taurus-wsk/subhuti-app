//! # Project Memory Interface
//!
//! 项目记忆接口定义。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectNote {
    pub id: String,
    pub project_id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait ProjectMemory: Send + Sync {
    async fn create_project(&self, name: &str, description: &str) -> crate::Result<String>;
    async fn get_project(&self, id: &str) -> Option<ProjectInfo>;
    async fn update_project(&self, id: &str, name: &str, description: &str) -> crate::Result<()>;
    async fn delete_project(&self, id: &str) -> crate::Result<()>;
    async fn list_projects(&self) -> Vec<ProjectInfo>;
    async fn add_note(
        &self,
        project_id: &str,
        content: &str,
        tags: Vec<String>,
    ) -> crate::Result<String>;
    async fn get_notes(&self, project_id: &str) -> Vec<ProjectNote>;
    async fn search_notes(&self, project_id: &str, query: &str) -> Vec<ProjectNote>;
}
