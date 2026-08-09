use std::sync::Arc;
use tokio::sync::RwLock;

use super::lifecycle::{Component, ComponentContext, ComponentState};

struct ComponentEntry {
    component: Arc<RwLock<dyn Component>>,
    state: ComponentState,
    context: ComponentContext,
}

pub struct ComponentRegistry {
    components: RwLock<std::collections::HashMap<String, ComponentEntry>>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            components: RwLock::new(std::collections::HashMap::new()),
        }
    }

    pub async fn register(&self, component: Arc<RwLock<dyn Component>>) -> crate::Result<()> {
        let id = component.read().await.id().to_string();
        let entry = ComponentEntry {
            component: component.clone(),
            state: ComponentState::Created,
            context: ComponentContext::new(&id),
        };
        self.components.write().await.insert(id, entry);
        Ok(())
    }

    pub async fn init(&self, component_id: &str) -> crate::Result<()> {
        let mut components = self.components.write().await;
        if let Some(entry) = components.get_mut(component_id) {
            let ctx = &entry.context;
            entry.component.write().await.on_init(ctx).await?;
            entry.state = ComponentState::Initialized;
        } else {
            return Err(crate::Error::Runtime("component not found".to_string()));
        }
        Ok(())
    }

    pub async fn activate(&self, component_id: &str) -> crate::Result<()> {
        let mut components = self.components.write().await;
        if let Some(entry) = components.get_mut(component_id) {
            if entry.state != ComponentState::Initialized && entry.state != ComponentState::Inactive
            {
                return Err(crate::Error::Runtime(
                    "component not initialized".to_string(),
                ));
            }
            entry.component.write().await.on_activate().await?;
            entry.state = ComponentState::Active;
        } else {
            return Err(crate::Error::Runtime("component not found".to_string()));
        }
        Ok(())
    }

    pub async fn deactivate(&self, component_id: &str) {
        let mut components = self.components.write().await;
        if let Some(entry) = components.get_mut(component_id) {
            entry.component.write().await.on_deactivate().await;
            entry.state = ComponentState::Inactive;
        }
    }

    pub async fn destroy(&self, component_id: &str) {
        let mut components = self.components.write().await;
        if let Some(entry) = components.remove(component_id) {
            entry.component.write().await.on_destroy().await;
        }
    }

    pub async fn get(&self, component_id: &str) -> Option<Arc<RwLock<dyn Component>>> {
        let components = self.components.read().await;
        components
            .get(component_id)
            .map(|entry| entry.component.clone())
    }

    pub async fn get_state(&self, component_id: &str) -> Option<ComponentState> {
        let components = self.components.read().await;
        components.get(component_id).map(|entry| entry.state)
    }

    pub async fn list(&self) -> Vec<(String, ComponentState)> {
        let components = self.components.read().await;
        components
            .iter()
            .map(|(id, entry)| (id.clone(), entry.state))
            .collect()
    }

    pub async fn init_all(&self) -> crate::Result<()> {
        let mut components = self.components.write().await;
        for entry in components.values_mut() {
            let ctx = &entry.context;
            entry.component.write().await.on_init(ctx).await?;
            entry.state = ComponentState::Initialized;
        }
        Ok(())
    }

    pub async fn activate_all(&self) -> crate::Result<()> {
        let mut components = self.components.write().await;
        for entry in components.values_mut() {
            if entry.state == ComponentState::Initialized {
                entry.component.write().await.on_activate().await?;
                entry.state = ComponentState::Active;
            }
        }
        Ok(())
    }

    pub async fn deactivate_all(&self) {
        let mut components = self.components.write().await;
        for entry in components.values_mut() {
            if entry.state == ComponentState::Active {
                entry.component.write().await.on_deactivate().await;
                entry.state = ComponentState::Inactive;
            }
        }
    }

    pub async fn destroy_all(&self) {
        let components = self.components.write().await;
        for entry in components.values() {
            entry.component.write().await.on_destroy().await;
        }
    }
}
