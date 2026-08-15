//! # 图编排配置（出站适配层）
//!
//! ## 竞标制架构（新）
//!
//! 图节点不再预绑定专家。节点只定义任务标签（task_tags），
//! 全局 Actor 池中的演员通过竞标制竞争节点：
//!
//! ```text
//! 图节点需要执行
//!   → 调度器获取节点标签（如 ["coding", "rust"]）
//!   → 从 ActorRegistry 竞标：所有 Actor 自评分数
//!   → 最高分者中标，上台执行
//! ```
//!
//! ## 设计原则
//!
//! - **图不管理专家**：专家注册时自动成为 Actor，图只定义任务要求
//! - **双向奔赴**：节点定义"需要什么"，Actor 自评"我能做什么"
//! - **图是流程骨架**：定义节点顺序和路由，不关心谁执行

use std::sync::Arc;
use subhuti_core::event::EventBus;
use subhuti_core::graph::{Graph, GraphBuilder, NodeResult, Route};
use subhuti_core::Result;

mod rust_edit;
mod rust_programming;

// ─── 图构建器 ─────────────────────────────────────────────────────

/// 图构建器（简化版，无需 ExpertAgent 绑定）
pub struct GraphBuilderHelper {
    builder: GraphBuilder,
    event_bus: Option<Arc<EventBus>>,
}

impl GraphBuilderHelper {
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

    /// 注册图节点（定义节点函数）
    pub fn node<F, Fut>(mut self, name: &str, func: F) -> Self
    where
        F: Fn(subhuti_core::graph::GraphState) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = NodeResult> + Send + 'static,
    {
        self.builder = self.builder.node(name, func);
        self
    }

    /// 设置节点竞标标签（Actor 根据标签自评匹配度）
    pub fn node_tag(mut self, name: &str, tags: Vec<&str>) -> Self {
        let tags: Vec<String> = tags.into_iter().map(|s| s.to_string()).collect();
        self.builder = self.builder.node_tag(name, tags);
        self
    }

    pub fn edge(mut self, from: &str, to: &str) -> Self {
        self.builder = self.builder.edge(from, to);
        self
    }

    pub fn conditional_edge<F>(mut self, from: &str, condition: F) -> Self
    where
        F: Fn(&subhuti_core::graph::GraphState) -> Route + Send + Sync + 'static,
    {
        self.builder = self.builder.conditional_edge(from, condition);
        self
    }

    pub fn entry(mut self, entry: &str) -> Self {
        self.builder = self.builder.entry(entry);
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

// ─── 图编排工厂 ───────────────────────────────────────────────────

/// 创建所有图编排流程
///
/// 图节点只定义任务标签，不绑定具体专家。
/// 竞标制：Actor 池中的演员通过自评分数竞争节点。
///
/// 注入 LLM 和 EventBus 供图节点内部使用（LLM 调用、事件发布）。
pub fn create_all_graphs(llm: Arc<dyn subhuti_core::LLM>, bus: Arc<EventBus>) -> Vec<Graph> {
    let mut graphs = Vec::new();

    // ── Rust 编程工作流 ──
    let g = rust_programming::create_rust_programming_graph(llm.clone(), bus.clone());
    graphs.push(g);

    // ── Rust 编辑工作流 ──
    let g = rust_edit::create_rust_edit_graph(llm.clone(), bus.clone());
    graphs.push(g);

    graphs
}
