use anyhow;
use async_trait::async_trait;
use chrono::Utc;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use subhuti_core::vertical::project::{ProjectInfo, ProjectMemory, ProjectNote};
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct MemoryProjectMemory {
    projects: RwLock<HashMap<String, ProjectInfo>>,
    notes: RwLock<HashMap<String, Vec<ProjectNote>>>,
}

impl Default for MemoryProjectMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProjectMemory {
    pub fn new() -> Self {
        Self {
            projects: RwLock::new(HashMap::new()),
            notes: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait]
impl ProjectMemory for MemoryProjectMemory {
    async fn create_project(&self, name: &str, description: &str) -> subhuti_core::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let project = ProjectInfo {
            id: id.clone(),
            name: name.to_string(),
            description: description.to_string(),
            metadata: serde_json::Value::Null,
            created_at: Utc::now(),
        };
        self.projects.write().await.insert(id.clone(), project);
        Ok(id)
    }

    async fn get_project(&self, id: &str) -> Option<ProjectInfo> {
        self.projects.read().await.get(id).cloned()
    }

    async fn update_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> subhuti_core::Result<()> {
        if let Some(project) = self.projects.write().await.get_mut(id) {
            project.name = name.to_string();
            project.description = description.to_string();
            Ok(())
        } else {
            Err(subhuti_core::Error::Any(anyhow::anyhow!("项目不存在")))
        }
    }

    async fn delete_project(&self, id: &str) -> subhuti_core::Result<()> {
        self.projects.write().await.remove(id);
        self.notes.write().await.remove(id);
        Ok(())
    }

    async fn list_projects(&self) -> Vec<ProjectInfo> {
        self.projects.read().await.values().cloned().collect()
    }

    async fn add_note(
        &self,
        project_id: &str,
        content: &str,
        tags: Vec<String>,
    ) -> subhuti_core::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let note = ProjectNote {
            id: id.clone(),
            project_id: project_id.to_string(),
            content: content.to_string(),
            tags,
            created_at: Utc::now(),
        };
        self.notes
            .write()
            .await
            .entry(project_id.to_string())
            .or_default()
            .push(note);
        Ok(id)
    }

    async fn get_notes(&self, project_id: &str) -> Vec<ProjectNote> {
        self.notes
            .read()
            .await
            .get(project_id)
            .cloned()
            .unwrap_or_default()
    }

    async fn search_notes(&self, project_id: &str, query: &str) -> Vec<ProjectNote> {
        let notes = self
            .notes
            .read()
            .await
            .get(project_id)
            .cloned()
            .unwrap_or_default();
        notes
            .into_iter()
            .filter(|n| n.content.contains(query) || n.tags.iter().any(|t| t.contains(query)))
            .collect()
    }
}
