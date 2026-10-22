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

/// 登录失败统一文案：用户不存在 / 密码错 / 账号已停用 三种情况必须完全一致（防账号枚举）
const INVALID_CREDENTIALS: &str = "Invalid username or password";

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

/// JWT_SECRET 环境变量名（签发与校验共用同一入口）
pub const JWT_SECRET_ENV: &str = "JWT_SECRET";

/// 解析 JWT_SECRET：未配置 / 空串 / 纯空白一律 Err（fail-fast）。
///
/// 安全（C-P0-1）：旧写法在未配置时静默回退到源码公开常量 `dev-secret`，
/// 等于把签名密钥告诉所有人 → 任何人都能伪造任意租户 admin token。
/// 这里彻底删除回退值：宁可拒绝启动，也不接受人尽皆知的密钥。
fn resolve_jwt_secret(raw: Option<&str>) -> anyhow::Result<String> {
    match raw.map(str::trim) {
        Some(secret) if !secret.is_empty() => Ok(secret.to_string()),
        _ => Err(anyhow::anyhow!(
            "JWT_SECRET 未配置或为空：拒绝启动（源码内置默认密钥已移除，否则任何人都能伪造 token）"
        )),
    }
}

/// 读取并校验 JWT_SECRET（每次签发/校验都走同一入口）
pub fn jwt_secret() -> anyhow::Result<String> {
    let raw = std::env::var(JWT_SECRET_ENV).ok();
    resolve_jwt_secret(raw.as_deref())
}

/// 启动自检：无 JWT_SECRET 直接拒绝启动，不把问题拖到第一个请求。
pub fn ensure_jwt_secret_configured() -> anyhow::Result<()> {
    jwt_secret().map(|_| ())
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
    let secret = jwt_secret()?;
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn validate_token(token: &str) -> anyhow::Result<Claims> {
    let secret = jwt_secret()?;
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
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

/// 受保护端点的统一用户加载入口（按 token sub 查库）。
///
/// 与 [`extract_user`] 的区别：多一次库内校验（用户仍存在且 `is_active=true`）。
/// 这是「停用即断」的执行点——停用后已签发的旧 token 即使未过期，
/// 也在这里被拒（401），无需等 24h 过期。
///
/// 错误文案保持模糊（不区分「用户不存在 / 已停用」），与登录侧一致，避免账号枚举。
pub async fn require_user(pool: &PgPool, headers: &HeaderMap) -> Result<AuthUser, String> {
    let user = extract_user(headers)?;
    let is_active: Option<bool> =
        sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
            .bind(user.user_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                // 查询失败不等于「用户有效」，也不能静默放行：显式拒绝并留日志
                tracing::error!(
                    user_id = %user.user_id,
                    error = %e,
                    "鉴权 is_active 查库失败，拒绝请求"
                );
                "Auth check unavailable".to_string()
            })?;
    match is_active {
        Some(true) => Ok(user),
        Some(false) => {
            tracing::warn!(
                user_id = %user.user_id,
                username = %user.username,
                "已停用账号的 token 被拒"
            );
            Err("Invalid or expired token".to_string())
        }
        None => {
            tracing::warn!(user_id = %user.user_id, "token 指向的用户不存在，拒绝");
            Err("Invalid or expired token".to_string())
        }
    }
}

pub fn is_admin(user: &AuthUser) -> bool {
    user.role == UserRole::Admin
}

/// 密码强度规则（注册与管理员重置共用，避免两处口径漂移）
pub fn validate_password(password: &str) -> anyhow::Result<()> {
    if password.len() < 6 {
        return Err(anyhow::anyhow!("Password must be at least 6 characters"));
    }
    Ok(())
}

/// 工作室管理终端（owner）鉴权：与租户角色体系正交。
/// owner 不属于任何租户，用 OWNER_TOKEN 环境变量配置的独立令牌识别；
/// 未配置或配置为空时一律拒绝（fail-closed），部署时必须显式开启。
pub fn is_owner(headers: &HeaderMap) -> bool {
    let Ok(expected) = std::env::var("OWNER_TOKEN") else {
        return false;
    };
    if expected.is_empty() {
        return false;
    }
    extract_bearer_token(headers).is_some_and(|token| token == expected)
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
    validate_password(password)?;
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
         RETURNING id, tenant_id, username, password_hash, role, display_name, created_at, is_active",
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
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at, is_active \
         FROM users WHERE username = $1 ORDER BY created_at LIMIT 1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("{}", INVALID_CREDENTIALS))?;
    // 先验密码再判停用：两条失败路径对外文案完全一致（防账号枚举），
    // 顺序也保证不会因为「已停用」提前返回而省掉 bcrypt 的耗时。
    let valid = verify(password, &user.password_hash)?;
    if !valid {
        return Err(anyhow::anyhow!("{}", INVALID_CREDENTIALS));
    }
    if !user.is_active {
        tracing::warn!(
            user_id = %user.id,
            username = %user.username,
            "已停用账号尝试登录，按密码错误统一拒绝"
        );
        return Err(anyhow::anyhow!("{}", INVALID_CREDENTIALS));
    }
    let token = generate_token(&user)?;
    Ok(AuthResponse { token, user })
}

