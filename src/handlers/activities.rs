//! 跟进记录 handler（时间轴）

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::activity::{Activity, CreateActivityRequest};
use crate::services::{audit, auth, feature};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/customers/:id/activities — 客户跟进时间轴
pub async fn list(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：跟进记录/时间线属基础档（crm.activities）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.activities").await {
        return resp;
    }
    let scope = auth::scope_where(&user, "c");

    // 先验证客户在 scope 内
    let exists: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c WHERE c.id = $1 AND {}",
        scope
    ))
    .bind(customer_id)
    .fetch_one(&pool)
    .await
    {
        Ok(n) => n,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    if exists == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Customer not found or not in your scope" })),
        )
            .into_response();
    }

    // 时间线附原作者名（author_name）：换人接手后可追溯每条记录是谁写的（spec §2.2）
    match sqlx::query(
        "SELECT a.id, a.tenant_id, a.customer_id, a.user_id, a.type, a.content, a.occurred_at, a.created_at, \
         COALESCE(u.display_name, u.username) AS author_name \
         FROM activities a LEFT JOIN users u ON u.id = a.user_id \
         WHERE a.customer_id = $1 AND a.tenant_id = $2 \
         ORDER BY a.occurred_at DESC",
    )
    .bind(customer_id)
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => {
            use sqlx::Row;
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "id": r.get::<Uuid, _>("id"),
                        "tenant_id": r.get::<Uuid, _>("tenant_id"),
                        "customer_id": r.get::<Uuid, _>("customer_id"),
                        "user_id": r.get::<Uuid, _>("user_id"),
                        "type": r.get::<crate::models::activity::ActivityType, _>("type"),
                        "content": r.get::<String, _>("content"),
                        "occurred_at": r.get::<chrono::DateTime<chrono::Utc>, _>("occurred_at"),
                        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
                        "author_name": r.get::<Option<String>, _>("author_name"),
                    })
                })
                .collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/customers/:id/activities — 添加跟进记录
pub async fn create(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
    Json(req): Json<CreateActivityRequest>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：跟进记录/时间线属基础档（crm.activities）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.activities").await {
        return resp;
    }
    let scope = auth::scope_where(&user, "c");

    // 验证客户在 scope 内
    let exists: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c WHERE c.id = $1 AND {}",
        scope
    ))
    .bind(customer_id)
    .fetch_one(&pool)
    .await
    {
        Ok(n) => n,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };
    if exists == 0 {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Customer not found or not in your scope" })),
        )
            .into_response();
    }

    let occurred_at = req.occurred_at.unwrap_or_else(chrono::Utc::now);

    let result = sqlx::query_as::<_, Activity>(
        "INSERT INTO activities (tenant_id, customer_id, user_id, type, content, occurred_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, tenant_id, customer_id, user_id, type, content, occurred_at, created_at",
    )
    .bind(user.tenant_id)
    .bind(customer_id)
    .bind(user.user_id)
    .bind(&req.r#type)
    .bind(&req.content)
    .bind(occurred_at)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(activity) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "activity.create",
                "activity",
                &activity.id.to_string(),
                &activity.content,
            )
            .await;
            (StatusCode::CREATED, Json(activity)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
