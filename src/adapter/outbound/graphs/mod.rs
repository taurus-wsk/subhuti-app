//! # 图编排配置（出站适配层）
//!
//! 在六边形架构中，图编排属于出站适配层——它是框架能力的配置和适配器，
//! 将领域专家组装为框架可执行的图节点。
//!
//! ## 职责
//!
//! - 创建图节点：将专家（ExpertAgent）包装为图节点函数
//! - 定义路由规则：通过 GraphBuilder 定义工作流路径
//! - 注册图到 Orchestrator：在应用启动时注册所有工作流
//!
//! ## 设计原则
//!
//! - **图不管理技能**：技能由专家直接暴露，图只负责编排执行顺序
//! - **图不包含业务逻辑**：业务逻辑在领域专家中实现
//! - **图是框架能力的使用**：图编排是 subhuti 框架的核心能力

use std::collections::HashMap;
use std::sync::Arc;
use subhuti::{
    event::EventBus,
    graph::{Graph, GraphBuilder, GraphState, NodeFn, NodeResult, Route},
    orchestrator::{AgentContext, ExpertAgent, ExpertState},
    Result,
};

// ─── 专家节点包装器 ─────────────────────────────────────────────────

/// 将 ExpertAgent 包装为图节点函数
///
/// 实现 GraphNode 和 ExpertAgent 的桥接：
/// - GraphState 中的 "input" 字段作为专家输入
/// - 专家输出写入 GraphState 的 "output" 字段
/// - 专家标签写入 GraphState 的 "domain_tags" 字段
pub struct ExpertNodeWrapper {
    agent: Arc<dyn ExpertAgent>,
    expert_state: ExpertState,
}

impl ExpertNodeWrapper {
    pub fn new(agent: Arc<dyn ExpertAgent>, expert_state: ExpertState) -> Self {
        Self {
            agent,
            expert_state,
        }
    }

    pub fn into_node_fn(self) -> NodeFn {
        let agent = self.agent;
        let expert_state = self.expert_state;

        NodeFn::new(move |state| {
            let agent = agent.clone();
            let expert_state = expert_state.clone();

            async move {
                let input = state.get("input").unwrap_or_default();

                tracing::debug!("图节点执行专家: {} ({})", agent.id(), agent.name());

                let ctx_id = uuid::Uuid::new_v4().to_string();
                let mut ctx = AgentContext::new(&input, &ctx_id);

                let result = agent.run(&mut ctx, &expert_state).await;

                match result {
                    Ok(output) => {
                        let mut updates = HashMap::new();
                        updates.insert("output".to_string(), serde_json::json!(output));
                        updates.insert("domain_tags".to_string(), serde_json::json!(agent.tags()));
                        updates.insert("expert_id".to_string(), serde_json::json!(agent.id()));

                        NodeResult::ok_with_state(output, updates)
                    }
                    Err(e) => {
                        tracing::error!("专家执行失败: {} ({}) - {}", agent.id(), agent.name(), e);
                        NodeResult::err(format!("专家执行失败: {}", e))
                    }
                }
            }
        })
    }
}

// ─── 任务路由图构建器 ───────────────────────────────────────────────

/// 任务类型路由图构建器
///
/// 将专家注册为图节点，并定义路由规则。
pub struct TaskRouteGraphBuilder {
    builder: GraphBuilder,
    event_bus: Option<Arc<EventBus>>,
}

