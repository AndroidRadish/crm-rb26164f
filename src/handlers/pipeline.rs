//! 销售管道 handler（看板 + 漏斗 + 阶段流转）

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::models::pipeline::{CreatePipelineEntryRequest, PipelineEntry};
use crate::models::user::UserRole;
use crate::services::{audit, auth};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/pipeline — 列出管道条目
pub async fn list(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    match sqlx::query_as::<_, PipelineEntry>(
        "SELECT p.id, p.tenant_id, p.customer_id, p.stage_id, p.entered_at, p.exited_at, p.amount_estimate \
         FROM pipeline_entries p WHERE p.tenant_id = $1 AND p.exited_at IS NULL \
         ORDER BY p.entered_at DESC",
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

/// POST /api/pipeline — 创建管道条目
pub async fn create(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(req): Json<CreatePipelineEntryRequest>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    // 验证客户在 scope 内
    let scope = auth::scope_where(&user, "c");
    let exists: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c WHERE c.id = $1 AND {}",
        scope
    ))
    .bind(req.customer_id)
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

    // 验证 stage 属于当前 tenant
    let stage_exists: i64 =
        match sqlx::query_scalar("SELECT COUNT(*) FROM stages WHERE id = $1 AND tenant_id = $2")
            .bind(req.stage_id)
            .bind(user.tenant_id)
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
    if stage_exists == 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Stage not found" })),
        )
            .into_response();
    }

    let result = sqlx::query_as::<_, PipelineEntry>(
        "INSERT INTO pipeline_entries (tenant_id, customer_id, stage_id, amount_estimate) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, tenant_id, customer_id, stage_id, entered_at, exited_at, amount_estimate",
    )
    .bind(user.tenant_id)
    .bind(req.customer_id)
    .bind(req.stage_id)
    .bind(req.amount_estimate)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(entry) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "pipeline.create",
                "pipeline",
                &entry.id.to_string(),
                &format!("customer={} stage={}", entry.customer_id, entry.stage_id),
            )
            .await;
            (StatusCode::CREATED, Json(entry)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/pipeline/:id — 更新管道条目（切换阶段）
pub async fn update(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let stage_id = req.get("stage_id").and_then(|v| v.as_str());
    let amount = req.get("amount_estimate").and_then(|v| v.as_f64());

    let mut sets: Vec<String> = Vec::new();
    let mut idx = 1;

    if let Some(sid) = stage_id {
        let stage_uuid = match Uuid::parse_str(sid) {
            Ok(u) => u,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "Invalid stage_id" })),
                )
                    .into_response();
            }
        };
        let stage_exists: i64 = match sqlx::query_scalar(
            "SELECT COUNT(*) FROM stages WHERE id = $1 AND tenant_id = $2",
        )
        .bind(stage_uuid)
        .bind(user.tenant_id)
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
        if stage_exists == 0 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Stage not found" })),
            )
                .into_response();
        }
        sets.push(format!("stage_id = ${}", idx));
        idx += 1;
    }

    if let Some(_a) = amount {
        sets.push(format!("amount_estimate = ${}", idx));
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
        "UPDATE pipeline_entries p SET {} WHERE p.id = ${} AND p.tenant_id = ${} \
         RETURNING p.id, p.tenant_id, p.customer_id, p.stage_id, p.entered_at, p.exited_at, p.amount_estimate",
        sets.join(", "),
        idx,
        idx + 1
    );

    let mut q = sqlx::query_as::<_, PipelineEntry>(&sql);

    if let Some(sid) = stage_id {
        let stage_uuid = Uuid::parse_str(sid).unwrap();
        q = q.bind(stage_uuid);
    }
    if let Some(a) = amount {
        q = q.bind(rust_decimal::Decimal::from_f64_retain(a).unwrap_or_default());
    }
    q = q.bind(id);
    q = q.bind(user.tenant_id);

    match q.fetch_optional(&pool).await {
        Ok(Some(entry)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "pipeline.update",
                "pipeline",
                &id.to_string(),
                &format!("stage={}", entry.stage_id),
            )
            .await;
            (StatusCode::OK, Json(entry)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Pipeline entry not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
/// DELETE /api/pipeline/:id — 删除管道条目
pub async fn delete(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let result = sqlx::query("DELETE FROM pipeline_entries WHERE id = $1 AND tenant_id = $2")
        .bind(id)
        .bind(user.tenant_id)
        .execute(&pool)
        .await;

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            audit::log_from_headers(
                &pool,
                &headers,
                "pipeline.delete",
                "pipeline",
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
            Json(serde_json::json!({ "error": "Pipeline entry not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/pipeline/kanban — 看板视图（按阶段分组）
pub async fn kanban(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    // 获取所有阶段
    let stages = match sqlx::query_as::<_, crate::models::stage::Stage>(
        "SELECT s.id, s.tenant_id, s.name, s.order_index, s.is_won, s.is_lost, s.is_active, s.created_at \
         FROM stages s WHERE s.tenant_id = $1 AND s.is_active = true ORDER BY s.order_index ASC",
    )
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // 获取看板可见客户（含阶段信息）
    // 管理员：全部客户；成员：自己名下 + 公海池（owner_id IS NULL，可认领）
    let scope = match user.role {
        UserRole::Admin => {
            format!("c.tenant_id = '{}'", user.tenant_id)
        }
        UserRole::Member => {
            format!(
                "c.tenant_id = '{}' AND (c.owner_id = '{}' OR c.owner_id IS NULL)",
                user.tenant_id, user.user_id
            )
        }
    };
    // 查询客户（含阶段信息）
    let customers = match sqlx::query_as::<_, crate::models::customer::Customer>(&format!(
        "SELECT c.id, c.tenant_id, c.company, c.contact_name, c.phone, c.email, c.address, \
         c.notes, c.owner_id, c.stage, c.source, c.duplicate_of, c.created_at, c.updated_at \
         FROM customers c WHERE {} AND c.stage IS NOT NULL AND c.duplicate_of IS NULL",
        scope
    ))
    .fetch_all(&pool)
    .await
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // 查询负责人 display_name 映射（owner_id -> display_name）
    let owner_names: std::collections::HashMap<Uuid, String> = match sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id, COALESCE(display_name, username) FROM users WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows.into_iter().collect(),
        Err(_) => std::collections::HashMap::new(),
    };

    // 将客户转换为 JSON（附加 owner_name）
    let customers_json: Vec<serde_json::Value> = customers
        .iter()
        .map(|c| {
            let owner_name = c.owner_id.and_then(|oid| owner_names.get(&oid).cloned());
            serde_json::json!({
                "id": c.id,
                "tenant_id": c.tenant_id,
                "company": c.company,
                "contact_name": c.contact_name,
                "phone": c.phone,
                "email": c.email,
                "address": c.address,
                "notes": c.notes,
                "owner_id": c.owner_id,
                "owner_name": owner_name,
                "stage": c.stage,
                "source": c.source,
                "duplicate_of": c.duplicate_of,
                "created_at": c.created_at,
                "updated_at": c.updated_at,
            })
        })
        .collect();

    // 按阶段分组
    let mut columns: Vec<serde_json::Value> = Vec::new();
    for stage in &stages {
        let stage_customers: Vec<&serde_json::Value> = customers_json
            .iter()
            .filter(|c| {
                c.get("stage")
                    .and_then(|s| s.as_str())
                    .map(|s| s == stage.id.to_string().as_str())
                    .unwrap_or(false)
            })
            .collect();
        columns.push(serde_json::json!({
            "stage_id": stage.id,
            "stage_name": stage.name,
            "order_index": stage.order_index,
            "is_won": stage.is_won,
            "is_lost": stage.is_lost,
            "customers": stage_customers,
        }));
    }

    (StatusCode::OK, Json(columns)).into_response()
}

/// GET /api/pipeline/funnel — 漏斗分析
pub async fn funnel(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let scope = auth::scope_where(&user, "c");

    // 各阶段客户数
    let rows = match sqlx::query(&format!(
        "SELECT s.id, s.name, s.order_index, COUNT(c.id) as cnt \
         FROM stages s \
         LEFT JOIN customers c ON c.stage = s.id AND c.duplicate_of IS NULL AND {} \
         WHERE s.tenant_id = $1 AND s.is_active = true \
         GROUP BY s.id, s.name, s.order_index \
         ORDER BY s.order_index ASC",
        scope
    ))
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let mut funnel: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let stage_id: Uuid = row.try_get("id").unwrap_or_default();
        let name: String = row.try_get("name").unwrap_or_default();
        let order_index: i32 = row.try_get("order_index").unwrap_or(0);
        let cnt: i64 = row.try_get("cnt").unwrap_or(0);
        funnel.push(serde_json::json!({
            "stage_id": stage_id,
            "name": name,
            "order_index": order_index,
            "count": cnt,
        }));
    }

    (StatusCode::OK, Json(funnel)).into_response()
}

/// GET /api/dashboard — 数据看板（个人/团队业绩）
pub async fn dashboard(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let scope = auth::scope_where(&user, "c");

    // 客户总数
    let total_customers: i64 =
        match sqlx::query_scalar(&format!("SELECT COUNT(*) FROM customers c WHERE {}", scope))
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

    // 跟进次数
    let total_activities: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM activities a \
         JOIN customers c ON a.customer_id = c.id \
         WHERE {}",
        scope
    ))
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

    // 赢单数（is_won 阶段）
    let won_count: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c \
         JOIN stages s ON c.stage = s.id \
         WHERE {} AND s.is_won = true",
        scope
    ))
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

    // 输单数（is_lost 阶段）
    let lost_count: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c \
         JOIN stages s ON c.stage = s.id \
         WHERE {} AND s.is_lost = true",
        scope
    ))
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

    // 待办数
    let pending_tasks: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE user_id = $1 AND status = 'pending'",
    )
    .bind(user.user_id)
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

    let win_rate = if total_customers > 0 {
        (won_count as f64) / (total_customers as f64) * 100.0
    } else {
        0.0
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "total_customers": total_customers,
            "total_activities": total_activities,
            "won_count": won_count,
            "lost_count": lost_count,
            "win_rate": win_rate,
            "pending_tasks": pending_tasks,
        })),
    )
        .into_response()
}
