//! 销售阶段 handler（管理员自定义）

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::stage::{CreateStageRequest, Stage, UpdateStageRequest};
use crate::services::{audit, auth};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/stages — 列出销售阶段
pub async fn list(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    match sqlx::query_as::<_, Stage>(
        "SELECT s.id, s.tenant_id, s.name, s.order_index, s.is_won, s.is_lost, s.is_active, s.created_at \
         FROM stages s WHERE s.tenant_id = $1 AND s.is_active = true \
         ORDER BY s.order_index ASC",
    )
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/stages — 创建阶段（管理员）
pub async fn create(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(req): Json<CreateStageRequest>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can manage stages" })),
        )
            .into_response();
    }

    let order_index = match req.order_index {
        Some(v) => v,
        None => {
            // 默认追加到末尾
            sqlx::query_scalar::<_, i32>(
                "SELECT COALESCE(MAX(order_index), -1) + 1 FROM stages WHERE tenant_id = $1",
            )
            .bind(user.tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap_or(0)
        }
    };

    let result = sqlx::query_as::<_, Stage>(
        "INSERT INTO stages (tenant_id, name, order_index, is_won, is_lost) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, tenant_id, name, order_index, is_won, is_lost, is_active, created_at",
    )
    .bind(user.tenant_id)
    .bind(&req.name)
    .bind(order_index)
    .bind(req.is_won.unwrap_or(false))
    .bind(req.is_lost.unwrap_or(false))
    .fetch_one(&pool)
    .await;

    match result {
        Ok(stage) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "stage.create",
                "stage",
                &stage.id.to_string(),
                &stage.name,
            )
            .await;
            (StatusCode::CREATED, Json(stage)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/stages/:id — 更新阶段（管理员）
pub async fn update(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateStageRequest>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can manage stages" })),
        )
            .into_response();
    }

    let mut sets = Vec::new();
    let mut idx = 1;

    if req.name.is_some() {
        sets.push(format!("name = ${}", idx));
        idx += 1;
    }
    if req.order_index.is_some() {
        sets.push(format!("order_index = ${}", idx));
        idx += 1;
    }
    if req.is_won.is_some() {
        sets.push(format!("is_won = ${}", idx));
        idx += 1;
    }
    if req.is_lost.is_some() {
        sets.push(format!("is_lost = ${}", idx));
        idx += 1;
    }
    if req.is_active.is_some() {
        sets.push(format!("is_active = ${}", idx));
        idx += 1;
    }

    if sets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Nothing to update" })),
        )
            .into_response();
    }

    let sql = format!(
        "UPDATE stages SET {} WHERE id = ${} AND tenant_id = ${} \
         RETURNING id, tenant_id, name, order_index, is_won, is_lost, is_active, created_at",
        sets.join(", "),
        idx,
        idx + 1
    );

    let mut q = sqlx::query_as::<_, Stage>(&sql);
    if let Some(ref v) = req.name {
        q = q.bind(v);
    }
    if let Some(ref v) = req.order_index {
        q = q.bind(v);
    }
    if let Some(ref v) = req.is_won {
        q = q.bind(v);
    }
    if let Some(ref v) = req.is_lost {
        q = q.bind(v);
    }
    if let Some(ref v) = req.is_active {
        q = q.bind(v);
    }
    q = q.bind(id).bind(user.tenant_id);

    match q.fetch_optional(&pool).await {
        Ok(Some(stage)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "stage.update",
                "stage",
                &id.to_string(),
                &stage.name,
            )
            .await;
            (StatusCode::OK, Json(stage)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Stage not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/stages/:id — 删除阶段（管理员，软删除）
pub async fn delete(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can manage stages" })),
        )
            .into_response();
    }

    // 软删除：is_active = false
    let result =
        sqlx::query("UPDATE stages SET is_active = false WHERE id = $1 AND tenant_id = $2")
            .bind(id)
            .bind(user.tenant_id)
            .execute(&pool)
            .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            audit::log_from_headers(
                &pool,
                &headers,
                "stage.delete",
                "stage",
                &id.to_string(),
                "",
            )
            .await;
            (
                StatusCode::NO_CONTENT,
                Json(serde_json::json!({ "ok": true })),
            )
                .into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Stage not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
