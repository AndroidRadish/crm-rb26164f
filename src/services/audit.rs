//! 审计日志服务

use axum::http::HeaderMap;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::services::auth;

pub async fn log_from_headers(
    pool: &PgPool,
    headers: &HeaderMap,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    detail: &str,
) {
    let (tenant_id, user_id, username) = match auth::extract_bearer_token(headers) {
        Some(token) => match auth::validate_token(&token) {
            Ok(claims) => {
                let uid = Uuid::parse_str(&claims.sub).ok();
                let tid = Uuid::parse_str(&claims.tenant_id).ok();
                (tid, uid, claims.username)
            }
            Err(_) => (None, None, "unknown".into()),
        },
        None => (None, None, "anonymous".into()),
    };
    log(
        pool,
        tenant_id,
        user_id,
        &username,
        action,
        entity_type,
        entity_id,
        detail,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub async fn log(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    user_id: Option<Uuid>,
    username: &str,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    detail: &str,
) {
    sqlx::query(
        "INSERT INTO audit_logs (tenant_id, user_id, username, action, entity_type, entity_id, detail) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(username)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(detail)
    .execute(pool)
    .await
    .ok();
}

pub async fn list(pool: &PgPool, tenant_id: Uuid, limit: i64) -> Vec<serde_json::Value> {
    let rows = sqlx::query(
        "SELECT user_id, username, action, entity_type, entity_id, detail, created_at \
         FROM audit_logs WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT $2",
    )
    .bind(tenant_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.iter()
        .map(|r| {
            serde_json::json!({
                "user_id": r.try_get::<Option<Uuid>, _>(0).unwrap_or_default(),
                "username": r.try_get::<String, _>(1).unwrap_or_default(),
                "action": r.try_get::<String, _>(2).unwrap_or_default(),
                "entity_type": r.try_get::<String, _>(3).unwrap_or_default(),
                "entity_id": r.try_get::<String, _>(4).unwrap_or_default(),
                "detail": r.try_get::<String, _>(5).unwrap_or_default(),
                "created_at": r.try_get::<String, _>(6).unwrap_or_default(),
            })
        })
        .collect()
}
