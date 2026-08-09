//! # 节点与边定义

use super::state::GraphState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// 异步节点函数（可克隆，支持 Actor 重启）
#[derive(Clone)]
pub struct NodeFn(
    Arc<dyn Fn(GraphState) -> Pin<Box<dyn Future<Output = NodeResult> + Send>> + Send + Sync>,
);

impl NodeFn {
    /// 创建节点函数
    pub fn new<F, Fut>(func: F) -> Self
    where
        F: Fn(GraphState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = NodeResult> + Send + 'static,
    {
        Self(Arc::new(move |state| Box::pin(func(state))))
    }

    /// 调用节点函数
    pub fn call(&self, state: GraphState) -> Pin<Box<dyn Future<Output = NodeResult> + Send>> {
        (self.0)(state)
    }
}

impl std::fmt::Debug for NodeFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NodeFn(..)")
    }
}

/// 节点执行结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    /// 节点输出内容
    pub output: String,
    /// 是否成功
    pub success: bool,
    /// 错误信息（失败时）
    pub error: Option<String>,
    /// 要写入状态的数据
    pub state_updates: HashMap<String, serde_json::Value>,
    /// 显式路由（覆盖默认边）
    pub route: Option<Route>,
    /// 执行耗时（毫秒）
    pub duration_ms: u64,
}

impl NodeResult {
    /// 成功结果
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            success: true,
            error: None,
            state_updates: HashMap::new(),
            route: None,
            duration_ms: 0,
        }
    }

    /// 成功结果 + 状态更新
    pub fn ok_with_state(
        output: impl Into<String>,
        updates: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            output: output.into(),
            success: true,
            error: None,
            state_updates: updates,
            route: None,
            duration_ms: 0,
        }
    }

    /// 失败结果
    pub fn err(error: impl Into<String>) -> Self {
        Self {
            output: String::new(),
            success: false,
            error: Some(error.into()),
            state_updates: HashMap::new(),
            route: None,
            duration_ms: 0,
        }
    }

    /// 设置显式路由
    pub fn with_route(mut self, output: impl Into<String>, route: Route) -> Self {
        self.output = output.into();
        self.route = Some(route);
        self
    }

    /// 设置耗时
    pub fn with_duration(mut self, ms: u64) -> Self {
        self.duration_ms = ms;
        self
    }
}

/// 路由目标
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Route {
    /// 跳转到指定节点
    To(String),
    /// 结束执行
    End,
}

/// 边：节点间的连接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// 源节点
    pub from: String,
    /// 目标节点
    pub to: String,
}

impl Edge {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// 条件边：根据状态动态路由
pub struct ConditionalEdge {
    /// 源节点
    pub from: String,
    /// 路由函数
    pub condition: Arc<dyn Fn(&GraphState) -> Route + Send + Sync>,
}

impl Clone for ConditionalEdge {
    fn clone(&self) -> Self {
        Self {
            from: self.from.clone(),
            condition: self.condition.clone(),
        }
    }
}

impl std::fmt::Debug for ConditionalEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConditionalEdge")
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

/// 图节点
#[derive(Clone)]
pub struct GraphNode {
    /// 节点名称
    pub name: String,
    /// 节点描述
    pub description: String,
    /// 执行函数
    pub func: NodeFn,
}

impl std::fmt::Debug for GraphNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphNode")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl GraphNode {
    pub fn new<F, Fut>(name: impl Into<String>, func: F) -> Self
    where
        F: Fn(GraphState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = NodeResult> + Send + 'static,
    {
        Self {
            name: name.into(),
            description: String::new(),
            func: NodeFn::new(func),
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }
}
