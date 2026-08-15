//! auth handler：登录、当前用户、管理员创建成员、成员列表

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::PgPool;

use crate::models::user::{LoginRequest, User};
use crate::services::{audit, auth};

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
    let user = match auth::extract_user(&headers) {
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
    let user = match auth::extract_user(&headers) {
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
    let user = match auth::extract_user(&headers) {
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
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at \
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
