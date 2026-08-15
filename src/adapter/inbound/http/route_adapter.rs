//! # 路由自动注册（方案 C：inventory 模式）
//!
//! 利用 `inventory` crate 在编译期自动收集所有路由注册项，
//! 无需在 server.rs 手动维护路由列表。
//!
//! ## 设计原则
//!
//! - **自注册**：每个路由文件通过 `inventory::submit!` 自行注册，新增路由文件无需改动 server.rs
//! - **路径内聚**：路由路径、方法、handler 配置全部内聚在路由文件内部
//! - **中央收集**：`build_router` 遍历 `inventory::iter::<RouteEntry>()` 统一构建 Router
//! - **无限扩展**：新增路由只需新建文件 + `inventory::submit!`，无需修改任何注册代码
//! - **trace 无关**：trace/session 记录由应用层 `TraceAppService` 装饰器自动覆盖所有端口，
//!   路由注册完全不感知 trace；`trace_enabled` 仅作为 debug 分类展示标记
//!
//! ## 使用方式
//!
//! 在任意路由文件中（adapters.rs / routes/*.rs / 新文件均可）：
//!
//! ```rust,ignore
//! use axum::routing::get;
//! use crate::adapter::inbound::http::route_adapter::RouteEntry;
//!
//! async fn hello_handler() -> &'static str { "hello" }
//!
//! inventory::submit! {
//!     RouteEntry {
//!         path: "/subhuti/api/v1/hello",
//!         method: "GET",
//!         trace_enabled: false,
//!         register: |r| r.route("/subhuti/api/v1/hello", get(hello_handler)),
//!     }
//! }
//! ```
//!
//! server.rs 只需调用 `build_router()`，所有路由自动挂载。

use axum::body::Body;
use axum::http::Request;
use axum::response::{IntoResponse, Response};
use axum::{routing::get, Router};
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tower::ServiceExt;
use tower_service::Service;

use crate::adapter::inbound::http::adapters::ApiSuccess;
use crate::adapter::inbound::http::routes::AppState;
use crate::application::observer::{record_fn_log, LogLevel};

/// 路由注册项
///
/// 方案 C 的核心数据结构：每个路由通过 `inventory::submit!` 提交一个实例，
/// `build_router` 遍历所有实例完成注册。
///
/// # 字段说明
///
/// - `path`：路由路径（用于 /_debug/routes 展示，不参与实际注册逻辑）
/// - `method`：HTTP 方法字符串（如 "GET"、"POST"、"GET+POST"），仅用于展示
/// - `trace_enabled`：核心业务路由分类标记（仅用于 /_debug/routes 展示分类；
///   trace 由应用层 `TraceAppService` 装饰器自动覆盖所有端口，与路由注册无关）
/// - `register`：注册函数，将路由挂载到传入的 Router 上
///
/// # 关于 register 函数指针
///
/// `register` 是一个不捕获环境的闭包（自动转换为 `fn` 指针），
/// 内部调用 `router.route(path, method(handler))` 完成实际注册。
/// 这样设计是因为 axum 的 `Handler` trait 是泛型的，不同 handler 参数类型不同，
/// 无法统一存储；而通过 `register` 函数让每个路由自己完成类型擦除。
pub struct RouteEntry {
    /// 路由路径（展示用）
    pub path: &'static str,
    /// HTTP 方法字符串（展示用，如 "GET"、"POST"、"GET+POST"）
    pub method: &'static str,
    /// 核心业务路由分类标记（仅用于 /_debug/routes 展示分类，trace 由装饰器自动处理）
    pub trace_enabled: bool,
    /// 注册函数：接收 Router，返回挂载了本路由的 Router
    pub register: fn(Router<AppState>) -> Router<AppState>,
}

// 声明 inventory 收集点：所有 `inventory::submit! { RouteEntry { ... } }` 都会被收集
inventory::collect!(RouteEntry);

/// 收集所有已注册路由的元信息（用于 /_debug/routes 展示）
pub fn list_routes() -> Vec<(&'static str, &'static str, bool)> {
    inventory::iter::<RouteEntry>()
        .map(|e| (e.path, e.method, e.trace_enabled))
        .collect()
}

