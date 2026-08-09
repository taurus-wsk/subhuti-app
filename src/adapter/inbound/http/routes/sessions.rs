use axum::{
    extract::{Json, Path, State},
    response::IntoResponse,
    routing::get,
};

use super::AppState;
use crate::adapter::inbound::http::route_adapter::RouteEntry;

pub async fn sessions_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sessions = state.session_observer.list_sessions();
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": sessions,
            "total": sessions.len(),
        })),
    )
}

pub async fn sessions_get_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.session_observer.get_session(&id) {
        Some(session) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "data": session,
            })),
        ),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Session not found",
                "code": 404,
            })),
        ),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/sessions",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/sessions", get(sessions_list_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/sessions/:id",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/sessions/:id", get(sessions_get_handler)),
    }
}
