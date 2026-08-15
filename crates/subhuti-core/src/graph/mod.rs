//! # 图引擎 - DAG + 事件驱动混合编排
//!
//! 结合 LangGraph 的图结构化能力和事件驱动的运行时适应性。
//!
//! ## 核心设计
//!
//! - **Node**: 可执行的计算单元（Agent / Tool / 子图）
//! - **Edge**: 节点间的连接，支持条件路由
//! - **GraphState**: 类型安全的状态容器，支持自定义 Reducer
//! - **Checkpoint**: 断点续跑，失败后可从检查点恢复
//! - **EventBus 集成**: 每个节点执行自动发布事件
//!
//! ## 示例
//!
//! ```rust,ignore
//! use subhuti::graph::{GraphBuilder, GraphState, NodeResult, Route};
//!
//! let graph = GraphBuilder::new()
//!     .node("planner", |_state| Box::pin(async {
//!         NodeResult::ok("plan created")
//!     }))
//!     .node("executor", |_state| Box::pin(async {
//!         NodeResult::ok("executed")
//!     }))
//!     .node("reviewer", |_state| Box::pin(async {
//!         NodeResult::with_route("done", Route::End)
//!     }))
//!     .edge("planner", "executor")
//!     .edge("executor", "reviewer")
//!     .conditional_edge("reviewer", |state| {
//!         match state.get("route").unwrap_or("") {
//!             "retry" => Route::To("executor"),
//!             _ => Route::End,
//!         }
//!     })
//!     .entry("planner")
//!     .build();
//!
//! let result = graph.run_with_id("example", &mut GraphState::new()).await?;
//! ```

pub mod actor;
pub mod checkpoint;
pub mod engine;
pub mod execution;
pub mod node;
pub mod state;
pub mod validator;

pub use actor::{
    ActorAddr, ActorHandle, ActorHealth, ActorLifecycle, ActorStats, EventDrivenActor,
    EventDrivenScheduler, NodeActor, NodeMessage, SupervisionStrategy, Supervisor,
};
pub use checkpoint::{Checkpoint, CheckpointStore, MemoryCheckpointStore};
pub use engine::{Graph, GraphBuilder, GraphError, GraphOutput, GraphStructure};
pub use execution::{
    ExecutionCommand, ExecutionContext, ExecutionSnapshot, ExecutionStatus, GraphExecution,
    GraphExecutionHandle,
};
pub use node::{ConditionalEdge, Edge, GraphNode, NodeFn, NodeResult, Route};
pub use state::{GraphState, StateReducer};
pub use validator::{
    CompositeValidator, INodeValidator, JsonFormatValidator, NodeFixRunner, RequiredFieldsValidator,
};
// Guardrail 通过 crate::guardrails::IGuardrail 引用，GraphBuilder.guardrail() 接受此 trait
