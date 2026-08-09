//! # Asset Library Interface
//!
//! 资产库接口定义。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub name: String,
    pub type_: String,
    pub path: String,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[async_trait]
pub trait AssetLibrary: Send + Sync {
    async fn save(&self, asset: Asset) -> crate::Result<String>;
    async fn get(&self, id: &str) -> Option<Asset>;
    async fn delete(&self, id: &str) -> crate::Result<()>;
    async fn list(&self, type_: Option<&str>) -> Vec<Asset>;
    async fn search(&self, query: &str) -> Vec<Asset>;
}
