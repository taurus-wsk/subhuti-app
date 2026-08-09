use std::sync::{Arc, LazyLock};
use subhuti_core::{
    component::{
        ComponentRegistry, ComponentState, ExecutionContext, ExpertAgentAdapter, GuardrailAdapter,
        PipelineBuilder, ValidatorAdapter,
    },
    graph::validator::JsonFormatValidator,
    guardrails::interface::{GuardrailConfig, IGuardrail, NoopGuardrail},
    memory::Memory,
    orchestrator::{AgentContext, ExpertAgent, ExpertState},
    Result,
};
use subhuti_infra::memory::DefaultMemory;

static DEFAULT_TAGS: LazyLock<Vec<String>> = LazyLock::new(|| vec!["default".to_string()]);

struct MockDefaultExpert;

#[async_trait::async_trait]
impl ExpertAgent for MockDefaultExpert {
    fn id(&self) -> &str {
        "default"
    }
    fn name(&self) -> &str {
        "通用专家"
    }
    fn tags(&self) -> &[String] {
        DEFAULT_TAGS.as_slice()
    }

    async fn run(&self, ctx: &mut AgentContext, _state: &ExpertState) -> Result<String> {
        let escaped_input = ctx.input.replace('"', "\\\"");
        Ok(format!(
            r#"{{"result": "通用专家处理: {}", "input": "{}"}}"#,
            escaped_input, escaped_input
        ))
    }
}

struct InputTransformComponent;

#[async_trait::async_trait]
impl subhuti_core::component::Component for InputTransformComponent {
    fn id(&self) -> &str {
        "input_transform"
    }
    fn name(&self) -> &str {
        "输入转换器"
    }

    async fn on_execute(&mut self, input: &str, ctx: &mut ExecutionContext) -> Result<String> {
        ctx.set("input_length", &input.len().to_string());
        Ok(input.to_string())
    }
}

struct OutputTransformComponent;

#[async_trait::async_trait]
impl subhuti_core::component::Component for OutputTransformComponent {
    fn id(&self) -> &str {
        "output_transform"
    }
    fn name(&self) -> &str {
        "输出转换器"
    }

    async fn on_execute(&mut self, input: &str, ctx: &mut ExecutionContext) -> Result<String> {
        let input_len = ctx.get("input_length").unwrap_or("0");
        Ok(format!("[最终输出] {} (原始长度: {})", input, input_len))
    }
}

#[tokio::test]
async fn test_component_workflow_with_expert() {
    println!("\n══════════════════════════════════════════════");
    println!(" 组件化工作流测试: 完整流程验证");
    println!("══════════════════════════════════════════════");

    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;
    let expert_state = ExpertState::builder(memory.clone()).build();

    let default_expert = Arc::new(MockDefaultExpert);
    let expert_adapter = ExpertAgentAdapter::new(default_expert, expert_state).into_component();

    let guardrail = Arc::new(NoopGuardrail::new(GuardrailConfig::default()));
    let guardrail_adapter = GuardrailAdapter::new(guardrail).into_component();

    let validator = Arc::new(JsonFormatValidator);
    let validator_adapter = ValidatorAdapter::new(validator).into_component();

    let input_transform = Arc::new(tokio::sync::RwLock::new(InputTransformComponent));
    let output_transform = Arc::new(tokio::sync::RwLock::new(OutputTransformComponent));

    let registry = Arc::new(ComponentRegistry::new());
    registry.register(expert_adapter.clone()).await.unwrap();
    registry.register(guardrail_adapter.clone()).await.unwrap();
    registry.register(validator_adapter.clone()).await.unwrap();
    registry.register(input_transform.clone()).await.unwrap();
    registry.register(output_transform.clone()).await.unwrap();

    println!(" 1. 组件注册完成");
    let components = registry.list().await;
    for (id, state) in components {
        println!("    - {}: {:?}", id, state);
    }

    registry.init_all().await.unwrap();
    registry.activate_all().await.unwrap();

    println!(" 2. 组件初始化激活完成");
    let components = registry.list().await;
    for (id, state) in components {
        println!("    - {}: {:?}", id, state);
    }

    let pipeline = PipelineBuilder::new()
        .input_transform(input_transform)
        .guardrail(guardrail_adapter)
        .validator(validator_adapter)
        .expert(expert_adapter)
        .output_transform(output_transform)
        .build();

    println!(" 3. Pipeline 构建完成 ({} 个组件)", pipeline.len());

    let mut ctx = ExecutionContext::new();
    let result = pipeline
        .execute(r#"{"message": "你好，这是一个测试"}"#, &mut ctx)
        .await;

    println!(" 4. 执行结果");
    match result {
        Ok(output) => {
            println!("    ✅ 成功: {}", output);
            assert!(output.contains("[最终输出]"));
            assert!(output.contains("通用专家处理"));
        }
        Err(e) => {
            println!("    ❌ 失败: {}", e);
            panic!("Pipeline 执行失败: {}", e);
        }
    }

    println!(" 5. 组件状态检查");
    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Active)
    );
    assert_eq!(
        registry.get_state("guardrail").await,
        Some(ComponentState::Active)
    );
    assert_eq!(
        registry.get_state("validator").await,
        Some(ComponentState::Active)
    );

    println!("\n ✅ 组件化工作流测试通过!");
}

