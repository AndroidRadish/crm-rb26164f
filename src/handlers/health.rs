//! 健康检查端点（无需认证）

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use sqlx::PgPool;

/// GET /api/health — 返回服务与数据库健康状态
pub async fn health(State(pool): State<PgPool>) -> impl IntoResponse {
    let db_up = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await
        .is_ok();

    let body = serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "db": if db_up { "up" } else { "down" },
    });

    (StatusCode::OK, Json(body)).into_response()
}
