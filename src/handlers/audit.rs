//! 审计日志 handler（管理员查看）

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::services::{audit, auth};

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    100
}

/// GET /api/audit — 管理员查看审计日志
pub async fn list(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
            )
                .into_response();
        }
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can view audit logs" })),
        )
            .into_response();
    }

    let logs = audit::list(&pool, user.tenant_id, query.limit).await;
    (StatusCode::OK, Json(logs)).into_response()
}
