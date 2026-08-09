use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    fn write(&self, item: MemoryItem) -> crate::Result<()>;
    fn read(&self, id: &str) -> Option<MemoryItem>;
    fn delete(&self, id: &str) -> crate::Result<()>;
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResult>;
    fn get_all(&self) -> Vec<MemoryItem>;
    fn clear(&mut self) -> crate::Result<()>;
}

#[async_trait]
pub trait DatabaseStore: Send + Sync {
    async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
    ) -> crate::Result<Vec<serde_json::Value>>;
    async fn execute(&self, sql: &str, params: Vec<serde_json::Value>) -> crate::Result<u64>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub short_term_capacity: usize,
    pub knowledge_dim: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            short_term_capacity: 100,
            knowledge_dim: 768,
        }
    }
}

pub trait Memory: Send + Sync {
    fn write_short_term(&self, content: &str, tags: Vec<String>);
    fn write_long_term(&self, content: &str, tags: Vec<String>);
    fn read(&self, id: &str) -> Option<MemoryItem>;
    fn delete(&self, id: &str);
    fn search(&self, query: &str, limit: usize) -> Vec<SearchResult>;
    fn get_all(&self) -> Vec<MemoryItem>;
    fn clear(&self);
}
