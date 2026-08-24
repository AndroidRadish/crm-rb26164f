//! 客户 handler：CRUD + 防撞单 + 公海池认领/释放

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::customer::{
    CreateCustomerRequest, Customer, DuplicateCheckResult, UpdateCustomerRequest,
};
use crate::models::user::UserRole;
use crate::services::{audit, auth, feature};

#[derive(Debug, Deserialize)]
pub struct CustomerQuery {
    pub search: Option<String>,
    pub stage: Option<Uuid>,
    pub owner: Option<String>, // "me" | "pool" | "all"
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/customers — 客户列表（带搜索/筛选/分页）
pub async fn list(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(query): Query<CustomerQuery>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * page_size;

    let mut sql = format!(
        "SELECT c.id, c.tenant_id, c.company, c.contact_name, c.phone, c.email, c.address, \
         c.notes, c.owner_id, c.stage, c.source, c.duplicate_of, c.created_at, c.updated_at \
         FROM customers c WHERE {}",
        scope
    );

    // 搜索：公司名/联系人/电话模糊匹配
    if let Some(ref search) = query.search {
        if !search.is_empty() {
            sql.push_str(&format!(
                " AND (c.company ILIKE '%{}%' OR c.contact_name ILIKE '%{}%' OR c.phone ILIKE '%{}%')",
                search.replace('\'', "''"),
                search.replace('\'', "''"),
                search.replace('\'', "''"),
            ));
        }
    }

    // 阶段筛选
    if let Some(ref stage) = query.stage {
        sql.push_str(&format!(" AND c.stage = '{}'", stage));
    }

    // 归属筛选
    match query.owner.as_deref() {
        Some("pool") => sql.push_str(" AND c.owner_id IS NULL"),
        Some("me") if user.role == UserRole::Member => {
            sql.push_str(&format!(" AND c.owner_id = '{}'", user.user_id));
        }
        _ => {}
    }

    sql.push_str(" ORDER BY c.updated_at DESC");
    sql.push_str(&format!(" LIMIT {} OFFSET {}", page_size, offset));

    match sqlx::query_as::<_, Customer>(&sql).fetch_all(&pool).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/customers — 创建客户（含防撞单检测）
pub async fn create(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(req): Json<CreateCustomerRequest>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    // 防撞单：按公司名或电话模糊匹配
    let dup_check = check_duplicate(&pool, &user, &req.company, req.phone.as_deref()).await;
    if let Ok(Some(dups)) = dup_check {
        if !dups.is_empty() {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "Duplicate customer detected",
                    "duplicates": dups,
                })),
            )
                .into_response();
        }
    }

    // 创建客户：member 自动归属自己，admin 可指定归属（默认自己）
    let owner_id = match user.role {
        UserRole::Admin => Some(user.user_id),
        UserRole::Member => Some(user.user_id),
    };

    let result = sqlx::query_as::<_, Customer>(
        "INSERT INTO customers (tenant_id, company, contact_name, phone, email, address, notes, owner_id, stage, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'manual') \
         RETURNING id, tenant_id, company, contact_name, phone, email, address, notes, owner_id, stage, source, duplicate_of, created_at, updated_at",
    )
    .bind(user.tenant_id)
    .bind(&req.company)
    .bind(&req.contact_name)
    .bind(&req.phone)
    .bind(&req.email)
    .bind(&req.address)
    .bind(&req.notes)
    .bind(owner_id)
    .bind(req.stage)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(customer) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "customer.create",
                "customer",
                &customer.id.to_string(),
                &customer.company,
            )
            .await;
            (StatusCode::CREATED, Json(customer)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/customers/:id — 客户详情
pub async fn get(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

    let sql = format!(
        "SELECT c.id, c.tenant_id, c.company, c.contact_name, c.phone, c.email, c.address, \
         c.notes, c.owner_id, c.stage, c.source, c.duplicate_of, c.created_at, c.updated_at \
         FROM customers c WHERE c.id = $1 AND {}",
        scope
    );

    match sqlx::query_as::<_, Customer>(&sql)
        .bind(id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(customer)) => (StatusCode::OK, Json(customer)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Customer not found or not in your scope" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/customers/:id — 更新客户
pub async fn update(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateCustomerRequest>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

    // 先验证客户在 scope 内
    let exists: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c WHERE c.id = $1 AND {}",
        scope
    ))
    .bind(id)
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

    // 动态构建 UPDATE
    let mut sets: Vec<String> = Vec::new();
    let mut idx = 1;

    macro_rules! push_set {
        ($field:expr) => {
            sets.push(format!("{} = ${}", $field, idx));
            idx += 1;
        };
    }

    if req.company.is_some() {
        push_set!("company");
    }
    if req.contact_name.is_some() {
        push_set!("contact_name");
    }
    if req.phone.is_some() {
        push_set!("phone");
    }
    if req.email.is_some() {
        push_set!("email");
    }
    if req.address.is_some() {
        push_set!("address");
    }
    if req.notes.is_some() {
        push_set!("notes");
    }
    if req.stage.is_some() {
        push_set!("stage");
    }

    if sets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Nothing to update" })),
        )
            .into_response();
    }

    sets.push("updated_at = now()".to_string());
    let sql = format!(
        "UPDATE customers c SET {} WHERE c.id = ${} AND {} \
         RETURNING c.id, c.tenant_id, c.company, c.contact_name, c.phone, c.email, c.address, \
         c.notes, c.owner_id, c.stage, c.source, c.duplicate_of, c.created_at, c.updated_at",
        sets.join(", "),
        idx,
        scope
    );

    let mut q = sqlx::query_as::<_, Customer>(&sql);

    if let Some(ref v) = req.company {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.contact_name {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.phone {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.email {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.address {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.notes {
        q = q.bind(v.clone());
    }
    if let Some(ref v) = req.stage {
        q = q.bind(*v);
    }
    q = q.bind(id);

    match q.fetch_optional(&pool).await {
        Ok(Some(customer)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "customer.update",
                "customer",
                &id.to_string(),
                &customer.company,
            )
            .await;
            (StatusCode::OK, Json(customer)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Customer not found or not in your scope" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// DELETE /api/customers/:id — 删除客户
pub async fn delete(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    let scope = auth::scope_where(&user, "c");

    let sql = format!("DELETE FROM customers c WHERE c.id = $1 AND {}", scope);
    match sqlx::query(&sql).bind(id).execute(&pool).await {
        Ok(res) if res.rows_affected() > 0 => {
            audit::log_from_headers(
                &pool,
                &headers,
                "customer.delete",
                "customer",
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
            Json(serde_json::json!({ "error": "Customer not found or not in your scope" })),
        )
            .into_response(),
        Err(e) => {
            if e.to_string().contains("foreign key") {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": "Customer has related records, cannot delete" })),
                )
                    .into_response()
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response()
            }
        }
    }
}

/// POST /api/customers/check-duplicate — 防撞单检测
pub async fn check_duplicate_handler(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let company = req.get("company").and_then(|v| v.as_str()).unwrap_or("");
    let phone = req.get("phone").and_then(|v| v.as_str());

    match check_duplicate(&pool, &user, company, phone).await {
        Ok(Some(dups)) => {
            let result = DuplicateCheckResult {
                is_duplicate: !dups.is_empty(),
                duplicates: dups,
            };
            (StatusCode::OK, Json(result)).into_response()
        }
        Ok(None) => (
            StatusCode::OK,
            Json(DuplicateCheckResult {
                is_duplicate: false,
                duplicates: vec![],
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/customers/:id/claim — 认领公海客户
pub async fn claim(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：公海认领/释放属专业档（pool.claim）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "pool.claim").await {
        return resp;
    }

    // 只能认领公海客户（owner_id IS NULL）
    let result = sqlx::query_as::<_, Customer>(
        "UPDATE customers SET owner_id = $1, source = 'claim', updated_at = now() \
         WHERE id = $2 AND tenant_id = $3 AND owner_id IS NULL \
         RETURNING id, tenant_id, company, contact_name, phone, email, address, notes, owner_id, stage, source, duplicate_of, created_at, updated_at",
    )
    .bind(user.user_id)
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(customer)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "customer.claim",
                "customer",
                &id.to_string(),
                &customer.company,
            )
            .await;
            (StatusCode::OK, Json(customer)).into_response()
        }
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Customer not in public pool or already claimed" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/customers/:id/release — 释放客户回公海池
pub async fn release(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：公海认领/释放属专业档（pool.claim）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "pool.claim").await {
        return resp;
    }

    // 只能释放自己名下的客户
    let result = sqlx::query_as::<_, Customer>(
        "UPDATE customers SET owner_id = NULL, updated_at = now() \
         WHERE id = $1 AND tenant_id = $2 AND owner_id = $3 \
         RETURNING id, tenant_id, company, contact_name, phone, email, address, notes, owner_id, stage, source, duplicate_of, created_at, updated_at",
    )
    .bind(id)
    .bind(user.tenant_id)
    .bind(user.user_id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(customer)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "customer.release",
                "customer",
                &id.to_string(),
                &customer.company,
            )
            .await;
            (StatusCode::OK, Json(customer)).into_response()
        }
        Ok(None) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "Customer not owned by you or not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// 防撞单检测：按公司名或电话模糊匹配
async fn check_duplicate(
    pool: &PgPool,
    user: &auth::AuthUser,
    company: &str,
    phone: Option<&str>,
) -> anyhow::Result<Option<Vec<Customer>>> {
    let mut sql = format!(
        "SELECT c.id, c.tenant_id, c.company, c.contact_name, c.phone, c.email, c.address, \
         c.notes, c.owner_id, c.stage, c.source, c.duplicate_of, c.created_at, c.updated_at \
         FROM customers c WHERE c.tenant_id = '{}' AND c.duplicate_of IS NULL",
        user.tenant_id
    );

    let mut has_condition = false;
    if !company.is_empty() {
        sql.push_str(&format!(
            " AND c.company ILIKE '%{}%'",
            company.replace('\'', "''")
        ));
        has_condition = true;
    }
    if let Some(p) = phone {
        if !p.is_empty() {
            sql.push_str(&format!(" AND c.phone = '{}'", p.replace('\'', "''")));
            has_condition = true;
        }
    }

    if !has_condition {
        return Ok(Some(vec![]));
    }

    sql.push_str(" LIMIT 10");
    let dups = sqlx::query_as::<_, Customer>(&sql).fetch_all(pool).await?;
    Ok(Some(dups))
}