/// 构建 Router<AppState>（不含全局 layer，由 server.rs 统一挂载）
///
/// 遍历所有 inventory 收集的 RouteEntry，统一调用其 `register` 函数挂载路由。
/// trace/session 记录由应用层 `TraceAppService` 装饰器自动覆盖所有入站端口，
/// 路由注册完全不感知 trace，`trace_enabled` 仅用于 /_debug/routes 分类展示。
///
/// 注意：此函数不调用 `with_state`，由调用方（server.rs）统一设置状态。
pub fn build_router() -> Router<AppState> {
    let mut router = Router::new();
    let mut count = 0usize;

    for entry in inventory::iter::<RouteEntry> {
        router = (entry.register)(router);
        count += 1;
    }

    record_fn_log(
        None,
        "",
        LogLevel::Info,
        format!("✅ 路由自动注册完成（inventory 模式）：total={}", count),
        None,
    );

    router
}

/// 构建可作为 `tower::Service` 使用的路由服务（用于单元测试）
///
/// 返回的 `RouterService` 实现 `Service<Request<Body>>`，
/// 用于测试时直接 oneshot 调用，无需启动真实 HTTP 服务器。
///
/// 实现原理：
/// - 通过 `build_router` 构建 `Router<AppState>`
/// - `with_state(state)` 转为 `Router<()>`（依赖 `FromRef<()>` for `AppState`）
/// - `Router<()>` 实现 `Service<Request<Body>>`，可直接 oneshot
/// - `State<AppState>` 提取器通过 `FromRef<()>` 从线程局部存储获取 AppState
pub fn build_service(state: AppState) -> RouterService {
    let router: Router<AppState> = build_router();
    let router: Router<()> = router.with_state(state.clone());
    RouterService::new(router, state)
}

// ─── /_debug/routes 调试端点（自身也通过 inventory 自动注册）────────

/// GET /subhuti/api/v1/_debug/routes
///
/// 返回所有已注册路由的元信息（path / method / trace_enabled），
/// 用于快速排查路由是否正确注册。
async fn debug_routes_handler() -> impl IntoResponse {
    let routes: Vec<serde_json::Value> = inventory::iter::<RouteEntry>()
        .map(|e| {
            serde_json::json!({
                "path": e.path,
                "method": e.method,
                "trace_enabled": e.trace_enabled,
            })
        })
        .collect();

    ApiSuccess::ok(serde_json::json!({
        "routes": routes,
        "total": routes.len(),
    }))
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/_debug/routes",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/_debug/routes", get(debug_routes_handler)),
    }
}

// ─── RouterService（测试基础设施，保留）────────────────────────────

/// 可作为 `tower::Service<Request<Body>>` 使用的路由服务。
///
/// 用于在测试中直接调用已注册的路由，无需启动真实 HTTP 服务器。
/// 内部使用 `Router<()>` 的 Service 实现，`AppState` 通过
/// `FromRef<()>` 线程局部存储注入。
#[derive(Clone)]
pub struct RouterService {
    router: Router<()>,
    state: Arc<AppState>,
}

impl RouterService {
    pub fn new(router: Router<()>, state: AppState) -> Self {
        Self {
            router,
            state: Arc::new(state),
        }
    }

    /// 获取 AppState 引用
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// 获取内部 Router 引用（调试或高级用例）
    #[allow(dead_code)]
    pub fn router(&self) -> &Router<()> {
        &self.router
    }

    /// 获取 AppState 的 Arc 克隆（便于在异步任务中共享）
    #[allow(dead_code)]
    pub fn state_arc(&self) -> Arc<AppState> {
        self.state.clone()
    }

    /// 转换为 `Router<AppState>`（用于 axum::serve）。
    ///
    /// 通过 `Router<()>.with_state(())` 将状态绑定回路由。
    /// 由于 Router<()> 的 with_state 需要 () 作为 from 状态，
    /// 通过 `FromRef<()>` for `AppState` 转换为目标状态。
    pub fn into_router(self) -> Router<AppState> {
        self.router.with_state::<AppState>(())
    }
}

impl Service<Request<Body>> for RouterService {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let router = self.router.clone();
        Box::pin(async move {
            // Router<()> 实现了 Service<Request<Body>>
            // 内部通过 FromRef<()> for AppState 从线程局部获取 state
            router.oneshot(req).await
        })
    }
}
