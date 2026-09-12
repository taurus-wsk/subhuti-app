use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use subhuti_core::vertical::tool::{ToolCommand, ToolIntegration, ToolRegistry};
use tokio::sync::RwLock;

pub struct MemoryToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn ToolIntegration>>>,
}

impl Default for MemoryToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub fn arc() -> Arc<Self> {
        Arc::new(Self::new())
    }
}

#[async_trait]
impl ToolRegistry for MemoryToolRegistry {
    async fn register(&self, tool: Arc<dyn ToolIntegration>) -> subhuti_core::Result<()> {
        self.tools
            .write()
            .await
            .insert(tool.name().to_string(), tool);
        Ok(())
    }

    async fn unregister(&self, name: &str) -> subhuti_core::Result<()> {
        self.tools.write().await.remove(name);
        Ok(())
    }

    async fn get_tool(&self, name: &str) -> Option<Arc<dyn ToolIntegration>> {
        self.tools.read().await.get(name).cloned()
    }

    async fn list_tools(&self) -> Vec<Arc<dyn ToolIntegration>> {
        self.tools.read().await.values().cloned().collect()
    }

    async fn is_available(&self, name: &str) -> bool {
        self.tools.read().await.contains_key(name)
    }

    async fn execute(
        &self,
        tool_name: &str,
        command: ToolCommand,
    ) -> subhuti_core::Result<subhuti_core::vertical::tool::ToolResult> {
        if let Some(tool) = self.tools.read().await.get(tool_name) {
            tool.execute(command).await
        } else {
            Err(subhuti_core::Error::Tool(format!(
                "工具 {} 不存在",
                tool_name
            )))
        }
    }
}
