//! # Flow + Tool 运行时（框架 shell）
//!
//! 设计：**Skill = Flow + Tool**。Flow 是固定、线性、数据驱动的节点序列模板
//! （`analyze→plan→edit→verify→done`），可复用切换、按需启用阶段；节点顺序
//! **不耗 LLM 编排**。Tool 分两类：纯工具调用（`Pure`）与 LLM 决策节点（`Llm`）。
//!
//! 分层（与六边形架构一致）：
//! - 本模块是**纯机制**：只定义 `FlowTemplate` / `FlowContext`、`ToolRuntime`
//!   稳定接口、`FlowRunner` 逐节点执行 + 事件发布。不含任何业务。
//! - Tool **接口由本框架 shell 定义**，由领域/应用层实现具体 ToolRuntime，
//!   并绑定真实工具（`llm / file_system / command / toolchain` 等）。
//! - `FlowRunner` 通过 `ToolRuntime` 回调调用，不感知实现细节。

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::Serialize;

use crate::event::{AgentEventData, EventBus};
use crate::Result;

/// Flow 节点：一个 React 阶段 + 可选参数模板。
///
/// 节点性质（纯工具 vs LLM）由 `stage` 唯一决定，不再用 `FlowNodeKind` 区分；
/// 对外展示名（步骤名 / 工具名）也由 `stage` 派生，避免多份字符串手工同步。
#[derive(Debug, Clone)]
pub struct FlowNode {
    /// 该节点对应的 React 阶段（唯一事实来源）
    pub stage: ReactStage,
    /// 该节点参数模板
    pub params: serde_json::Value,
}

/// Flow 模板：固定线性节点序列，数据驱动，可复用切换
#[derive(Debug, Clone)]
pub struct FlowTemplate {
    /// 模板 id（如 `rust.react` / `blender.react`，复用同一结构）
    pub id: String,
    /// 事件用标识，直接喂给 `FlowStarted.flow_type`
    pub flow_type: String,
    pub nodes: Vec<FlowNode>,
}

/// 节点执行结果
#[derive(Debug, Clone, Serialize)]
pub struct FlowNodeResult {
    pub output: String,
    pub success: bool,
}

/// Flow 上下文：输入 / 累积产物 / 逐节点结果
#[derive(Debug, Clone, Default)]
pub struct FlowContext {
    /// 用户请求
    pub input: String,
    /// 累积产物（兼容现值 `push_phase` 扫描式回填语义）
    pub output: String,
    /// node_id -> 该节点结果（有序）
    pub results: BTreeMap<String, FlowNodeResult>,
}

impl FlowContext {
    pub fn new(input: &str) -> Self {
        let mut ctx = Self::default();
        ctx.input = input.to_string();
        ctx
    }

    /// 追加到累积产物
    pub fn push_output(&mut self, s: &str) {
        self.output.push_str(s);
    }
}

/// 工具运行时（框架 shell 的稳定接口）
///
/// 领域层定义接口、基础层实现：实现方绑定真实工具能力，并决定
/// 每个纯工具 / LLM 决策节点怎么做。`FlowRunner` 只按此接口调用。
#[async_trait]
pub trait ToolRuntime: Send + Sync {
    /// 纯工具调用
    async fn call_pure(&self, node: &FlowNode, ctx: &FlowContext) -> Result<FlowNodeResult>;
    /// LLM 决策节点调用（`plan` / `edit`）
    async fn call_llm(&self, node: &FlowNode, ctx: &FlowContext) -> Result<FlowNodeResult>;
}

/// 固定 React 反应链类型标识
pub const REACT_FLOW_TYPE: &str = "analyze_plan_edit_verify_done";

/// React 反应链的固定阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactStage {
    Analyze,
    Plan,
    Edit,
    Verify,
    Done,
}

impl ReactStage {
    /// 是否为纯工具节点（不耗 LLM）
    pub fn is_pure(self) -> bool {
        matches!(
            self,
            ReactStage::Analyze | ReactStage::Verify | ReactStage::Done
        )
    }

    /// 步骤名（`FlowContext.results` 的 key，事件 `FlowStepExecuted.step_name`）
    pub fn step_name(self) -> &'static str {
        match self {
            ReactStage::Analyze => "analyze",
            ReactStage::Plan => "plan",
            ReactStage::Edit => "edit",
            ReactStage::Verify => "verify",
            ReactStage::Done => "done",
        }
    }

    /// 工具展示名（事件 `ToolCalling/ToolResponded.tool_name`）
    pub fn tool_name(self) -> &'static str {
        match self {
            ReactStage::Analyze => "fs.analyze",
            ReactStage::Plan => "plan",
            ReactStage::Edit => "edit",
            ReactStage::Verify => "toolchain.verify",
            ReactStage::Done => "aggregate.done",
        }
    }
}

