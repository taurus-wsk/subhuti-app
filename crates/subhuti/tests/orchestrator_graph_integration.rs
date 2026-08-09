//! # Orchestrator ↔ Graph 集成测试
//!
//! 验证豆包设计哲学："Orchestrator 用图做全局总控"

use std::sync::{Arc, LazyLock};
use subhuti::{
    event::{EventBus, EventHandler, EventRecorder},
    graph::{GraphBuilder, GraphState, NodeResult, Route},
};
use subhuti_core::{
    memory::Memory,
    orchestrator::{AgentContext, ExpertAgent, ExpertState, Orchestrator},
    Result,
};
use subhuti_infra::memory::DefaultMemory;

static CODE_TAGS: LazyLock<Vec<String>> = LazyLock::new(|| vec!["coding".to_string()]);
static REVIEW_TAGS: LazyLock<Vec<String>> = LazyLock::new(|| vec!["review".to_string()]);
static DEFAULT_TAGS: LazyLock<Vec<String>> = LazyLock::new(|| vec!["default".to_string()]);

struct MockCodeExpert;

#[async_trait::async_trait]
impl ExpertAgent for MockCodeExpert {
    fn id(&self) -> &str {
        "code_expert"
    }
    fn name(&self) -> &str {
        "代码专家"
    }
    fn tags(&self) -> &[String] {
        CODE_TAGS.as_slice()
    }

    async fn run(&self, ctx: &mut AgentContext, _state: &ExpertState) -> Result<String> {
        let input = &ctx.input;
        Ok(format!("代码专家处理: {}", input))
    }
}

struct MockReviewExpert;

#[async_trait::async_trait]
impl ExpertAgent for MockReviewExpert {
    fn id(&self) -> &str {
        "review_expert"
    }
    fn name(&self) -> &str {
        "评审专家"
    }
    fn tags(&self) -> &[String] {
        REVIEW_TAGS.as_slice()
    }

    async fn run(&self, ctx: &mut AgentContext, _state: &ExpertState) -> Result<String> {
        let input = &ctx.input;
        Ok(format!("评审专家处理: {}", input))
    }
}

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
        Ok(format!("通用专家处理: {}", ctx.input))
    }
}

#[tokio::test]
async fn test_orchestrator_basic_dispatch() {
    println!("\n══════════════════════════════════════════════");
    println!(" 测试 1: Orchestrator 基础分发");
    println!("══════════════════════════════════════════════");

    let _bus = Arc::new(EventBus::new(128));
    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;

    let mut orchestrator = Orchestrator::new();
    orchestrator.register_agent(Arc::new(MockCodeExpert));
    orchestrator.register_agent(Arc::new(MockReviewExpert));
    orchestrator.register_agent(Arc::new(MockDefaultExpert));

    let mut ctx = AgentContext::new("编写代码实现排序算法", "test_ctx_001");
    let state = ExpertState::builder(memory.clone()).build();
    let result = orchestrator.dispatch(&mut ctx, &state).await;

    println!("  输入: {}", ctx.input);
    println!("  匹配专家: {:?}", result.expert_chain);
    println!("  输出: {}", result.output);

    assert!(result.success);
    assert_eq!(result.expert_chain, vec!["code_expert"]);
    assert!(result.output.contains("代码专家处理"));

    let mut ctx2 = AgentContext::new("审查文档内容", "test_ctx_002");
    let result2 = orchestrator.dispatch(&mut ctx2, &state).await;

    println!("  输入: {}", ctx2.input);
    println!("  匹配专家: {:?}", result2.expert_chain);
    println!("  输出: {}", result2.output);

    assert!(result2.success);
    assert_eq!(result2.expert_chain, vec!["review_expert"]);
    assert!(result2.output.contains("评审专家处理"));

    println!("  ✅ 基础分发测试通过");
}

#[tokio::test]
async fn test_orchestrator_graph_execution() {
    println!("\n══════════════════════════════════════════════");
    println!(" 测试 2: Orchestrator + Graph 图执行");
    println!("══════════════════════════════════════════════");

    let bus = Arc::new(EventBus::new(128));
    let recorder = Arc::new(EventRecorder::new());
    bus.subscribe(recorder.clone() as Arc<dyn EventHandler>)
        .await;

    let graph = GraphBuilder::new()
        .name("code_review_flow")
        .node("code_expert", move |state| {
            let input = state.get("input").unwrap_or_default();
            async move {
                NodeResult::ok_with_state(
                    format!("代码专家处理: {}", input),
                    std::collections::HashMap::new(),
                )
            }
        })
        .node("review_expert", move |state| {
            let input = state.get("input").unwrap_or_default();
            async move {
                NodeResult::ok_with_state(
                    format!("评审专家处理: {}", input),
                    std::collections::HashMap::new(),
                )
            }
        })
        .edge("code_expert", "review_expert")
        .entry("code_expert")
        .event_bus(bus.clone())
        .build()
        .unwrap();

    let mut state = GraphState::new();
    state.set("input", "编写一个排序算法");

    let output = graph.run(state).await.unwrap();
    println!("  图执行路径: {:?}", output.execution_path);
    println!("  图输出: {}", output.output);

    assert_eq!(output.execution_path, vec!["code_expert", "review_expert"]);
    assert!(output.output.contains("评审专家处理"));

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let recording = recorder.get_recording().await;
    println!("  事件记录数: {}", recording.events.len());

    let event_types: Vec<&str> = recording
        .events
        .iter()
        .map(|e| e.data.event_type())
        .collect();
    println!("  事件类型: {:?}", event_types);
    assert!(recording
        .events
        .iter()
        .any(|e| e.data.event_type().contains("flow_started")));
    assert!(recording
        .events
        .iter()
        .any(|e| e.data.event_type().contains("flow_completed")));

    println!("  ✅ 图执行测试通过");
}

