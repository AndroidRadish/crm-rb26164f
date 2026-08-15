//! Seed 数据（开发/演示用）
//!
//! 运行：`SEED_CONFIRM=YES cargo run -- seed`（DATABASE_URL 指向目标库）
//! 会重建租户 + admin + 2 个成员 + 默认销售阶段 + 示例客户

use anyhow::Context;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn run_seed(database_url: &str) -> anyhow::Result<()> {
    if std::env::var("SEED_CONFIRM").as_deref() != Ok("YES") {
        anyhow::bail!("seed 需要显式确认。确认目标库后设置 SEED_CONFIRM=YES 再运行");
    }
    let admin_pw = std::env::var("SEED_ADMIN_PASSWORD")
        .unwrap_or_else(|_| Uuid::new_v4().simple().to_string()[..16].to_owned());
    let member_pw = std::env::var("SEED_MEMBER_PASSWORD")
        .unwrap_or_else(|_| Uuid::new_v4().simple().to_string()[..16].to_owned());

    tracing::info!("Connecting to database...");
    let pool = crate::db::init_db(database_url).await?;

    // 清库（按依赖顺序删除）
    sqlx::query("DELETE FROM pipeline_entries")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tasks").execute(&pool).await.ok();
    sqlx::query("DELETE FROM activities")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM import_batches")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM audit_logs")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM customers")
        .execute(&pool)
        .await
        .ok();
    sqlx::query("DELETE FROM stages").execute(&pool).await.ok();
    sqlx::query("DELETE FROM users").execute(&pool).await.ok();
    sqlx::query("DELETE FROM tenants").execute(&pool).await.ok();
    tracing::info!("Database cleared. Seeding...");

    // 租户
    let tenant = sqlx::query_as::<_, (Uuid,)>(
        "INSERT INTO tenants (name, code) VALUES ('罗老板团队', 'rb26164f') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .context("insert tenant")?;
    let tenant_id = tenant.0;

    // 用户
    let admin = create_user(&pool, tenant_id, "admin", &admin_pw, "admin", "罗老板").await?;
    let xiaoli = create_user(&pool, tenant_id, "xiaoli", &member_pw, "member", "小李").await?;
    let alex = create_user(&pool, tenant_id, "alex", &member_pw, "member", "阿力").await?;

    // 默认销售阶段
    let stage_lead = create_stage(&pool, tenant_id, "线索", 0, false, false).await?;
    let stage_intent = create_stage(&pool, tenant_id, "意向", 1, false, false).await?;
    let stage_quote = create_stage(&pool, tenant_id, "报价", 2, false, false).await?;
    let _stage_won = create_stage(&pool, tenant_id, "赢单", 3, true, false).await?;
    let _stage_lost = create_stage(&pool, tenant_id, "输单", 4, false, true).await?;

    // 示例客户
    create_customer(
        &pool,
        tenant_id,
        "示例科技有限公司",
        "王经理",
        "13800138001",
        Some(xiaoli),
        Some(stage_lead),
    )
    .await?;
    create_customer(
        &pool,
        tenant_id,
        "示例贸易有限公司",
        "李总",
        "13800138002",
        Some(alex),
        Some(stage_intent),
    )
    .await?;
    create_customer(
        &pool,
        tenant_id,
        "示例制造厂",
        "张工",
        "13800138003",
        Some(xiaoli),
        Some(stage_quote),
    )
    .await?;
    create_customer(
        &pool,
        tenant_id,
        "公海示例客户",
        "赵先生",
        "13800138004",
        None,
        None,
    )
    .await?;

    // 示例跟进记录
    let _ = sqlx::query(
        "INSERT INTO activities (tenant_id, customer_id, user_id, type, content) \
         SELECT $1, c.id, $2, 'call', '初次电话沟通，客户对产品感兴趣' \
         FROM customers c WHERE c.company = '示例科技有限公司'",
    )
    .bind(tenant_id)
    .bind(xiaoli)
    .execute(&pool)
    .await;

    // 示例待办
    let _ = sqlx::query(
        "INSERT INTO tasks (tenant_id, customer_id, user_id, title, due_at) \
         SELECT $1, c.id, $2, '跟进报价反馈', now() + interval '2 days' \
         FROM customers c WHERE c.company = '示例制造厂'",
    )
    .bind(tenant_id)
    .bind(xiaoli)
    .execute(&pool)
    .await;

    tracing::info!("Seed complete.");
    println!("================================================");
    println!("CRM RB-26164F 种子数据已写入");
    println!("租户: 罗老板团队 (rb26164f)");
    println!("管理员: admin / {}", admin_pw);
    println!("成员: xiaoli / {}", member_pw);
    println!("成员: alex / {}", member_pw);
    println!("================================================");

    let _ = admin;
    Ok(())
}

async fn create_user(
    pool: &PgPool,
    tenant_id: Uuid,
    username: &str,
    password: &str,
    role: &str,
    display_name: &str,
) -> anyhow::Result<Uuid> {
    let hash = bcrypt::hash(password, 8)?;
    let row = sqlx::query_as::<_, (Uuid,)>(
        "INSERT INTO users (tenant_id, username, password_hash, role, display_name) \
         VALUES ($1, $2, $3, $4::user_role, $5) RETURNING id",
    )
    .bind(tenant_id)
    .bind(username)
    .bind(&hash)
    .bind(role)
    .bind(display_name)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

async fn create_stage(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    order_index: i32,
    is_won: bool,
    is_lost: bool,
) -> anyhow::Result<Uuid> {
    let row = sqlx::query_as::<_, (Uuid,)>(
        "INSERT INTO stages (tenant_id, name, order_index, is_won, is_lost) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(order_index)
    .bind(is_won)
    .bind(is_lost)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

async fn create_customer(
    pool: &PgPool,
    tenant_id: Uuid,
    company: &str,
    contact_name: &str,
    phone: &str,
    owner_id: Option<Uuid>,
    stage: Option<Uuid>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO customers (tenant_id, company, contact_name, phone, owner_id, stage, source) \
         VALUES ($1, $2, $3, $4, $5, $6, 'manual')",
    )
    .bind(tenant_id)
    .bind(company)
    .bind(contact_name)
    .bind(phone)
    .bind(owner_id)
    .bind(stage)
    .execute(pool)
    .await?;
    Ok(())
}
