//! auth handler：登录、当前用户、管理员创建成员、成员列表、成员生命周期（停用/启用/重置密码）

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::user::{LoginRequest, ResetPasswordRequest, User, UserRole};
use crate::services::{audit, auth};

fn unauthorized(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

fn forbidden(error: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({ "error": error })),
    )
        .into_response()
}

fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "User not found" })),
    )
        .into_response()
}

fn server_error(error: impl std::fmt::Display) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": error.to_string() })),
    )
        .into_response()
}

/// 成员管理端点统一前置：先按 token sub 查库验证用户仍启用（停用即断），再做 admin 门禁。
/// 非 admin 一律 403；鉴权失败一律 401 —— 正负两条路径都有明确响应，不静默放行。
async fn require_admin(pool: &PgPool, headers: &HeaderMap) -> Result<auth::AuthUser, Response> {
    let caller = match auth::require_user(pool, headers).await {
        Ok(u) => u,
        Err(e) => {
            tracing::warn!(error = %e, "成员管理端点鉴权失败");
            return Err(unauthorized(e));
        }
    };
    if !auth::is_admin(&caller) {
        tracing::warn!(
            user_id = %caller.user_id,
            "非管理员调用成员管理端点，拒绝"
        );
        return Err(forbidden("Only admin can manage members"));
    }
    Ok(caller)
}

/// 按 id 读取本租户目标成员：跨租户/不存在统一 404（不泄露他人租户成员是否存在）。
async fn load_target(
    pool: &PgPool,
    tenant_id: Uuid,
    target_id: Uuid,
) -> Result<User, Response> {
    match auth::get_user_in_tenant(pool, tenant_id, target_id).await {
        Ok(Some(u)) => Ok(u),
        Ok(None) => {
            tracing::warn!(tenant_id = %tenant_id, target_id = %target_id, "成员管理目标不存在");
            Err(not_found())
        }
        Err(e) => {
            tracing::error!(tenant_id = %tenant_id, target_id = %target_id, error = %e, "读取目标成员失败");
            Err(server_error(e))
        }
    }
}

/// POST /api/auth/login — 登录
pub async fn login(State(pool): State<PgPool>, Json(req): Json<LoginRequest>) -> impl IntoResponse {
    match auth::login_user(&pool, &req.username, &req.password).await {
        Ok(resp) => (StatusCode::OK, Json(resp)).into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/auth/me — 当前用户信息
pub async fn me(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
            )
                .into_response();
        }
    };
    match auth::get_user_by_id(&pool, user.user_id).await {
        Ok(u) => (StatusCode::OK, Json(u)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /api/users — 管理员创建成员
pub async fn create_user(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(req): Json<crate::models::user::CreateUserRequest>,
) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(e) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": e })),
            )
                .into_response();
        }
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can create accounts" })),
        )
            .into_response();
    }

    match auth::register_user(
        &pool,
        &req.username,
        &req.password,
        req.role,
        user.tenant_id,
    )
    .await
    {
        Ok(resp) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "user.create",
                "user",
                &resp.user.id.to_string(),
                &format!("username={} role={:?}", resp.user.username, resp.user.role),
            )
            .await;
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/users — 管理员列出成员
pub async fn list_users(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::require_user(&pool, &headers).await {
        Ok(u) => u,
        Err(_) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
            )
                .into_response();
        }
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can list users" })),
        )
            .into_response();
    }

    match sqlx::query_as::<_, User>(
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at, is_active \
         FROM users WHERE tenant_id = $1 ORDER BY created_at",
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

/// POST /api/users/:id/deactivate — 管理员停用成员（离职/收回账号）
///
/// 停用后该成员：① 登录按「密码错误」统一拒绝；② 已签发 token 在下一次请求即 401
/// （鉴权入口 require_user 每次查 is_active）。
pub async fn deactivate_user(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(target_id): Path<Uuid>,
) -> impl IntoResponse {
    let caller = match require_admin(&pool, &headers).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let target = match load_target(&pool, caller.tenant_id, target_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    // 自锁防线 1（先于「不能停用自己」判定）：最后一名启用中的管理员不可停用。
    // 若顺序反过来，「唯一 admin 停自己」只会报「不能停用自己」，最后管理员防线永远走不到。
    if target.role == UserRole::Admin && target.is_active {
        let active_admins = match auth::count_active_admins(&pool, caller.tenant_id).await {
            Ok(n) => n,
            Err(e) => {
                tracing::error!(tenant_id = %caller.tenant_id, error = %e, "统计启用中管理员失败");
                return server_error(e);
            }
        };
        if active_admins <= 1 {
            tracing::warn!(
                tenant_id = %caller.tenant_id,
                target_id = %target.id,
                "拒绝停用本租户最后一个启用中的管理员（防自锁）"
            );
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "不能停用本租户最后一个管理员" })),
            )
                .into_response();
        }
    }

    // 自锁防线 2：不能停用自己
    if target.id == caller.user_id {
        tracing::warn!(user_id = %caller.user_id, "拒绝停用自己（防自锁）");
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "不能停用自己" })),
        )
            .into_response();
    }

    match auth::set_user_active(&pool, caller.tenant_id, target.id, false).await {
        Ok(Some(user)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "user.deactivate",
                "user",
                &user.id.to_string(),
                &format!("username={} role={:?}", user.username, user.role),
            )
            .await;
            (StatusCode::OK, Json(user)).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!(target_id = %target.id, error = %e, "停用成员失败");
            server_error(e)
        }
    }
}

