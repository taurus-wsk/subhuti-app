//! # 领域层 Flow 执行封装（Skill = Flow + Tool 的领域落地）
//!
//! 六边形分层（与设计文档一致）：
//! - core(`subhuti_core::orchestrator::flow`) 是**纯机制**：`FlowTemplate` / `FlowContext` /
//!   `ToolRuntime` 接口 / `FlowRunner` 逐节点执行 + 事件发射，不含业务。
//! - 本模块是**领域适配**：把 core 的 `ToolRuntime` 接口接到领域执行上下文，
//!   并提供一个统一的 `run_react_flow` 入口，让各领域专家复用同一张固定
//!   React 模板（`analyze→plan→edit→verify→done`）承载执行，按需启用阶段。
//!
//! 关键设计：**节点处理逻辑由技能自己提供**（`FlowNodeExecutor`），
//! `DomainToolRuntime` 只负责把 Flow 节点的 `spec.id` / `spec.role` 分派到
//! 对应的 executor 方法；不同技能的差异（prompt、文件写入、编译修复循环）
//! 保留在各自 executor 里，不塞进通用分派，避免行为漂移。
//!
//! 事件：`run_react_flow` 在 `event_bus` 注入且带非空 `trace_id` 时把真实
//! `EventBus` 交给 `FlowRunner`，由其发射 `FlowStarted/FlowStepExecuted/
//! FlowCompleted/ToolCalling/ToolResponded`；无总线时静默（单测/未注入场景）。

use std::sync::Arc;

use async_trait::async_trait;
use subhuti_core::orchestrator::{
    react_flow, FlowContext, FlowNode, FlowNodeResult, FlowRunner, ReactStage, ToolRuntime,
};
use subhuti_core::Result as CoreResult;

use crate::domain::traits::{DomainError, DomainExecutionContext, DomainResult};

/// 节点执行器：领域技能提供的"这一个节点怎么跑"回调集合。
///
/// 每个技能实现/构造自己的 executor，把自身不同的分析与生成逻辑放进来；
/// FlowRunner 负责节点顺序、上下文传递与事件发射，不感知具体技能。
#[async_trait]
pub trait FlowNodeExecutor: Send + Sync {
    /// 纯工具 / 无副作用节点（`analyze` / `verify` / `done` 及领域内联节点）。
    ///
    /// `stage` 取该节点对应的 React 阶段（`Analyze` / `Verify` / `Done`），
    /// 实现方按枚举做穷尽分派并产出 `FlowNodeResult`（类型化，消除字符串魔法绑定）。
    async fn execute_pure(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult>;

    /// LLM 决策节点（`plan` / `edit`）。
    ///
    /// `stage` 为该节点对应的 React 阶段（`Plan` / `Edit`），实现方组装领域消息并调 LLM。
    async fn execute_llm(
        &self,
        stage: ReactStage,
        exec_ctx: &DomainExecutionContext,
        ctx: &mut FlowContext,
    ) -> DomainResult<FlowNodeResult>;
}

/// 把 core 的 `ToolRuntime` 接口接到领域实现。
///
/// 内部持有领域执行上下文与节点的 executor，把 Flow 节点分派出去；
/// 错误从领域错误映射为 core 错误（`FlowRunner` 要求返回 `core::Result`）。
pub struct DomainToolRuntime {
    exec_ctx: DomainExecutionContext,
    executor: Arc<dyn FlowNodeExecutor>,
}

impl DomainToolRuntime {
    pub fn new(exec_ctx: DomainExecutionContext, executor: Arc<dyn FlowNodeExecutor>) -> Self {
        Self { exec_ctx, executor }
    }
}

#[async_trait]
impl ToolRuntime for DomainToolRuntime {
    async fn call_pure(&self, node: &FlowNode, ctx: &FlowContext) -> CoreResult<FlowNodeResult> {
        let mut ctx = ctx.clone();
        let r = self
            .executor
            .execute_pure(node.stage, &self.exec_ctx, &mut ctx)
            .await
            .map_err(|e| match e {
                DomainError::Precondition(m) => subhuti_core::Error::Precondition(m),
                other => subhuti_core::Error::Any(anyhow::anyhow!(other)),
            })?;
        Ok(r)
    }

    async fn call_llm(&self, node: &FlowNode, ctx: &FlowContext) -> CoreResult<FlowNodeResult> {
        let mut ctx = ctx.clone();
        let r = self
            .executor
            .execute_llm(node.stage, &self.exec_ctx, &mut ctx)
            .await
            .map_err(|e| match e {
                DomainError::Precondition(m) => subhuti_core::Error::Precondition(m),
                other => subhuti_core::Error::Any(anyhow::anyhow!(other)),
            })?;
        Ok(r)
    }
}

/// 统一入口：构造 `FlowContext` + 固定 React 模板，交给 `FlowRunner` 执行。
///
/// - `stages`：按需启用的阶段（`react_flow` 天然支持切片取舍）。
/// - `id`：模板 id（如 `"rust.react"` / `"blender.react"`，区分不同领域复用的实例）。
/// - 返回累积产物（`FlowContext.output`），与现有"返回执行结果字符串"口径一致。
///
/// 事件：`exec_ctx.event_bus` 为 `Some` 且 `trace_id` 非空时传真实总线，
/// `FlowRunner` 借此发射 `FlowStarted/FlowStepExecuted/FlowCompleted/ToolCalling/ToolResponded`。
pub async fn run_react_flow(
    exec_ctx: &DomainExecutionContext,
    id: &str,
    stages: &[ReactStage],
    executor: Arc<dyn FlowNodeExecutor>,
) -> DomainResult<String> {
    let bus = exec_ctx.event_bus.clone();
    let trace_id = exec_ctx.ctx.trace_id.clone().unwrap_or_default();
    let session_id = exec_ctx.ctx.session_id.clone();

    let mut fctx = FlowContext::new(&exec_ctx.ctx.input);
    let flow = react_flow(id, stages);
    let runtime = DomainToolRuntime::new(exec_ctx.clone(), executor);

    let output = FlowRunner::run(
        bus.as_deref(),
        &trace_id,
        session_id.as_deref(),
        &flow,
        &mut fctx,
        &runtime,
    )
    .await
    .map_err(|e| DomainError::ExecutionError(e.to_string()))?;

    Ok(output)
}

/// 类型化中间结果 key：枚举值 → 黑板字符串 key。
///
/// 专家把跨阶段传值的 key 定义成枚举实现本 trait，读写 `FlowContext.results`
/// 一律经 [`flow_str`] / [`flow_flag`]；裸字符串无法再传入，编译期即报错，
/// 魔法字符串只存在各枚举的 `as_str()` 一处。框架记账 key（`ReactStage` 步骤名）
/// 同样实现本 trait，读取特定步骤产物也走类型化。
pub trait FlowKeyId {
    fn as_str(&self) -> &'static str;
}

impl FlowKeyId for ReactStage {
    fn as_str(&self) -> &'static str {
        self.step_name()
    }
}

/// 从 Flow 上下文读取中间结果字符串
pub fn flow_str<K: FlowKeyId>(ctx: &FlowContext, key: K) -> String {
    ctx.results
        .get(key.as_str())
        .map(|r| r.output.clone())
        .unwrap_or_default()
}

/// 从 Flow 上下文读取布尔标志（存为 "true"/"false"）
pub fn flow_flag<K: FlowKeyId>(ctx: &FlowContext, key: K) -> bool {
    ctx.results
        .get(key.as_str())
        .map(|r| r.output == "true")
        .unwrap_or(false)
}
