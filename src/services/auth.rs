//! 权限模型：CRM 版
//!
//! 角色：
//! - admin   → tenant 全局数据可见；管理成员；自定义销售阶段；查看团队业绩看板；数据导出
//! - member  → 仅看自己名下客户；录入/编辑自己的客户；跟进记录；认领公海客户
//!
//! scope_where(user, table_prefix) 生成 SQL WHERE 片段，所有查询都追加。
//! admin 看 tenant 全部，member 自动追加 `AND <prefix>owner_id = 自己`。
//! 越权靠"每一条 SQL 都带 scope_where"来保证，集成测试守住。

use axum::http::HeaderMap;
use bcrypt::{hash, verify};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::user::{AuthResponse, User, UserRole};

/// bcrypt cost：debug 下用 8 加速测试，release 下用 12
const BCRYPT_COST: u32 = 8;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub tenant_id: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
    pub role: UserRole,
    pub tenant_id: Uuid,
}

fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret".to_string())
}

pub fn generate_token(user: &User) -> anyhow::Result<String> {
    let now = Utc::now();
    let exp = now + Duration::hours(24);
    let claims = Claims {
        sub: user.id.to_string(),
        username: user.username.clone(),
        role: serde_json::to_value(&user.role)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "member".to_string()),
        tenant_id: user.tenant_id.to_string(),
        iat: now.timestamp(),
        exp: exp.timestamp(),
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret().as_bytes()),
    )?;
    Ok(token)
}

pub fn validate_token(token: &str) -> anyhow::Result<Claims> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret().as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

pub fn extract_user(headers: &HeaderMap) -> Result<AuthUser, String> {
    let token = extract_bearer_token(headers).ok_or("Missing token")?;
    let claims = validate_token(&token).map_err(|e| format!("Invalid token: {e}"))?;
    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| "Invalid user ID".to_string())?;
    let role = serde_json::from_str::<UserRole>(&format!("\"{}\"", claims.role))
        .unwrap_or(UserRole::Member);
    let tenant_id =
        Uuid::parse_str(&claims.tenant_id).map_err(|_| "Invalid tenant ID".to_string())?;
    Ok(AuthUser {
        user_id,
        username: claims.username,
        role,
        tenant_id,
    })
}

pub fn is_admin(user: &AuthUser) -> bool {
    user.role == UserRole::Admin
}

/// 生成数据范围过滤 SQL。
/// admin → `tenant_id = '...'`
/// member → `tenant_id = '...' AND <prefix>owner_id = '...'`
pub fn scope_where(user: &AuthUser, table_prefix: &str) -> String {
    let prefix = if table_prefix.is_empty() {
        String::new()
    } else {
        format!("{}.", table_prefix)
    };
    match user.role {
        UserRole::Admin => format!("{}tenant_id = '{}'", prefix, user.tenant_id),
        UserRole::Member => format!(
            "{}tenant_id = '{}' AND {}owner_id = '{}'",
            prefix, user.tenant_id, prefix, user.user_id
        ),
    }
}

/// 注册用户（管理员创建成员）
pub async fn register_user(
    pool: &PgPool,
    username: &str,
    password: &str,
    role: Option<UserRole>,
    tenant_id: Uuid,
) -> anyhow::Result<AuthResponse> {
    if password.len() < 6 {
        return Err(anyhow::anyhow!("Password must be at least 6 characters"));
    }
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE username = $1 AND tenant_id = $2",
    )
    .bind(username)
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    if existing > 0 {
        return Err(anyhow::anyhow!("Username already exists"));
    }

    let password_hash = hash(password, BCRYPT_COST)?;
    let user_role = role.unwrap_or(UserRole::Member);
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (tenant_id, username, password_hash, role) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, tenant_id, username, password_hash, role, display_name, created_at",
    )
    .bind(tenant_id)
    .bind(username)
    .bind(&password_hash)
    .bind(&user_role)
    .fetch_one(pool)
    .await?;

    let token = generate_token(&user)?;
    Ok(AuthResponse { token, user })
}

pub async fn login_user(
    pool: &PgPool,
    username: &str,
    password: &str,
) -> anyhow::Result<AuthResponse> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at \
         FROM users WHERE username = $1 ORDER BY created_at LIMIT 1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Invalid username or password"))?;
    let valid = verify(password, &user.password_hash)?;
    if !valid {
        return Err(anyhow::anyhow!("Invalid username or password"));
    }
    let token = generate_token(&user)?;
    Ok(AuthResponse { token, user })
}

pub async fn get_user_by_id(pool: &PgPool, user_id: Uuid) -> anyhow::Result<User> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at \
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("User not found"))?;
    Ok(user)
}

pub async fn get_default_tenant(pool: &PgPool) -> anyhow::Result<Uuid> {
    let row = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No tenant configured"))?;
    Ok(row.0)
}
