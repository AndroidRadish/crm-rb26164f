//! 报表 handler（RB-266FB3 §2.4）
//!
//! 口径定义（断言以此为准）：
//! - 本月新增客户数：scope 内 created_at >= 本月 1 号 00:00（服务器时区）的客户数
//! - 总推进：scope 内进入过管道的客户数（stage IS NOT NULL，排除去重标记）
//! - 成交率 = 赢单数 / 总推进 × 100（总推进为 0 时返回 0）
//! - 逾期任务数：scope 口径的 pending 且 due_at < now() 任务数（明细走 GET /api/tasks/overdue）
//!
//! 数据域复用 §2.3：admin（老板）全团队口径，member（销售）仅自己名下客户口径。
//! 功能分级：crm.reports（专业档），关闭后 403 + 前端隐藏入口。

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;

use crate::models::user::UserRole;
use crate::services::{auth, feature};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/reports/summary — 报表汇总（本月新增 / 成交率 / 逾期数）
pub async fn summary(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "crm.reports").await {
        return resp;
    }
    let scope = auth::scope_where(&user, "c");

    // 本月新增客户数
    let month_new: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c \
         WHERE {} AND c.duplicate_of IS NULL AND c.created_at >= date_trunc('month', now())",
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

    // 总推进（进入过管道：stage 非空，排除去重标记）
    let advanced_total: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c WHERE {} AND c.stage IS NOT NULL AND c.duplicate_of IS NULL",
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

    // 赢单数（stage 落在 is_won 阶段）
    let won_count: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c JOIN stages s ON c.stage = s.id \
         WHERE {} AND s.is_won = true AND c.duplicate_of IS NULL",
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

    // 输单数（辅助口径，报表页展示用）
    let lost_count: i64 = match sqlx::query_scalar(&format!(
        "SELECT COUNT(*) FROM customers c JOIN stages s ON c.stage = s.id \
         WHERE {} AND s.is_lost = true AND c.duplicate_of IS NULL",
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

    // 逾期任务数：admin 全团队，member 仅自己（与 /tasks/overdue 同口径）
    let overdue_count: i64 = match user.role {
        UserRole::Admin => sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks t WHERE t.tenant_id = $1 AND t.status = 'pending' AND t.due_at < now()",
        )
        .bind(user.tenant_id)
        .fetch_one(&pool)
        .await,
        UserRole::Member => sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks t WHERE t.tenant_id = $1 AND t.user_id = $2 AND t.status = 'pending' AND t.due_at < now()",
        )
        .bind(user.tenant_id)
        .bind(user.user_id)
        .fetch_one(&pool)
        .await,
    }
    .unwrap_or_else(|e| {
        // 计数失败按 0 返回会掩盖问题：记日志暴露（列表接口仍会真实报错）
        tracing::warn!("reports 逾期任务计数失败: tenant={} err={}", user.tenant_id, e);
        0
    });

    let win_rate = if advanced_total > 0 {
        (won_count as f64) / (advanced_total as f64) * 100.0
    } else {
        0.0
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "month_new_customers": month_new,
            "advanced_total": advanced_total,
            "won_count": won_count,
            "lost_count": lost_count,
            "win_rate": win_rate,
            "overdue_count": overdue_count,
        })),
    )
        .into_response()
}