/// 构造固定 React 反应链模板：`analyze→plan→edit→verify→done`。
///
/// - `id`：模板 id（如 `"rust.react"`），用于区分不同领域复用的实例。
/// - `stages`：按需启用的阶段（顺序固定；可通过切片取舍部分阶段）。
pub fn react_flow(id: &str, stages: &[ReactStage]) -> FlowTemplate {
    let nodes = stages
        .iter()
        .copied()
        .map(|stage| FlowNode {
            stage,
            params: serde_json::Value::Null,
        })
        .collect();
    FlowTemplate {
        id: id.to_string(),
        flow_type: REACT_FLOW_TYPE.to_string(),
        nodes,
    }
}

/// Flow 执行器：逐节点执行 + 上下文传递 + 事件发布。
///
/// - 每节点：`ToolCalling` → 执行 → `ToolResponded` → `FlowStepExecuted`。
/// - 开始 `FlowStarted`，结束 `FlowCompleted`。
/// - 节点返回 `success=false` 时仍继续并透出，成败判决策留给上层（沿用现状口径）。
/// - 运行时 `Err` 时中止并上抛（任一步失败即整体失败）。
pub struct FlowRunner;

impl FlowRunner {
    /// 执行一个 Flow 模板，返回累积产物（`FlowContext.output`）。
    ///
    /// `bus` 为 `None` 时静默（单测或非事件环境）。
    pub async fn run(
        bus: Option<&EventBus>,
        trace_id: &str,
        session_id: Option<&str>,
        flow: &FlowTemplate,
        ctx: &mut FlowContext,
        runtime: &dyn ToolRuntime,
    ) -> Result<String> {
        Self::emit(
            bus,
            trace_id,
            session_id,
            AgentEventData::FlowStarted {
                flow_type: flow.flow_type.clone(),
                input: ctx.input.clone(),
            },
        )
        .await;

        let iterations = flow.nodes.len();
        for (i, node) in flow.nodes.iter().enumerate() {
            // 工具调用前（tool_name 由 stage 派生，args 即节点参数模板）
            let tool_name = node.stage.tool_name().to_string();
            let args = node.params.clone();
            Self::emit(
                bus,
                trace_id,
                session_id,
                AgentEventData::ToolCalling {
                    tool_name: tool_name.clone(),
                    args,
                },
            )
            .await;

            let started = std::time::Instant::now();
            // 执行（错误直接终止，整体判失败）；纯工具 / LLM 由 stage 决定
            let result = if node.stage.is_pure() {
                runtime.call_pure(node, ctx).await?
            } else {
                runtime.call_llm(node, ctx).await?
            };
            let duration_ms = started.elapsed().as_millis() as u64;

            // 结果写回上下文（key = 步骤名）
            ctx.output.push_str(&result.output);
            ctx.results
                .insert(node.stage.step_name().to_string(), result.clone());

            // 工具响应后
            Self::emit(
                bus,
                trace_id,
                session_id,
                AgentEventData::ToolResponded {
                    tool_name: tool_name.clone(),
                    result: result.output.clone(),
                    success: result.success,
                    duration_ms,
                },
            )
            .await;
            // 步骤执行
            Self::emit(
                bus,
                trace_id,
                session_id,
                AgentEventData::FlowStepExecuted {
                    step_index: i,
                    step_name: node.stage.step_name().to_string(),
                    result: result.output.clone(),
                },
            )
            .await;
        }

        Self::emit(
            bus,
            trace_id,
            session_id,
            AgentEventData::FlowCompleted {
                output: ctx.output.clone(),
                iterations,
            },
        )
        .await;

        Ok(ctx.output.clone())
    }

