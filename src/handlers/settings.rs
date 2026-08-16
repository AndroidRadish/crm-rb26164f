//! settings.rs — 租户级设置（owner 可读写）

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use sqlx::PgPool;

use crate::services::{audit, auth};

/// 允许的配置键白名单（防止乱写）
const KEYS: &[&str] = &["wecom_webhook_url", "dingtalk_webhook_url"];

pub async fn get_settings(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response()
        }
    };
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE tenant_id = $1",
    )
    .bind(user.tenant_id)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    let map: serde_json::Map<String, serde_json::Value> = rows
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    (StatusCode::OK, Json(serde_json::Value::Object(map))).into_response()
}

pub async fn put_settings(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "unauthorized"}))).into_response()
        }
    };
    if !auth::is_admin(&user) {
        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "仅管理员可改设置"}))).into_response();
    }
    let Some(obj) = body.as_object() else {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "需要 JSON 对象"}))).into_response();
    };
    for (k, v) in obj {
        if !KEYS.contains(&k.as_str()) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": format!("不支持的键: {k}")}))).into_response();
        }
        let val = v.as_str().unwrap_or("");
        sqlx::query(
            "INSERT INTO settings (tenant_id, key, value, updated_at) VALUES ($1,$2,$3,now()) \
             ON CONFLICT (tenant_id, key) DO UPDATE SET value = $3, updated_at = now()",
        )
        .bind(user.tenant_id)
        .bind(k)
        .bind(val)
        .execute(&pool)
        .await
        .ok();
    }
    audit::log_from_headers(&pool, &headers, "settings.update", "settings", "", "更新提醒配置").await;
    (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response()
}

/// 读单个配置（服务内部用）：DB 优先，环境变量兜底
pub async fn get_setting(pool: &PgPool, tenant_id: uuid::Uuid, key: &str) -> Option<String> {
    if let Ok(Some(v)) = sqlx::query_scalar::<_, String>(
        "SELECT value FROM settings WHERE tenant_id = $1 AND key = $2",
    )
    .bind(tenant_id)
    .bind(key)
    .fetch_optional(pool)
    .await
    {
        if !v.is_empty() {
            return Some(v);
        }
    }
    std::env::var(key.to_uppercase()).ok().filter(|s| !s.is_empty())
}
