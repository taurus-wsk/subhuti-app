//! # 知识库 & 文档 CRUD 路由
//!
//! 提供知识库（domain_kb）和切片（kb_chunk）的增删改查 API。

use axum::{
    extract::{Json, Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};

use super::AppState;
use crate::adapter::inbound::http::route_adapter::RouteEntry;

/// 统一成功响应
fn ok_response<T: serde::Serialize>(data: T) -> axum::response::Response {
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({ "success": true, "data": data })),
    )
        .into_response()
}

/// 统一错误响应
fn err_response(status: axum::http::StatusCode, msg: &str) -> axum::response::Response {
    (
        status,
        Json(serde_json::json!({ "success": false, "error": msg })),
    )
        .into_response()
}

// ─── 请求体 ──────────────────────────────────────────────────

#[derive(serde::Deserialize)]
pub struct CreateKbRequest {
    pub name: String,
    pub description: Option<String>,
    /// 自定义标签数组，如 ["运动控制", "伺服"]
    pub tags: Option<Vec<String>>,
    /// 关联的专家 id（可为空字符串表示未关联专家）
    pub expert_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct UpdateKbRequest {
    pub name: String,
    pub description: Option<String>,
    /// 自定义标签数组
    pub tags: Option<Vec<String>>,
    /// 关联的专家 id
    pub expert_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct CreateChunkRequest {
    pub title: Option<String>,
    pub content: String,
}

#[derive(serde::Deserialize)]
pub struct UpdateChunkRequest {
    pub title: Option<String>,
    pub content: String,
}

// ─── Handlers ─────────────────────────────────────────────────

/// GET /subhuti/api/v1/knowledge-bases — 获取所有知识库
pub async fn list_knowledge_bases_handler(State(state): State<AppState>) -> impl IntoResponse {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg.list_knowledge_bases().await {
            Ok(kbs) => ok_response(kbs),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("查询知识库失败: {}", e),
            ),
        },
        None => err_response(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "数据库未连接（降级模式），知识库功能不可用",
        ),
    }
}

/// POST /subhuti/api/v1/knowledge-bases — 创建知识库
pub async fn create_knowledge_base_handler(
    State(state): State<AppState>,
    Json(req): Json<CreateKbRequest>,
) -> axum::response::Response {
    match state.pg_storage.as_ref() {
        Some(pg) => {
            let tags = req.tags.unwrap_or_default();
            let tags_json = serde_json::json!(tags);
            let expert_id = req.expert_id.unwrap_or_default();
            match pg
                .create_knowledge_base(
                    &req.name,
                    &req.description.unwrap_or_default(),
                    &tags_json,
                    &expert_id,
                )
                .await
            {
                Ok(kb) => (
                    axum::http::StatusCode::CREATED,
                    Json(serde_json::json!({ "success": true, "data": kb })),
                )
                    .into_response(),
                Err(e) => err_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("创建知识库失败: {}", e),
                ),
            }
        }
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// PUT /subhuti/api/v1/knowledge-bases/:id — 更新知识库
pub async fn update_knowledge_base_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateKbRequest>,
) -> axum::response::Response {
    match state.pg_storage.as_ref() {
        Some(pg) => {
            let tags = req.tags.unwrap_or_default();
            let tags_json = serde_json::json!(tags);
            let expert_id = req.expert_id.unwrap_or_default();
            match pg
                .update_knowledge_base(
                    &id,
                    &req.name,
                    &req.description.unwrap_or_default(),
                    &tags_json,
                    &expert_id,
                )
                .await
            {
                Ok(kb) => ok_response(kb),
                Err(e) => err_response(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("更新知识库失败: {}", e),
                ),
            }
        }
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// DELETE /subhuti/api/v1/knowledge-bases/:id — 删除知识库
pub async fn delete_knowledge_base_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg.delete_knowledge_base(&id).await {
            Ok(_) => ok_response(serde_json::json!({ "deleted": true })),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("删除知识库失败: {}", e),
            ),
        },
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// GET /subhuti/api/v1/knowledge-bases/:id/chunks — 获取切片列表
pub async fn list_chunks_handler(
    State(state): State<AppState>,
    Path(kb_id): Path<String>,
) -> impl IntoResponse {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg.list_chunks(&kb_id).await {
            Ok(chunks) => ok_response(chunks),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("查询切片失败: {}", e),
            ),
        },
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// POST /subhuti/api/v1/knowledge-bases/:id/chunks — 创建切片
pub async fn create_chunk_handler(
    State(state): State<AppState>,
    Path(kb_id): Path<String>,
    Json(req): Json<CreateChunkRequest>,
) -> axum::response::Response {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg
            .create_chunk(&kb_id, &req.title.unwrap_or_default(), &req.content)
            .await
        {
            Ok(chunk) => (
                axum::http::StatusCode::CREATED,
                Json(serde_json::json!({ "success": true, "data": chunk })),
            )
                .into_response(),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("创建切片失败: {}", e),
            ),
        },
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// PUT /subhuti/api/v1/chunks/:id — 更新切片
pub async fn update_chunk_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateChunkRequest>,
) -> impl IntoResponse {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg
            .update_chunk(&id, &req.title.unwrap_or_default(), &req.content)
            .await
        {
            Ok(chunk) => ok_response(chunk),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("更新切片失败: {}", e),
            ),
        },
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

/// DELETE /subhuti/api/v1/chunks/:id — 删除切片
pub async fn delete_chunk_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.pg_storage.as_ref() {
        Some(pg) => match pg.delete_chunk(&id).await {
            Ok(_) => ok_response(serde_json::json!({ "deleted": true })),
            Err(e) => err_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                &format!("删除切片失败: {}", e),
            ),
        },
        None => err_response(axum::http::StatusCode::SERVICE_UNAVAILABLE, "数据库未连接"),
    }
}

// ─── 路由注册 ─────────────────────────────────────────────────

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases", get(list_knowledge_bases_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases", post(create_knowledge_base_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases/:id",
        method: "PUT",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases/:id", put(update_knowledge_base_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases/:id",
        method: "DELETE",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases/:id", delete(delete_knowledge_base_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases/:id/chunks",
        method: "GET",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases/:id/chunks", get(list_chunks_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/knowledge-bases/:id/chunks",
        method: "POST",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/knowledge-bases/:id/chunks", post(create_chunk_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/chunks/:id",
        method: "PUT",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/chunks/:id", put(update_chunk_handler)),
    }
}

inventory::submit! {
    RouteEntry {
        path: "/subhuti/api/v1/chunks/:id",
        method: "DELETE",
        trace_enabled: false,
        register: |r| r.route("/subhuti/api/v1/chunks/:id", delete(delete_chunk_handler)),
    }
}
