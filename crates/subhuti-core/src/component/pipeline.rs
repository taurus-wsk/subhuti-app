use std::sync::Arc;
use tokio::sync::RwLock;

use super::lifecycle::{Component, ComponentState, ExecutionContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentSlot {
    InputTransform,
    GuardrailInput,
    GuardrailOutput,
    Validator,
    Expert,
    OutputTransform,
}

impl ComponentSlot {
    pub fn priority(&self) -> u8 {
        match self {
            ComponentSlot::InputTransform => 1,
            ComponentSlot::GuardrailInput => 2,
            ComponentSlot::Expert => 3,
            ComponentSlot::GuardrailOutput => 4,
            ComponentSlot::Validator => 5,
            ComponentSlot::OutputTransform => 6,
        }
    }
}

struct PipelineNode {
    slot: ComponentSlot,
    component: Arc<RwLock<dyn Component>>,
}

pub struct Pipeline {
    nodes: Vec<PipelineNode>,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn add_component(
        mut self,
        slot: ComponentSlot,
        component: Arc<RwLock<dyn Component>>,
    ) -> Self {
        self.nodes.push(PipelineNode { slot, component });
        self.nodes
            .sort_by(|a, b| a.slot.priority().cmp(&b.slot.priority()));
        self
    }

    pub async fn execute(&self, input: &str, ctx: &mut ExecutionContext) -> crate::Result<String> {
        let mut current_input = input.to_string();

        for node in &self.nodes {
            let mut component = node.component.write().await;

            if component.state() != ComponentState::Active {
                let comp_ctx = crate::component::lifecycle::ComponentContext::new(component.id());
                component.on_init(&comp_ctx).await?;
                component.on_activate().await?;
            }

            current_input = component.on_execute(&current_input, ctx).await?;
        }

        Ok(current_input)
    }

    pub async fn init_all(&self) -> crate::Result<()> {
        for node in &self.nodes {
            let mut component = node.component.write().await;
            let id = component.id().to_string();
            let comp_ctx = crate::component::lifecycle::ComponentContext::new(&id);
            component.on_init(&comp_ctx).await?;
        }
        Ok(())
    }

    pub async fn activate_all(&self) -> crate::Result<()> {
        for node in &self.nodes {
            let mut component = node.component.write().await;
            component.on_activate().await?;
        }
        Ok(())
    }

    pub async fn deactivate_all(&self) {
        for node in &self.nodes {
            let mut component = node.component.write().await;
            component.on_deactivate().await;
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

pub struct PipelineBuilder {
    pipeline: Pipeline,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self {
            pipeline: Pipeline::new(),
        }
    }

    pub fn input_transform(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::InputTransform, component);
        self
    }

    pub fn guardrail_input(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::GuardrailInput, component);
        self
    }

    pub fn guardrail_output(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::GuardrailOutput, component);
        self
    }

    pub fn validator(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::Validator, component);
        self
    }

    pub fn expert(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::Expert, component);
        self
    }

    pub fn output_transform(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::OutputTransform, component);
        self
    }

    pub fn guardrail(mut self, component: Arc<RwLock<dyn Component>>) -> Self {
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::GuardrailInput, component.clone());
        self.pipeline = self
            .pipeline
            .add_component(ComponentSlot::GuardrailOutput, component);
        self
    }

    pub fn build(self) -> Pipeline {
        self.pipeline
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
