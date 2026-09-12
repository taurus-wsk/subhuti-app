use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use subhuti_core::vertical::workflow::{Workflow, WorkflowStore};
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct MemoryWorkflowStore {
    workflows: RwLock<HashMap<String, Workflow>>,
}

impl Default for MemoryWorkflowStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryWorkflowStore {
    pub fn new() -> Self {
        Self {
            workflows: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait]
impl WorkflowStore for MemoryWorkflowStore {
    async fn save(&self, workflow: Workflow) -> subhuti_core::Result<String> {
        let id = workflow.id.clone();
        self.workflows.write().await.insert(id.clone(), workflow);
        Ok(id)
    }

    async fn get(&self, id: &str) -> Option<Workflow> {
        self.workflows.read().await.get(id).cloned()
    }

    async fn delete(&self, id: &str) -> subhuti_core::Result<()> {
        self.workflows.write().await.remove(id);
        Ok(())
    }

    async fn list(&self) -> Vec<Workflow> {
        self.workflows.read().await.values().cloned().collect()
    }

    async fn search(&self, query: &str) -> Vec<Workflow> {
        self.workflows
            .read()
            .await
            .values()
            .filter(|w| w.name.contains(query) || w.description.contains(query))
            .cloned()
            .collect()
    }
}
