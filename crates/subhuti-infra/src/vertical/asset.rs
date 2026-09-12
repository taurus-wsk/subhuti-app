use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use subhuti_core::vertical::asset::{Asset, AssetLibrary};
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct MemoryAssetLibrary {
    assets: RwLock<HashMap<String, Asset>>,
}

impl Default for MemoryAssetLibrary {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryAssetLibrary {
    pub fn new() -> Self {
        Self {
            assets: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait]
impl AssetLibrary for MemoryAssetLibrary {
    async fn save(&self, asset: Asset) -> subhuti_core::Result<String> {
        let id = asset.id.clone();
        self.assets.write().await.insert(id.clone(), asset);
        Ok(id)
    }

    async fn get(&self, id: &str) -> Option<Asset> {
        self.assets.read().await.get(id).cloned()
    }

    async fn delete(&self, id: &str) -> subhuti_core::Result<()> {
        self.assets.write().await.remove(id);
        Ok(())
    }

    async fn list(&self, type_: Option<&str>) -> Vec<Asset> {
        let assets = self.assets.read().await;
        assets
            .values()
            .filter(|a| type_.map(|t| a.type_ == t).unwrap_or(true))
            .cloned()
            .collect()
    }

    async fn search(&self, query: &str) -> Vec<Asset> {
        let assets = self.assets.read().await;
        assets
            .values()
            .filter(|a| a.name.contains(query) || a.path.contains(query))
            .cloned()
            .collect()
    }
}
