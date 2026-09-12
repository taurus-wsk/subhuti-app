//! # HTTP 适配器层
//!
//! Axum HTTP 服务器，将应用服务暴露为 REST API。
//! 采用 inventory 自动注册模式（方案 C）：路由通过 `inventory::submit!` 自注册，
//! `route_adapter::build_router` 编译期自动收集所有路由，无需手动维护注册列表。
//!
//! ## 模块
//!
//! - `route_adapter` - RouteEntry + inventory 收集 + build_router / build_service
//! - `adapters` - 业务路由 handler（编排 Orchestrate）+ inventory::submit!
//! - `middleware` - HTTP 中间件（Trace ID、请求日志）；trace/session 记录由应用层装饰器处理
//! - `routes` - 辅助路由 handler（health/experts/traces/sessions）+ inventory::submit!
//! - `server` - 服务器启动，调用 build_router 自动装配路由

pub mod adapters;
pub mod middleware;
pub mod route_adapter;
pub mod routes;
pub mod server;
