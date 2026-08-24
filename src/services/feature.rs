//! 功能分级（entitlement）服务 — 打样首单（spec: req-crm-tiering-spec.md）
//!
//! 缺省规则：entitlements 表无记录 = 按 feature_keys.tier 默认档
//! （基础档 tier=1 默认开，tier>=2 默认关）。显式记录覆盖默认档。
//! 判定失败返回 403 且必须带 feature key（前端靠它显示升级提示）。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// 某租户是否拥有某功能授权。
/// DB 出错时 fail-closed（视为无授权）并记日志——权限判定宁可误拒不可误放。
pub async fn is_enabled(pool: &PgPool, tenant_id: Uuid, key: &str) -> bool {
    let explicit: Option<bool> = match sqlx::query_scalar(
        "SELECT enabled FROM entitlements \
         WHERE scope = 'tenant' AND scope_id = $1 AND feature_key = $2",
    )
    .bind(tenant_id.to_string())
    .bind(key)
    .fetch_optional(pool)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("entitlement 查询失败（fail-closed）: tenant={} key={} err={}", tenant_id, key, e);
            return false;
        }
    };
    if let Some(enabled) = explicit {
        return enabled;
    }
    // 无记录 → 按默认档
    match sqlx::query_scalar::<_, i32>("SELECT tier FROM feature_keys WHERE key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
    {
        Ok(Some(tier)) => tier == 1,
        Ok(None) => {
            // 未登记键：登记纪律要求代码不得使用未登记键，拒绝并暴露
            tracing::warn!("使用了未登记的功能键（fail-closed）: {}", key);
            false
        }
        Err(e) => {
            tracing::warn!("feature_keys 查询失败（fail-closed）: key={} err={}", key, e);
            false
        }
    }
}

/// 端点守卫：无授权 → 403 + feature key
pub async fn require_feature(pool: &PgPool, tenant_id: Uuid, key: &str) -> Result<(), Response> {
    if is_enabled(pool, tenant_id, key).await {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "当前版本不含此功能，联系客服升级",
                "feature": key,
            })),
        )
            .into_response())
    }
}

/// 当前租户授权清单（默认档计算后的结果），供 GET /api/entitlements 与 owner 配置页使用
pub async fn list_for_tenant(pool: &PgPool, tenant_id: Uuid) -> anyhow::Result<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT f.key, f.label, f.tier, e.enabled AS explicit_enabled \
         FROM feature_keys f \
         LEFT JOIN entitlements e \
           ON e.scope = 'tenant' AND e.scope_id = $1 AND e.feature_key = f.key \
         ORDER BY f.tier, f.key",
    )
    .bind(tenant_id.to_string())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let tier: i32 = r.get("tier");
            let explicit: Option<bool> = r.get("explicit_enabled");
            serde_json::json!({
                "key": r.get::<String, _>("key"),
                "label": r.get::<String, _>("label"),
                "tier": tier,
                "enabled": explicit.unwrap_or(tier == 1),
                "source": if explicit.is_some() { "entitlement" } else { "default" },
            })
        })
        .collect())
}

/// 写授权（owner 配置）。返回变更前的显式值（None = 此前按默认档），供审计明细。
/// 未登记键直接报错（登记纪律）。
pub async fn set_entitlement(
    pool: &PgPool,
    tenant_id: Uuid,
    key: &str,
    enabled: bool,
) -> anyhow::Result<Option<bool>> {
    let registered: bool = sqlx::query_scalar("SELECT COUNT(*) > 0 FROM feature_keys WHERE key = $1")
        .bind(key)
        .fetch_one(pool)
        .await?;
    if !registered {
        anyhow::bail!("未登记的功能键: {key}");
    }
    let previous: Option<bool> = sqlx::query_scalar(
        "SELECT enabled FROM entitlements \
         WHERE scope = 'tenant' AND scope_id = $1 AND feature_key = $2",
    )
    .bind(tenant_id.to_string())
    .bind(key)
    .fetch_optional(pool)
    .await?;
    sqlx::query(
        "INSERT INTO entitlements (scope, scope_id, feature_key, enabled, updated_at) \
         VALUES ('tenant', $1, $2, $3, now()) \
         ON CONFLICT (scope, scope_id, feature_key) \
         DO UPDATE SET enabled = $3, updated_at = now()",
    )
    .bind(tenant_id.to_string())
    .bind(key)
    .bind(enabled)
    .execute(pool)
    .await?;
    Ok(previous)
}
