use std::any::Any;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::lifecycle::{Component, ComponentContext, ComponentState, ExecutionContext};
use super::pipeline::Pipeline;
use crate::graph::validator::INodeValidator;
use crate::graph::{node::NodeFn, node::NodeResult};
use crate::guardrails::interface::IGuardrail;
use crate::orchestrator::{AgentContext, ExpertAgent, ExpertState};

pub struct ExpertAgentAdapter {
    agent: Arc<dyn ExpertAgent>,
    state: ComponentState,
    expert_state: ExpertState,
}

impl ExpertAgentAdapter {
    pub fn new(agent: Arc<dyn ExpertAgent>, expert_state: ExpertState) -> Self {
        Self {
            agent,
            state: ComponentState::Created,
            expert_state,
        }
    }

    pub fn into_component(self) -> Arc<RwLock<dyn Component>> {
        Arc::new(RwLock::new(self))
    }
}

#[async_trait::async_trait]
impl Component for ExpertAgentAdapter {
    fn id(&self) -> &str {
        self.agent.id()
    }

    fn name(&self) -> &str {
        self.agent.name()
    }

    async fn on_init(&mut self, _ctx: &ComponentContext) -> crate::Result<()> {
        self.state = ComponentState::Initialized;
        Ok(())
    }

    async fn on_activate(&mut self) -> crate::Result<()> {
        self.state = ComponentState::Active;
        Ok(())
    }

    async fn on_execute(
        &mut self,
        input: &str,
        _ctx: &mut ExecutionContext,
    ) -> crate::Result<String> {
        let mut agent_ctx = AgentContext::new(input, self.id());
        self.agent.run(&mut agent_ctx, &self.expert_state).await
    }

    async fn on_deactivate(&mut self) {
        self.state = ComponentState::Inactive;
    }

    async fn on_destroy(&mut self) {
        self.state = ComponentState::Destroyed;
    }

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn state(&self) -> ComponentState {
        self.state
    }
}

pub struct GuardrailAdapter {
    guardrail: Arc<dyn IGuardrail>,
    state: ComponentState,
}

impl GuardrailAdapter {
    pub fn new(guardrail: Arc<dyn IGuardrail>) -> Self {
        Self {
            guardrail,
            state: ComponentState::Created,
        }
    }

    pub fn into_component(self) -> Arc<RwLock<dyn Component>> {
        Arc::new(RwLock::new(self))
    }
}

#[async_trait::async_trait]
impl Component for GuardrailAdapter {
    fn id(&self) -> &str {
        "guardrail"
    }

    fn name(&self) -> &str {
        "Guardrail"
    }

    async fn on_init(&mut self, _ctx: &ComponentContext) -> crate::Result<()> {
        self.state = ComponentState::Initialized;
        Ok(())
    }

    async fn on_activate(&mut self) -> crate::Result<()> {
        self.state = ComponentState::Active;
        Ok(())
    }

    async fn on_execute(
        &mut self,
        input: &str,
        _ctx: &mut ExecutionContext,
    ) -> crate::Result<String> {
        let input_result = self.guardrail.check_input(input).await;
        if !input_result.allowed {
            return Err(crate::Error::Runtime(format!(
                "护栏拦截: {}",
                input_result.reason
            )));
        }

        let output = if let Some(sanitized) = input_result.sanitized_input {
            sanitized
        } else {
            input.to_string()
        };

        let output_result = self.guardrail.check_output(&output).await;
        if !output_result.allowed {
            return Err(crate::Error::Runtime(format!(
                "护栏拦截: {}",
                output_result.reason
            )));
        }

        Ok(output_result.sanitized_output.unwrap_or(output))
    }

    async fn on_deactivate(&mut self) {
        self.state = ComponentState::Inactive;
    }

    async fn on_destroy(&mut self) {
        self.state = ComponentState::Destroyed;
    }

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn state(&self) -> ComponentState {
        self.state
    }
}

pub struct ValidatorAdapter {
    validator: Arc<dyn INodeValidator>,
    state: ComponentState,
}

impl ValidatorAdapter {
    pub fn new(validator: Arc<dyn INodeValidator>) -> Self {
        Self {
            validator,
            state: ComponentState::Created,
        }
    }

    pub fn into_component(self) -> Arc<RwLock<dyn Component>> {
        Arc::new(RwLock::new(self))
    }
}

#[async_trait::async_trait]
impl Component for ValidatorAdapter {
    fn id(&self) -> &str {
        "validator"
    }

    fn name(&self) -> &str {
        "Validator"
    }

    async fn on_init(&mut self, _ctx: &ComponentContext) -> crate::Result<()> {
        self.state = ComponentState::Initialized;
        Ok(())
    }

    async fn on_activate(&mut self) -> crate::Result<()> {
        self.state = ComponentState::Active;
        Ok(())
    }

    async fn on_execute(
        &mut self,
        input: &str,
        _ctx: &mut ExecutionContext,
    ) -> crate::Result<String> {
        let result = NodeResult::ok(input);
        let graph_state = crate::graph::state::GraphState::new();

        match self.validator.validate(&graph_state, &result).await {
            Ok(_) => Ok(input.to_string()),
            Err(e) => Err(crate::Error::Runtime(format!("校验失败: {}", e))),
        }
    }

    async fn on_deactivate(&mut self) {
        self.state = ComponentState::Inactive;
    }

    async fn on_destroy(&mut self) {
        self.state = ComponentState::Destroyed;
    }

    fn as_any(&self) -> &dyn Any
    where
        Self: Sized,
    {
        self
    }

    fn state(&self) -> ComponentState {
        self.state
    }
}

pub struct GraphNodeComponentAdapter {
    pipeline: Pipeline,
}

impl GraphNodeComponentAdapter {
    pub fn new(pipeline: Pipeline) -> Self {
        Self { pipeline }
    }

    pub fn into_node_fn(self) -> NodeFn {
        let pipeline = Arc::new(self.pipeline);
        NodeFn::new(move |state| {
            let pipeline = pipeline.clone();
            async move {
                let input = state.get("input").unwrap_or_default();
                let mut ctx = ExecutionContext::new();

                match pipeline.execute(&input, &mut ctx).await {
                    Ok(output) => {
                        let mut updates = std::collections::HashMap::new();
                        for (key, value) in ctx.metadata {
                            updates.insert(key, serde_json::Value::String(value));
                        }
                        NodeResult::ok_with_state(output, updates)
                    }
                    Err(e) => NodeResult::err(e.to_string()),
                }
            }
        })
    }

    pub async fn init(&self) -> crate::Result<()> {
        self.pipeline.init_all().await
    }

    pub async fn activate(&self) -> crate::Result<()> {
        self.pipeline.activate_all().await
    }

    pub async fn deactivate(&self) {
        self.pipeline.deactivate_all().await
    }
}
