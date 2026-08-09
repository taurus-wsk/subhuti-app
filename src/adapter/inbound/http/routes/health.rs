use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::get,
};

use super::AppState;
use crate::adapter::inbound::http::route_adapter::RouteEntry;
use chrono::Local;

pub async fn health_handler() -> impl IntoResponse {
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "timestamp": Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        })),
    )
}

pub async fn health_detailed_handler(State(_state): State<AppState>) -> impl IntoResponse {
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "status": "healthy",
            "overall_healthy": true,
            "timestamp": Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            "components": []
        })),
    )
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/health",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/health", get(health_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/health/detailed",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/health/detailed", get(health_detailed_handler)),
    }
}
