//! 待办任务 handler

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::task::{CreateTaskRequest, Task, UpdateTaskRequest};
use crate::services::{audit, auth};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/tasks — 我的待办列表
pub async fn list(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    match sqlx::query_as::<_, Task>(
        "SELECT t.id, t.tenant_id, t.customer_id, t.user_id, t.title, t.due_at, t.status, t.reminder_channel, t.created_at \
         FROM tasks t WHERE t.tenant_id = $1 AND t.user_id = $2 \
         ORDER BY t.due_at ASC",
    )
    .bind(user.tenant_id)
    .bind(user.user_id)
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

/// GET /api/customers/:id/tasks — 客户相关待办
pub async fn list_by_customer(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

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

    match sqlx::query_as::<_, Task>(
        "SELECT t.id, t.tenant_id, t.customer_id, t.user_id, t.title, t.due_at, t.status, t.reminder_channel, t.created_at \
         FROM tasks t WHERE t.customer_id = $1 AND t.tenant_id = $2 \
         ORDER BY t.due_at ASC",
    )
    .bind(customer_id)
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

/// POST /api/customers/:id/tasks — 创建待办
pub async fn create(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
    Json(req): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

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

    let reminder = req
        .reminder_channel
        .unwrap_or(crate::models::task::ReminderChannel::System);

    let result = sqlx::query_as::<_, Task>(
        "INSERT INTO tasks (tenant_id, customer_id, user_id, title, due_at, reminder_channel) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, tenant_id, customer_id, user_id, title, due_at, status, reminder_channel, created_at",
    )
    .bind(user.tenant_id)
    .bind(customer_id)
    .bind(user.user_id)
    .bind(&req.title)
    .bind(req.due_at)
    .bind(&reminder)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(task) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "task.create",
                "task",
                &task.id.to_string(),
                &task.title,
            )
            .await;

            // 企微/钉钉 webhook 提醒
            let channel_str = match &reminder {
                crate::models::task::ReminderChannel::Wecom => "wecom",
                crate::models::task::ReminderChannel::Dingtalk => "dingtalk",
                _ => "system",
            };
            if channel_str != "system" {
                let title = task.title.clone();
                let due = task.due_at.to_rfc3339();
                let _ = crate::services::webhook::send_reminder(
                    channel_str,
                    &title,
                    &format!("截止时间：{}", due),
                )
                .await;
            }

            (StatusCode::CREATED, Json(task)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/tasks/:id — 更新待办（状态/标题/截止时间）
pub async fn update(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTaskRequest>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let mut sets = Vec::new();
    let mut idx = 1;

    if req.title.is_some() {
        sets.push(format!("title = ${}", idx));
        idx += 1;
    }
    if req.due_at.is_some() {
        sets.push(format!("due_at = ${}", idx));
        idx += 1;
    }
    if req.status.is_some() {
        sets.push(format!("status = ${}", idx));
        idx += 1;
    }
    if req.reminder_channel.is_some() {
        sets.push(format!("reminder_channel = ${}", idx));
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
        "UPDATE tasks SET {} WHERE id = ${} AND tenant_id = ${} AND user_id = ${} \
         RETURNING id, tenant_id, customer_id, user_id, title, due_at, status, reminder_channel, created_at",
        sets.join(", "),
        idx,
        idx + 1,
        idx + 2
    );

    let mut q = sqlx::query_as::<_, Task>(&sql);
    if let Some(ref v) = req.title {
        q = q.bind(v);
    }
    if let Some(ref v) = req.due_at {
        q = q.bind(v);
    }
    if let Some(ref v) = req.status {
        q = q.bind(v);
    }
    if let Some(ref v) = req.reminder_channel {
        q = q.bind(v);
    }
    q = q.bind(id).bind(user.tenant_id).bind(user.user_id);

    match q.fetch_optional(&pool).await {
        Ok(Some(task)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "task.update",
                "task",
                &id.to_string(),
                &task.title,
            )
            .await;
            (StatusCode::OK, Json(task)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Task not found or not yours" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/tasks/:id — 删除待办
pub async fn delete(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let result = sqlx::query("DELETE FROM tasks WHERE id = $1 AND tenant_id = $2 AND user_id = $3")
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
        .execute(&pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            audit::log_from_headers(&pool, &headers, "task.delete", "task", &id.to_string(), "")
                .await;
            (
                StatusCode::NO_CONTENT,
                Json(serde_json::json!({ "ok": true })),
            )
                .into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Task not found or not yours" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
