//! entitlements handler：租户授权清单查询 + owner（工作室管理终端）功能包配置
//!
//! owner 与租户 admin 是两条正交的身份路径：
//! - /api/entitlements            租户内用户（admin/member）查自己租户的授权清单
//! - /api/admin/tenants[...]      owner 令牌（OWNER_TOKEN）配置任意租户的功能包
//! 租户 admin 不得自改授权（否则分级失去意义），owner 端点只认 is_owner。

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::services::{audit, auth, feature};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

fn owner_forbidden() -> impl IntoResponse {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": "仅工作室管理终端（owner）可配置功能包" })),
    )
}

/// GET /api/entitlements — 当前租户的授权清单（默认档计算后结果）
pub async fn list_mine(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    match feature::list_for_tenant(&pool, user.tenant_id).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/admin/tenants — owner 租户列表（功能包配置页用）
pub async fn admin_list_tenants(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth::is_owner(&headers) {
        return owner_forbidden().into_response();
    }
    match sqlx::query_as::<_, crate::models::user::Tenant>(
        "SELECT id, name, code, status, created_at FROM tenants ORDER BY created_at",
    )
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

/// 校验目标租户存在；不存在返回 404 Response，DB 错误返回 500（不静默吞错）
async fn ensure_tenant(pool: &PgPool, tenant_id: Uuid) -> Result<(), axum::response::Response> {
    let exists: bool = match sqlx::query_scalar("SELECT COUNT(*) > 0 FROM tenants WHERE id = $1")
        .bind(tenant_id)
        .fetch_one(pool)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response());
        }
    };
    if exists {
        Ok(())
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Tenant not found" })),
        )
            .into_response())
    }
}

/// GET /api/admin/tenants/:id/entitlements — owner 查看某租户授权清单（含默认档计算）
pub async fn admin_get_entitlements(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(tenant_id): Path<Uuid>,
) -> impl IntoResponse {
    if !auth::is_owner(&headers) {
        return owner_forbidden().into_response();
    }
    if let Err(resp) = ensure_tenant(&pool, tenant_id).await {
        return resp;
    }
    match feature::list_for_tenant(&pool, tenant_id).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// PUT /api/admin/tenants/:id/entitlements — owner 配置功能包
/// body：{"pool.claim": false, "import.export": true, ...}（扁平对象，值必须布尔）
/// 未登记键整体拒绝（登记纪律）；每次变更进审计日志。
pub async fn admin_put_entitlements(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if !auth::is_owner(&headers) {
        return owner_forbidden().into_response();
    }
    if let Err(resp) = ensure_tenant(&pool, tenant_id).await {
        return resp;
    }
    let Some(obj) = body.as_object() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "需要 JSON 对象：{feature_key: bool}" })),
        )
            .into_response();
    };
    if obj.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "没有任何变更项" })),
        )
            .into_response();
    }
    for (k, v) in obj {
        if !v.is_boolean() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": format!("键 {k} 的值必须是布尔") })),
            )
                .into_response();
        }
    }

    let mut changes: Vec<String> = Vec::new();
    for (k, v) in obj {
        let enabled = v.as_bool().unwrap_or(false); // 上面已校验 is_boolean，此处不会触发
        match feature::set_entitlement(&pool, tenant_id, k, enabled).await {
            Ok(previous) => {
                let before = previous
                    .map(|b| b.to_string())
                    .unwrap_or_else(|| "默认档".to_string());
                changes.push(format!("{k}: {before} → {enabled}"));
            }
            Err(e) => {
                // 未登记键或 DB 错误：整体拒绝并说明（登记纪律）
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e.to_string(), "feature": k })),
                )
                    .into_response();
            }
        }
    }

    audit::log(
        &pool,
        Some(tenant_id),
        None,
        "owner",
        "entitlement.update",
        "entitlement",
        &tenant_id.to_string(),
        &changes.join("; "),
    )
    .await;

    match feature::list_for_tenant(&pool, tenant_id).await {
        Ok(list) => (StatusCode::OK, Json(list)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
