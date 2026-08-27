//! 待办任务 handler
//!
//! 数据域（RB-266FB3 §2.3）：admin（老板）看全团队任务，member（销售）仅自己的任务。
//! 跟进提醒（§2.1）：GET /api/tasks/overdue 逾期任务列表，member 自己/admin 全团队口径。

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::models::task::{CreateTaskRequest, Task, UpdateTaskRequest};
use crate::models::user::UserRole;
use crate::services::{audit, auth, feature};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// 任务行 → JSON（附客户公司名与负责人名，列表/逾期共用）
fn task_row_to_json(r: &sqlx::postgres::PgRow) -> serde_json::Value {
    serde_json::json!({
        "id": r.get::<Uuid, _>("id"),
        "tenant_id": r.get::<Uuid, _>("tenant_id"),
        "customer_id": r.get::<Uuid, _>("customer_id"),
        "user_id": r.get::<Uuid, _>("user_id"),
        "title": r.get::<String, _>("title"),
        "due_at": r.get::<chrono::DateTime<chrono::Utc>, _>("due_at"),
        "status": r.get::<crate::models::task::TaskStatus, _>("status"),
        "reminder_channel": r.get::<crate::models::task::ReminderChannel, _>("reminder_channel"),
        "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>("created_at"),
        "customer_company": r.get::<Option<String>, _>("customer_company"),
        "assignee_name": r.get::<Option<String>, _>("assignee_name"),
    })
}

const TASK_SELECT: &str = "SELECT t.id, t.tenant_id, t.customer_id, t.user_id, t.title, t.due_at, t.status, t.reminder_channel, t.created_at, \
     c.company AS customer_company, COALESCE(u.display_name, u.username) AS assignee_name \
     FROM tasks t \
     JOIN customers c ON c.id = t.customer_id \
     LEFT JOIN users u ON u.id = t.user_id";

/// GET /api/tasks — 待办列表（admin 全团队，member 仅自己）
pub async fn list(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：待办任务属基础档（crm.tasks）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.tasks").await {
        return resp;
    }

    // 数据域过滤：admin 全团队口径，member 仅自己（spec §2.3）
    let sql = match user.role {
        UserRole::Admin => format!("{} WHERE t.tenant_id = $1 ORDER BY t.due_at ASC", TASK_SELECT),
        UserRole::Member => format!(
            "{} WHERE t.tenant_id = $1 AND t.user_id = $2 ORDER BY t.due_at ASC",
            TASK_SELECT
        ),
    };
    let mut q = sqlx::query(&sql).bind(user.tenant_id);
    if user.role == UserRole::Member {
        q = q.bind(user.user_id);
    }

    match q.fetch_all(&pool).await {
        Ok(rows) => {
            let list: Vec<_> = rows.iter().map(task_row_to_json).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/tasks/overdue — 逾期任务列表（status=pending 且 due_at < now()）
/// member 自己口径 / admin 全团队口径；功能分级 crm.reminders（跟进提醒）
pub async fn overdue(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：跟进提醒（crm.reminders）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.reminders").await {
        return resp;
    }

    let sql = match user.role {
        UserRole::Admin => format!(
            "{} WHERE t.tenant_id = $1 AND t.status = 'pending' AND t.due_at < now() \
             ORDER BY t.due_at ASC",
            TASK_SELECT
        ),
        UserRole::Member => format!(
            "{} WHERE t.tenant_id = $1 AND t.user_id = $2 AND t.status = 'pending' AND t.due_at < now() \
             ORDER BY t.due_at ASC",
            TASK_SELECT
        ),
    };
    let mut q = sqlx::query(&sql).bind(user.tenant_id);
    if user.role == UserRole::Member {
        q = q.bind(user.user_id);
    }

    match q.fetch_all(&pool).await {
        Ok(rows) => {
            let list: Vec<_> = rows.iter().map(task_row_to_json).collect();
            (StatusCode::OK, Json(list)).into_response()
        }
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
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.tasks").await {
        return resp;
    }
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
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.tasks").await {
        return resp;
    }
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
                    &pool,
                    user.tenant_id,
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

/// PUT /api/tasks/:id — 更新待办（admin 可改全团队，member 仅自己的）
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
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.tasks").await {
        return resp;
    }

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

    // 数据域：admin 全团队可改，member 只能改自己的任务
    let owner_clause = match user.role {
        UserRole::Admin => String::new(),
        UserRole::Member => format!(" AND user_id = ${}", idx + 2),
    };
    let sql = format!(
        "UPDATE tasks SET {} WHERE id = ${} AND tenant_id = ${}{} \
         RETURNING id, tenant_id, customer_id, user_id, title, due_at, status, reminder_channel, created_at",
        sets.join(", "),
        idx,
        idx + 1,
        owner_clause
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
    q = q.bind(id).bind(user.tenant_id);
    if user.role == UserRole::Member {
        q = q.bind(user.user_id);
    }

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

/// DELETE /api/tasks/:id — 删除待办（admin 可删全团队，member 仅自己的）
pub async fn delete(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.tasks").await {
        return resp;
    }

    // 数据域：admin 全团队可删，member 只能删自己的任务
    let result = match user.role {
        UserRole::Admin => {
            sqlx::query("DELETE FROM tasks WHERE id = $1 AND tenant_id = $2")
                .bind(id)
                .bind(user.tenant_id)
                .execute(&pool)
                .await
        }
        UserRole::Member => {
            sqlx::query("DELETE FROM tasks WHERE id = $1 AND tenant_id = $2 AND user_id = $3")
                .bind(id)
                .bind(user.tenant_id)
                .bind(user.user_id)
                .execute(&pool)
                .await
        }
    };

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