#[tokio::test]
async fn test_component_lifecycle_management() {
    println!("\n══════════════════════════════════════════════");
    println!(" 组件生命周期管理测试");
    println!("══════════════════════════════════════════════");

    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;
    let expert_state = ExpertState::builder(memory.clone()).build();

    let default_expert = Arc::new(MockDefaultExpert);
    let expert_adapter = ExpertAgentAdapter::new(default_expert, expert_state).into_component();

    let registry = ComponentRegistry::new();
    registry.register(expert_adapter.clone()).await.unwrap();

    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Created)
    );
    println!(" 1. 组件创建: Created ✓");

    registry.init("default").await.unwrap();
    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Initialized)
    );
    println!(" 2. 组件初始化: Initialized ✓");

    registry.activate("default").await.unwrap();
    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Active)
    );
    println!(" 3. 组件激活: Active ✓");

    registry.deactivate("default").await;
    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Inactive)
    );
    println!(" 4. 组件停用: Inactive ✓");

    registry.activate("default").await.unwrap();
    assert_eq!(
        registry.get_state("default").await,
        Some(ComponentState::Active)
    );
    println!(" 5. 组件重新激活: Active ✓");

    registry.destroy("default").await;
    assert_eq!(registry.get_state("default").await, None);
    println!(" 6. 组件销毁: Destroyed ✓");

    println!("\n ✅ 组件生命周期测试通过!");
}

#[tokio::test]
async fn test_pipeline_partial_execution() {
    println!("\n══════════════════════════════════════════════");
    println!(" Pipeline 部分执行测试");
    println!("══════════════════════════════════════════════");

    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;
    let expert_state = ExpertState::builder(memory.clone()).build();

    let default_expert = Arc::new(MockDefaultExpert);
    let expert_adapter = ExpertAgentAdapter::new(default_expert, expert_state).into_component();

    let input_transform = Arc::new(tokio::sync::RwLock::new(InputTransformComponent));

    let pipeline_only_expert = PipelineBuilder::new()
        .expert(expert_adapter.clone())
        .build();

    let mut ctx = ExecutionContext::new();
    let result = pipeline_only_expert.execute("直接测试专家", &mut ctx).await;

    match result {
        Ok(output) => {
            println!(" 1. 仅专家执行: {}", output);
            assert!(output.contains("通用专家处理"));
        }
        Err(e) => panic!("执行失败: {}", e),
    }

    let pipeline_with_transform = PipelineBuilder::new()
        .input_transform(input_transform)
        .expert(expert_adapter)
        .build();

    let mut ctx2 = ExecutionContext::new();
    let result2 = pipeline_with_transform
        .execute("带转换测试", &mut ctx2)
        .await;

    match result2 {
        Ok(output) => {
            println!(" 2. 带转换执行: {}", output);
            assert!(output.contains("通用专家处理"));
        }
        Err(e) => panic!("执行失败: {}", e),
    }

    println!("\n ✅ Pipeline 部分执行测试通过!");
}
