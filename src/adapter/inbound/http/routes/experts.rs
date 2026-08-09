use axum::{
    extract::{Json, State},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;

use super::AppState;
use crate::adapter::inbound::http::route_adapter::RouteEntry;

#[derive(Debug, Deserialize)]
pub struct MatchExpertRequest {
    input: String,
}

pub async fn experts_list_handler(State(state): State<AppState>) -> impl IntoResponse {
    // 通过 ExpertQueryPort 获取专家列表（实际已注册的专家快照）
    let experts = state.expert_query_port.list_experts().await;
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": experts,
            "total": experts.len(),
        })),
    )
}

pub async fn experts_active_handler(State(state): State<AppState>) -> impl IntoResponse {
    // 通过 ExpertQueryPort 获取当前激活的专家（返回 None = 未激活）
    let active = state.expert_query_port.active_expert().await;
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": active,
        })),
    )
}

pub async fn experts_match_handler(
    State(state): State<AppState>,
    Json(req): Json<MatchExpertRequest>,
) -> impl IntoResponse {
    // 通过 ExpertQueryPort 匹配专家（空数组 = 未匹配到）
    let matched = state.expert_query_port.match_expert(&req.input).await;
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "success": true,
            "data": matched,
        })),
    )
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/experts",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/experts", get(experts_list_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/experts/active",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/experts/active", get(experts_active_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/experts/match",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/experts/match", post(experts_match_handler)),
    }
}