    /// 统一事件出口：带 trace/session 发射（供 ProgressEventBridge 识别转发）
    async fn emit(
        bus: Option<&EventBus>,
        trace_id: &str,
        session_id: Option<&str>,
        data: AgentEventData,
    ) {
        if let Some(b) = bus {
            if !trace_id.is_empty() {
                b.emit_with_trace(data, trace_id, session_id.map(|s| s.to_string()))
                    .await;
            } else {
                b.emit(data).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试实现：按工具/角色返回可辨别的输出，并记录调用序列
    struct MockRuntime {
        calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl ToolRuntime for MockRuntime {
        async fn call_pure(&self, node: &FlowNode, _ctx: &FlowContext) -> Result<FlowNodeResult> {
            let name = node.stage.tool_name();
            self.calls.lock().unwrap().push(format!("pure:{name}"));
            Ok(FlowNodeResult {
                output: format!("<{name}>"),
                success: true,
            })
        }
        async fn call_llm(&self, node: &FlowNode, _ctx: &FlowContext) -> Result<FlowNodeResult> {
            let name = node.stage.tool_name();
            self.calls.lock().unwrap().push(format!("llm:{name}"));
            Ok(FlowNodeResult {
                output: format!("[{name}:{}]", _ctx.input),
                success: true,
            })
        }
    }

    fn default_stages() -> Vec<ReactStage> {
        vec![
            ReactStage::Analyze,
            ReactStage::Plan,
            ReactStage::Edit,
            ReactStage::Verify,
            ReactStage::Done,
        ]
    }

    #[tokio::test]
    async fn runner_executes_nodes_in_order_and_routes_context() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let runtime = MockRuntime {
            calls: calls.clone(),
        };
        let flow = react_flow("test.react", &default_stages());
        let mut ctx = FlowContext::new("hi");
        let out = FlowRunner::run(None, "", None, &flow, &mut ctx, &runtime)
            .await
            .unwrap();

        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "pure:fs.analyze".to_string(),
                "llm:plan".to_string(),
                "llm:edit".to_string(),
                "pure:toolchain.verify".to_string(),
                "pure:aggregate.done".to_string(),
            ]
        );
        // 产物递增累积；每步结果落进 results
        assert_eq!(ctx.results.len(), 5);
        assert!(out.contains("[plan:hi]"));
        assert!(out.contains("<toolchain.verify>"));
    }

    #[tokio::test]
    async fn runner_enables_subset_of_stages() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let runtime = MockRuntime {
            calls: calls.clone(),
        };
        // 只启用 plan → edit → done
        let flow = react_flow(
            "subset.react",
            &[ReactStage::Plan, ReactStage::Edit, ReactStage::Done],
        );
        let mut ctx = FlowContext::new("hi");
        let _ = FlowRunner::run(None, "", None, &flow, &mut ctx, &runtime)
            .await
            .unwrap();
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "llm:plan".to_string(),
                "llm:edit".to_string(),
                "pure:aggregate.done".to_string()
            ]
        );
    }

    #[tokio::test]
    async fn runner_emits_flow_and_tool_events_in_order() {
        let bus = EventBus::new(64);
        let mut rx = bus.subscribe_raw();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let runtime = MockRuntime { calls };
        let flow = react_flow("evt.react", &default_stages());
        let mut ctx = FlowContext::new("hi");

        // 先订阅再发送（broadcast 只收到订阅后事件）
        let run = {
            let bus = &bus;
            let flow = &flow;
            let runtime = &runtime;
            FlowRunner::run(Some(bus), "trace-1", Some("s1"), flow, &mut ctx, runtime)
        };

        let (_out, events) = tokio::join!(run, async {
            let mut kinds = Vec::new();
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        kinds.push(ev.data.event_type().to_string());
                        if matches!(ev.data, AgentEventData::FlowCompleted { .. }) {
                            break;
                        }
                    }
                    // 订阅后容量不足导致漏读：跳过该批，继续取后续
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            kinds
        });

        let expected: Vec<String> = {
            let mut v = vec!["flow_started".to_string()];
            for _ in 0..5 {
                v.extend([
                    "tool_calling".to_string(),
                    "tool_responded".to_string(),
                    "flow_step_executed".to_string(),
                ]);
            }
            v.push("flow_completed".to_string());
            v
        };
        assert_eq!(events, expected);
    }

    #[tokio::test]
    async fn runner_surfaces_node_failure_without_panicking() {
        struct FailingRuntime;
        #[async_trait]
        impl ToolRuntime for FailingRuntime {
            async fn call_pure(
                &self,
                node: &FlowNode,
                _ctx: &FlowContext,
            ) -> Result<FlowNodeResult> {
                Ok(FlowNodeResult {
                    output: format!("<fail:{}>", node.stage.tool_name()),
                    // verify 之外全成功；verify 标记失败
                    success: node.stage != ReactStage::Verify,
                })
            }
            async fn call_llm(
                &self,
                node: &FlowNode,
                _ctx: &FlowContext,
            ) -> Result<FlowNodeResult> {
                Ok(FlowNodeResult {
                    output: format!("[{}]", node.stage.tool_name()),
                    success: true,
                })
            }
        }
        let runtime = FailingRuntime;
        let flow = react_flow("fail.react", &default_stages());
        let mut ctx = FlowContext::new("hi");
        let out = FlowRunner::run(None, "", None, &flow, &mut ctx, &runtime)
            .await
            .unwrap();
        // 运行不 panic；失败节点结果如实透出，供上层判决
        assert_eq!(ctx.results["verify"].success, false);
        assert!(out.contains("<fail:toolchain.verify>"));
    }
}