#[tokio::test]
async fn test_orchestrator_graph_conditional_routing() {
    println!("\n══════════════════════════════════════════════");
    println!(" 测试 3: Orchestrator + Graph 条件路由");
    println!("══════════════════════════════════════════════");

    let bus = Arc::new(EventBus::new(128));

    let graph = GraphBuilder::new()
        .name("task_routing")
        .node("analyzer", |mut state| async move {
            let input = state.get("input").unwrap_or_default();
            let task_type = if input.contains("评审") {
                "review"
            } else if input.contains("代码") {
                "code"
            } else {
                "default"
            };
            state.set("task_type", task_type);
            NodeResult::ok_with_state("分析完成", state.data().clone())
        })
        .node("code_expert", |_state| async {
            NodeResult::ok("代码任务处理完成")
        })
        .node("review_expert", |_state| async {
            NodeResult::ok("评审任务处理完成")
        })
        .node("default_expert", |_state| async {
            NodeResult::ok("通用任务处理完成")
        })
        .conditional_edge("analyzer", |state| {
            match state.get("task_type").as_deref() {
                Some("code") => Route::To("code_expert".to_string()),
                Some("review") => Route::To("review_expert".to_string()),
                _ => Route::To("default_expert".to_string()),
            }
        })
        .entry("analyzer")
        .event_bus(bus.clone())
        .build()
        .unwrap();

    let mut state1 = GraphState::new();
    state1.set("input", "编写代码实现排序算法");
    let output1 = graph.run(state1).await.unwrap();
    println!("  场景1 (代码任务): {}", output1.execution_path.join(" → "));
    assert_eq!(output1.execution_path, vec!["analyzer", "code_expert"]);
    assert!(output1.output.contains("代码"));

    let mut state2 = GraphState::new();
    state2.set("input", "评审这段代码");
    let output2 = graph.run(state2).await.unwrap();
    println!("  场景2 (评审任务): {}", output2.execution_path.join(" → "));
    assert_eq!(output2.execution_path, vec!["analyzer", "review_expert"]);
    assert!(output2.output.contains("评审"));

    let mut state3 = GraphState::new();
    state3.set("input", "你好");
    let output3 = graph.run(state3).await.unwrap();
    println!("  场景3 (通用任务): {}", output3.execution_path.join(" → "));
    assert_eq!(output3.execution_path, vec!["analyzer", "default_expert"]);
    assert!(output3.output.contains("通用"));

    println!("  ✅ 条件路由测试通过");
}

#[tokio::test]
async fn test_expert_node_integration() {
    println!("\n══════════════════════════════════════════════");
    println!(" 测试 4: ExpertAgent 集成到图节点");
    println!("══════════════════════════════════════════════");

    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;
    let expert_state = ExpertState::builder(memory.clone()).build();

    let code_agent = Arc::new(MockCodeExpert);
    let code_expert_state = expert_state.clone();
    let code_node = subhuti::graph::NodeFn::new(move |state| {
        let agent = code_agent.clone();
        let expert_state = code_expert_state.clone();
        async move {
            let input = state.get("input").unwrap_or_default();
            let ctx_id = uuid::Uuid::new_v4().to_string();
            let mut ctx = AgentContext::new(&input, &ctx_id);
            let result = agent.run(&mut ctx, &expert_state).await;
            match result {
                Ok(output) => NodeResult::ok(output),
                Err(e) => NodeResult::err(e.to_string()),
            }
        }
    });

    let mut state = GraphState::new();
    state.set("input", "test input");

    let result = code_node.call(state).await;
    assert!(result.success);
    assert!(result.output.contains("代码专家处理"));

    let review_agent = Arc::new(MockReviewExpert);
    let review_expert_state = expert_state.clone();
    let review_node = subhuti::graph::NodeFn::new(move |state| {
        let agent = review_agent.clone();
        let expert_state = review_expert_state.clone();
        async move {
            let input = state.get("input").unwrap_or_default();
            let ctx_id = uuid::Uuid::new_v4().to_string();
            let mut ctx = AgentContext::new(&input, &ctx_id);
            let result = agent.run(&mut ctx, &expert_state).await;
            match result {
                Ok(output) => NodeResult::ok(output),
                Err(e) => NodeResult::err(e.to_string()),
            }
        }
    });

    let mut state = GraphState::new();
    state.set("input", "review input");

    let result = review_node.call(state).await;
    assert!(result.success);
    assert!(result.output.contains("评审专家处理"));

    println!("  ✅ ExpertAgent 节点集成测试通过");
}

#[tokio::test]
async fn test_orchestrator_no_matching_expert() {
    println!("\n══════════════════════════════════════════════");
    println!(" 测试 5: 无匹配专家场景");
    println!("══════════════════════════════════════════════");

    let memory = Arc::new(DefaultMemory::new()) as Arc<dyn Memory>;

    let orchestrator = Orchestrator::new();

    let mut ctx = AgentContext::new("任意输入", "test_ctx_001");
    let state = ExpertState::builder(memory.clone()).build();
    let result = orchestrator.dispatch(&mut ctx, &state).await;

    println!("  输入: {}", ctx.input);
    println!(
        "  结果: success={}, output={}",
        result.success, result.output
    );

    assert!(!result.success);
    assert_eq!(result.output, "未找到匹配的专家");
    assert!(result.expert_chain.is_empty());

    println!("  ✅ 无匹配专家测试通过");
}
