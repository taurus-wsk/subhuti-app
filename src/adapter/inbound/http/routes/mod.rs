//! # HTTP 辅助路由
//!
//! 将 REST API 请求映射到应用服务。
//!
//! 六边形架构 + inventory 自动注册说明：
//! - `chat` / `orchestrate` / `skill` 等核心业务路由位于 `adapter::inbound::http::adapters`，
//!   本模块包含辅助路由（health/experts/traces/sessions）。
//! - 所有路由（业务 + 辅助）均通过 `inventory::submit!` 自注册，
//!   `route_adapter::build_router` 编译期自动收集，无需手动维护注册列表。
//! - `AppState` 作为 axum State 分发的依赖容器，仅持有端口接口。

pub mod experts;
pub mod health;
pub mod knowledge;
pub mod sessions;
pub mod traces;

use std::sync::Arc;

use crate::application::{ChatPort, ExpertQueryPort, SessionObserverPort, TraceObserverPort};

/// Axum State 分发的依赖注入容器
///
/// 六边形架构：入站适配层通过 Port 接口调用应用层，不直接依赖框架实例。
/// 所有依赖以 `Arc<dyn Port>` 形式持有，实现 Clone 以支持 axum State 分发。
///
/// 接口隔离：原 OrchestratePort（9 方法）拆分为 2 个窄端口，
/// handler 只依赖需要的端口。
#[derive(Clone)]
pub struct AppState {
    /// 入站窄端口 1：聊天/编排调度
    pub chat_port: Arc<dyn ChatPort>,
    /// 入站窄端口 2：专家查询
    pub expert_query_port: Arc<dyn ExpertQueryPort>,
    /// 出站端口：追踪观察者（记录执行链路）
    pub trace_observer: Arc<dyn TraceObserverPort>,
    /// 出站端口：会话观察者（记录会话信息）
    pub session_observer: Arc<dyn SessionObserverPort>,
    /// 知识库存储（可选，PG 降级模式下为 None）
    pub pg_storage: Option<Arc<subhuti_infra::sutra_library::storage::PgStorage>>,
}

// ─── FromRef<()> for AppState ──────────────────────────────────
//
// 用于测试场景：当 Router 状态为 () 时，State<AppState> 提取器
// 通过此实现从线程局部存储获取 AppState。
//
// 这是 axum 0.7 的已知限制：Router<S> 仅在 S = () 时实现 Service。
// 测试中通过 `RouterService` 注入状态到 Extension，同时设置线程局部状态
// 以支持 State<AppState> 提取器。
//
// 生产路径（Router<AppState>.with_state(state)）不依赖此实现。

thread_local! {
    static TEST_APP_STATE: std::cell::RefCell<Option<AppState>> = const { std::cell::RefCell::new(None) };
}

impl axum::extract::FromRef<()> for AppState {
    fn from_ref(_input: &()) -> Self {
        TEST_APP_STATE.with(|cell| {
            cell.borrow()
                .clone()
                .expect("AppState not set in thread-local storage. Use `set_test_app_state` or `RouterService`.")
        })
    }
}

/// 设置测试用的 AppState 到线程局部存储。
///
/// 在使用 `Router<()>` 或 `RouterService` 进行测试前调用，
/// 以使 `State<AppState>` 提取器能够正常工作。
pub fn set_test_app_state(state: AppState) {
    TEST_APP_STATE.with(|cell| {
        *cell.borrow_mut() = Some(state);
    });
}

/// 清除线程局部存储中的 AppState。
///
/// 测试完成后调用以避免状态泄漏。
pub fn clear_test_app_state() {
    TEST_APP_STATE.with(|cell| {
        *cell.borrow_mut() = None;
    });
}