impl TaskRouteGraphBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            builder: GraphBuilder::new().name(name),
            event_bus: None,
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn register_expert<F, Fut>(mut self, node_name: &str, func: F) -> Self
    where
        F: Fn(GraphState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = NodeResult> + Send + 'static,
    {
        self.builder = self.builder.node(node_name, func);
        self
    }

    pub fn edge(mut self, from: &str, to: &str) -> Self {
        self.builder = self.builder.edge(from, to);
        self
    }

    pub fn conditional_edge<F>(mut self, from: &str, condition: F) -> Self
    where
        F: Fn(&GraphState) -> Route + Send + Sync + 'static,
    {
        self.builder = self.builder.conditional_edge(from, condition);
        self
    }

    pub fn entry(mut self, entry: &str) -> Self {
        self.builder = self.builder.entry(entry);
        self
    }

    pub fn max_iterations(mut self, max: usize) -> Self {
        self.builder = self.builder.max_iterations(max);
        self
    }

    pub fn reducer(mut self, key: &str, reducer: subhuti::graph::StateReducer) -> Self {
        self.builder = self.builder.reducer(key, reducer);
        self
    }

    pub fn build(self) -> Result<Graph> {
        let mut builder = self.builder;
        if let Some(bus) = self.event_bus {
            builder = builder.event_bus(bus);
        }
        Ok(builder.build()?)
    }
}

// ─── 便捷方法：直接注册专家为图节点 ─────────────────────────────────

impl TaskRouteGraphBuilder {
    /// 将 ExpertAgent 注册为图节点
    ///
    /// 自动桥接 GraphState ↔ AgentContext：
    /// - 从 GraphState 读取 "input" 作为专家输入
    /// - 专家输出写入 GraphState 的 "output" 字段
    pub fn expert_node(
        mut self,
        node_name: &str,
        agent: Arc<dyn ExpertAgent>,
        state: ExpertState,
    ) -> Self {
        let agent = agent;
        let expert_state = state;
        self.builder = self.builder.node(node_name, move |gs| {
            let agent = agent.clone();
            let st = expert_state.clone();
            async move {
                let input = gs.get("input").unwrap_or_default();
                let mut ctx = AgentContext::new(&input, &uuid::Uuid::new_v4().to_string());
                match agent.run(&mut ctx, &st).await {
                    Ok(output) => {
                        let mut updates = HashMap::new();
                        updates.insert("output".to_string(), serde_json::json!(output));
                        updates.insert("expert_id".to_string(), serde_json::json!(agent.id()));
                        NodeResult::ok_with_state(output, updates)
                    }
                    Err(e) => {
                        tracing::error!("图节点专家 {} 执行失败: {}", agent.id(), e);
                        NodeResult::err(format!("专家执行失败: {}", e))
                    }
                }
            }
        });
        self
    }
}

// ─── 图编排工厂：应用层组装入口 ─────────────────────────────────────

/// 创建所有图编排流程
///
/// 将已注册的专家组装为图节点，定义路由规则。
/// 在 `CompositionRoot::build()` 中调用，注册到 Orchestrator。
///
/// 新增工作流只需在此函数中追加图定义。
pub fn create_all_graphs(agents: &[Arc<dyn ExpertAgent>], state: &ExpertState) -> Vec<Graph> {
    let mut graphs = Vec::new();

    // 示例：Blender 工作流（单专家直连）
    if let Some(blender) = agents.iter().find(|a| a.id() == "blender") {
        match TaskRouteGraphBuilder::new("blender_workflow")
            .expert_node("blender", blender.clone(), state.clone())
            .entry("blender")
            .build()
        {
            Ok(g) => {
                tracing::info!("注册图: blender_workflow");
                graphs.push(g);
            }
            Err(e) => tracing::warn!("blender_workflow 图构建失败: {}", e),
        }
    }

    // ── 开发模板：新增工作流在此追加 ──
    //
    // match TaskRouteGraphBuilder::new("code_review")
    //     .expert_node("coder", coder_agent, state.clone())
    //     .expert_node("reviewer", reviewer_agent, state.clone())
    //     .expert_node("fixer", fixer_agent, state.clone())
    //     .edge("coder", "reviewer")
    //     .conditional_edge("reviewer", |s| {
    //         if s.get("needs_fix").unwrap_or(false) {
    //             Route::To("fixer")
    //         } else {
    //             Route::End
    //         }
    //     })
    //     .edge("fixer", "reviewer")  // 修复后重新审查
    //     .entry("coder")
    //     .build()
    // {
    //     Ok(g) => graphs.push(g),
    //     Err(e) => tracing::warn!("code_review 图构建失败: {}", e),
    // }

    graphs
}