/// POST /api/users/:id/activate — 管理员重新启用成员（恢复登录与旧 token 可用性）
pub async fn activate_user(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(target_id): Path<Uuid>,
) -> impl IntoResponse {
    let caller = match require_admin(&pool, &headers).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let target = match load_target(&pool, caller.tenant_id, target_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    match auth::set_user_active(&pool, caller.tenant_id, target.id, true).await {
        Ok(Some(user)) => {
            audit::log_from_headers(
                &pool,
                &headers,
                "user.activate",
                "user",
                &user.id.to_string(),
                &format!("username={} role={:?}", user.username, user.role),
            )
            .await;
            (StatusCode::OK, Json(user)).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!(target_id = %target.id, error = %e, "启用成员失败");
            server_error(e)
        }
    }
}

/// POST /api/users/:id/reset-password — 管理员直接设新密码（不读旧密码）
///
/// 请求体手动解析而不是用 `Json` 提取器：`Json` 会在进入 handler 前失败，
/// 导致非管理员带畸形请求体时先收到 400 而不是 403。这里先过 admin 门禁再解析，
/// 保证「非 admin 一律 403」这条权限语义不被请求体形状影响。
pub async fn reset_password(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(target_id): Path<Uuid>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let caller = match require_admin(&pool, &headers).await {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let req: ResetPasswordRequest = match serde_json::from_slice(&body) {
        Ok(req) => req,
        Err(e) => {
            tracing::warn!(error = %e, "重置密码请求体解析失败");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "需要 JSON 请求体：{\"password\": \"新密码\"}" })),
            )
                .into_response();
        }
    };
    if let Err(e) = auth::validate_password(&req.password) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    let target = match load_target(&pool, caller.tenant_id, target_id).await {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    match auth::reset_user_password(&pool, caller.tenant_id, target.id, &req.password).await {
        Ok(true) => {
            // 审计只记操作与目标，绝不写明文密码
            audit::log_from_headers(
                &pool,
                &headers,
                "user.reset_password",
                "user",
                &target.id.to_string(),
                &format!("username={}", target.username),
            )
            .await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "ok": true, "user_id": target.id })),
            )
                .into_response()
        }
        Ok(false) => not_found(),
        Err(e) => {
            tracing::error!(target_id = %target.id, error = %e, "重置成员密码失败");
            server_error(e)
        }
    }
}
