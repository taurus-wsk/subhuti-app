use async_trait::async_trait;
use std::any::Any;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentState {
    Created,
    Initialized,
    Active,
    Inactive,
    Destroyed,
}

#[derive(Debug, Clone)]
pub struct ComponentContext {
    pub component_id: String,
    pub properties: std::collections::HashMap<String, String>,
}

impl ComponentContext {
    pub fn new(component_id: &str) -> Self {
        Self {
            component_id: component_id.to_string(),
            properties: std::collections::HashMap::new(),
        }
    }

    pub fn with_property(mut self, key: &str, value: &str) -> Self {
        self.properties.insert(key.to_string(), value.to_string());
        self
    }
}

pub struct ExecutionContext {
    pub metadata: std::collections::HashMap<String, String>,
}

impl ExecutionContext {
    pub fn new() -> Self {
        Self {
            metadata: std::collections::HashMap::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }
}

#[async_trait]
pub trait Component: Send + Sync + 'static {
    fn id(&self) -> &str;
    fn name(&self) -> &str;

    async fn on_init(&mut self, _ctx: &ComponentContext) -> crate::Result<()> {
        Ok(())
    }

    async fn on_activate(&mut self) -> crate::Result<()> {
        Ok(())
    }

    async fn on_execute(
        &mut self,
        input: &str,
        _ctx: &mut ExecutionContext,
    ) -> crate::Result<String> {
        Ok(input.to_string())
    }

    async fn on_deactivate(&mut self) {}

    async fn on_destroy(&mut self) {}

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn state(&self) -> ComponentState {
        ComponentState::Created
    }
}

#[async_trait]
pub trait ComponentLifecycle: Component {
    fn set_state(&mut self, state: ComponentState);
}

pub type ComponentRef = Arc<dyn Component>;
pub type MutComponentRef = Arc<tokio::sync::RwLock<dyn Component>>;