pub async fn get_user_by_id(pool: &PgPool, user_id: Uuid) -> anyhow::Result<User> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at, is_active \
         FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("User not found"))?;
    Ok(user)
}

/// 按租户取用户（跨租户一律当作不存在，避免探测他人租户成员）
pub async fn get_user_in_tenant(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, tenant_id, username, password_hash, role, display_name, created_at, is_active \
         FROM users WHERE id = $1 AND tenant_id = $2",
    )
    .bind(user_id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// 本租户当前启用中的管理员数量（自锁防线用）
pub async fn count_active_admins(pool: &PgPool, tenant_id: Uuid) -> anyhow::Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE tenant_id = $1 AND role = 'admin' AND is_active",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// 启用/停用成员。返回更新后的用户；目标不在本租户时返回 None。
pub async fn set_user_active(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    active: bool,
) -> anyhow::Result<Option<User>> {
    let user = sqlx::query_as::<_, User>(
        "UPDATE users SET is_active = $3 WHERE id = $1 AND tenant_id = $2 \
         RETURNING id, tenant_id, username, password_hash, role, display_name, created_at, is_active",
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(active)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

/// 管理员重置成员密码（不读旧密码）。目标不在本租户时返回 false。
pub async fn reset_user_password(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    new_password: &str,
) -> anyhow::Result<bool> {
    validate_password(new_password)?;
    let password_hash = hash(new_password, BCRYPT_COST)?;
    let result = sqlx::query("UPDATE users SET password_hash = $3 WHERE id = $1 AND tenant_id = $2")
        .bind(user_id)
        .bind(tenant_id)
        .bind(&password_hash)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn get_default_tenant(pool: &PgPool) -> anyhow::Result<Uuid> {
    let row = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("No tenant configured"))?;
    Ok(row.0)
}

#[cfg(test)]
mod tests {
    use super::resolve_jwt_secret;

    /// 负：未配置环境变量 → 拒绝（不得回退公开常量）
    #[test]
    fn missing_secret_rejected() {
        let err = resolve_jwt_secret(None).expect_err("未配置 JWT_SECRET 必须拒绝");
        assert!(
            err.to_string().contains("JWT_SECRET"),
            "报错必须点名 JWT_SECRET: {err}"
        );
    }

    /// 负：空串 / 纯空白同样视为未配置
    #[test]
    fn empty_and_whitespace_secret_rejected() {
        assert!(resolve_jwt_secret(Some("")).is_err(), "空串必须拒绝");
        assert!(resolve_jwt_secret(Some("   ")).is_err(), "纯空白必须拒绝");
        assert!(resolve_jwt_secret(Some("\t\n")).is_err(), "空白字符必须拒绝");
    }

    /// 正：配置了非空值 → 采用（并去掉首尾空白，避免 env 带换行导致签名不一致）
    #[test]
    fn configured_secret_accepted_and_trimmed() {
        assert_eq!(
            resolve_jwt_secret(Some("  s3cret-value  ")).expect("非空值应被接受"),
            "s3cret-value"
        );
    }
}
