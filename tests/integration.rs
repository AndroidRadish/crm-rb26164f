//! CRM RB-26164F 集成测试
//!
//! 运行：`cargo test --test integration -- --test-threads=1`
//! 前置：本地 PostgreSQL，已建 `crm_rb26164f_test` 库
//! 环境变量：DATABASE_URL（默认 postgres://postgres:postgres@localhost:5432/crm_rb26164f_test）

use reqwest::Client;
use serde_json::Value;

/// 清库（按依赖顺序删除）
async fn clean_db(pool: &sqlx::PgPool) {
    sqlx::query("DELETE FROM pipeline_entries")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM tasks").execute(pool).await.ok();
    sqlx::query("DELETE FROM activities")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM import_batches")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM audit_logs")
        .execute(pool)
        .await
        .ok();
    sqlx::query("DELETE FROM customers")
        .execute(pool)
        .await
        .ok();
    // settings 有 tenant_id FK：漏删会挡住 DELETE FROM tenants（静默 .ok()），
    // 残留租户导致下次跑测试时固定 code（legacy/tier-default）重复键失败
    sqlx::query("DELETE FROM settings").execute(pool).await.ok();
    sqlx::query("DELETE FROM stages").execute(pool).await.ok();
    sqlx::query("DELETE FROM users").execute(pool).await.ok();
    sqlx::query("DELETE FROM entitlements").execute(pool).await.ok();
    sqlx::query("DELETE FROM tenants").execute(pool).await.ok();
}

/// 确保有默认租户
async fn ensure_tenant(pool: &sqlx::PgPool) -> uuid::Uuid {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tenants")
        .fetch_one(pool)
        .await
        .unwrap_or(0);
    if exists == 0 {
        sqlx::query("INSERT INTO tenants (name, code) VALUES ('测试租户', 'test')")
            .execute(pool)
            .await
            .ok();
    }
    sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
        .fetch_one(pool)
        .await
        .expect("tenant")
}

/// 启动测试服务器
async fn start_server() -> String {
    // owner 管理端令牌（测试环境固定值；生产由部署环境变量注入）
    std::env::set_var("OWNER_TOKEN", "test-owner-token");
    // JWT_SECRET（测试环境固定值；生产由部署 env 注入）。
    // 代码已删除源码回退常量，未配置会拒绝启动 —— 测试进程必须显式提供。
    std::env::set_var("JWT_SECRET", "test-jwt-secret-not-the-public-default");

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });

    let pool = crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("Failed to initialize database");

    clean_db(&pool).await;
    let tenant_id = ensure_tenant(&pool).await;

    // 创建默认阶段
    sqlx::query(
        "INSERT INTO stages (tenant_id, name, order_index, is_won, is_lost) \
         VALUES ($1, '线索', 0, false, false), ($1, '赢单', 1, true, false), ($1, '输单', 2, false, true)",
    )
    .bind(tenant_id)
    .execute(&pool)
    .await
    .ok();

    let app = axum::Router::new()
        .nest("/api", crm_rb26164f::router())
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let addr = listener.local_addr().expect("Failed to get local address");
    let base_url = format!("http://{}", addr);

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Server failed");
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    base_url
}

/// 直接插入用户并返回 token
async fn setup_user(
    client: &Client,
    base: &str,
    username: &str,
    password: &str,
    role: &str,
) -> String {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });
    let pool = crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("pool init");
    let tenant_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("tenant");
    let hash = bcrypt::hash(password, 8).expect("hash");
    sqlx::query(
        "INSERT INTO users (tenant_id, username, password_hash, role) VALUES ($1,$2,$3,$4::user_role) \
         ON CONFLICT (tenant_id, username) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(username)
    .bind(&hash)
    .bind(role)
    .execute(&pool)
    .await
    .expect("insert user");

    let resp = client
        .post(&format!("{}/api/auth/login", base))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .expect("login");
    assert!(resp.status().is_success(), "login failed for {}", username);
    let body: Value = resp.json().await.expect("json");
    body["token"].as_str().expect("token").to_string()
}

/// 创建客户
async fn create_customer(client: &Client, base: &str, token: &str, company: &str) -> Value {
    let resp = client
        .post(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "company": company,
            "contact_name": "测试联系人",
            "phone": "13800138000",
        }))
        .send()
        .await
        .expect("create customer");
    assert!(
        resp.status().is_success(),
        "create customer failed: {}",
        resp.status()
    );
    resp.json().await.expect("json")
}

#[tokio::test]
async fn test_health() {
    let base = start_server().await;
    let client = Client::new();
    let resp = client
        .get(&format!("{}/api/health", base))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn test_auth_login_and_me() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin1", "admin123", "admin").await;

    // /auth/me
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["username"], "admin1");
    assert_eq!(body["role"], "admin");
}

#[tokio::test]
async fn test_unauthorized_access() {
    let base = start_server().await;
    let client = Client::new();

    // 无 token 访问业务端点
    let resp = client
        .get(&format!("{}/api/customers", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    let resp = client
        .get(&format!("{}/api/stages", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_customer_crud() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin2", "admin123", "admin").await;

    // 创建客户
    let customer = create_customer(&client, &base, &token, "测试公司A").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();
    assert_eq!(customer["company"], "测试公司A");

    // 获取客户
    let resp = client
        .get(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["company"], "测试公司A");

    // 更新客户
    let resp = client
        .put(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "notes": "已更新备注" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["notes"], "已更新备注");

    // 删除客户
    let resp = client
        .delete(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn test_permission_isolation() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin3", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member1", "member123", "member").await;

    // 管理员创建客户
    let customer = create_customer(&client, &base, &admin_token, "隔离测试公司").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();

    // 成员访问他人客户应返回 404
    let resp = client
        .get(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // 成员列表不应看到该客户
    let resp = client
        .get(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let arr = body.as_array().unwrap();
    assert!(arr.is_empty(), "member should not see admin's customers");
}

#[tokio::test]
async fn test_public_pool_claim_release() {
    let base = start_server().await;
    let client = Client::new();
    let _admin_token = setup_user(&client, &base, "admin4", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member2", "member123", "member").await;

    // 直接插入公海客户（owner_id = NULL）
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });
    let pool = crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("pool init");
    let tenant_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("tenant");
    let customer_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO customers (tenant_id, company, contact_name, owner_id, source) \
         VALUES ($1, '公海测试公司', '公海联系人', NULL, 'manual') RETURNING id",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .expect("insert pool customer");

    // 成员认领公海客户
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "claim failed: {}",
        resp.status()
    );
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["source"], "claim");

    // 成员释放客户回公海
    let resp = client
        .post(&format!("{}/api/customers/{}/release", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "release failed: {}",
        resp.status()
    );
    let body: Value = resp.json().await.unwrap();
    assert!(body["owner_id"].is_null());
}

#[tokio::test]
async fn test_duplicate_detection() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin5", "admin123", "admin").await;

    // 创建第一个客户
    create_customer(&client, &base, &token, "重复检测公司").await;

    // 创建相同公司名的客户应返回 409
    let resp = client
        .post(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "company": "重复检测公司",
            "contact_name": "另一个联系人",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);

    // 检查重复检测端点
    let resp = client
        .post(&format!("{}/api/customers/check-duplicate", base))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "company": "重复检测公司" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["is_duplicate"], true);
}

#[tokio::test]
async fn test_stages_crud() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin6", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member3", "member123", "member").await;

    // 管理员创建阶段
    let resp = client
        .post(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "name": "新阶段", "order_index": 5 }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let stage: Value = resp.json().await.unwrap();
    let stage_id = stage["id"].as_str().unwrap().to_string();

    // 成员不能创建阶段
    let resp = client
        .post(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .json(&serde_json::json!({ "name": "非法阶段" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);

    // 列出阶段
    let resp = client
        .get(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert!(body.as_array().unwrap().len() >= 3);

    // 更新阶段
    let resp = client
        .put(&format!("{}/api/stages/{}", base, stage_id))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "name": "更新后的阶段" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn test_activities_timeline() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin7", "admin123", "admin").await;

    let customer = create_customer(&client, &base, &token, "时间轴测试公司").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();

    // 添加跟进记录
    let resp = client
        .post(&format!(
            "{}/api/customers/{}/activities",
            base, customer_id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "type": "call",
            "content": "电话沟通，客户有意向",
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 列出跟进记录
    let resp = client
        .get(&format!(
            "{}/api/customers/{}/activities",
            base, customer_id
        ))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_tasks() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin8", "admin123", "admin").await;

    let customer = create_customer(&client, &base, &token, "待办测试公司").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();

    // 创建待办
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "title": "跟进报价",
            "due_at": "2026-08-20T10:00:00Z",
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 列出待办
    let resp = client
        .get(&format!("{}/api/tasks", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_kanban_and_funnel() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin9", "admin123", "admin").await;

    // 创建客户并设置阶段
    let customer = create_customer(&client, &base, &token, "看板测试公司").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();

    // 获取阶段列表
    let resp = client
        .get(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    let stages: Value = resp.json().await.unwrap();
    let stage_id = stages[0]["id"].as_str().unwrap().to_string();

    // 更新客户阶段
    let resp = client
        .put(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "stage": stage_id }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 看板
    let resp = client
        .get(&format!("{}/api/pipeline/kanban", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 漏斗
    let resp = client
        .get(&format!("{}/api/pipeline/funnel", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

#[tokio::test]
async fn test_kanban_member_sees_public_pool() {
    let base = start_server().await;
    let client = Client::new();
    let _admin_token = setup_user(&client, &base, "admin13", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member6", "member123", "member").await;

    // 直接插入公海客户（owner_id = NULL）并设置阶段
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });
    let pool = crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("pool init");
    let tenant_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("tenant");
    let stage_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM stages ORDER BY order_index LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("stage");
    let customer_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO customers (tenant_id, company, contact_name, owner_id, stage, source) \
         VALUES ($1, '公海看板公司', '公海联系人', NULL, $2, 'manual') RETURNING id",
    )
    .bind(tenant_id)
    .bind(stage_id)
    .fetch_one(&pool)
    .await
    .expect("insert pool customer");

    // 成员看板应能看到公海客户（owner_id IS NULL）
    let resp = client
        .get(&format!("{}/api/pipeline/kanban", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let columns = body.as_array().unwrap();
    let mut found = false;
    for col in columns {
        let customers = col["customers"].as_array().unwrap();
        for c in customers {
            if c["id"].as_str().unwrap() == customer_id.to_string().as_str() {
                assert!(c["owner_id"].is_null(), "public pool customer should have null owner_id");
                found = true;
            }
        }
    }
    assert!(found, "member kanban should include public pool customer");

    // 成员认领后，看板应显示该客户且 owner_id 非空
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let resp = client
        .get(&format!("{}/api/pipeline/kanban", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let columns = body.as_array().unwrap();
    let mut found_claimed = false;
    for col in columns {
        let customers = col["customers"].as_array().unwrap();
        for c in customers {
            if c["id"].as_str().unwrap() == customer_id.to_string().as_str() {
                assert!(!c["owner_id"].is_null(), "claimed customer should have owner_id");
                assert!(
                    c["owner_name"].is_string(),
                    "claimed customer should have owner_name"
                );
                found_claimed = true;
            }
        }
    }
    assert!(found_claimed, "member kanban should include claimed customer");
}

#[tokio::test]
async fn test_dashboard() {
    let base = start_server().await;
    let client = Client::new();
    let token = setup_user(&client, &base, "admin10", "admin123", "admin").await;

    let resp = client
        .get(&format!("{}/api/dashboard", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert!(body["total_customers"].is_number());
}

#[tokio::test]
async fn test_audit_logs() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin11", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member4", "member123", "member").await;

    // 管理员可查看审计日志
    let resp = client
        .get(&format!("{}/api/audit", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 成员不能查看审计日志
    let resp = client
        .get(&format!("{}/api/audit", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_import_and_export() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin12", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member5", "member123", "member").await;

    // 模板下载（C-P2-2：已补 auth/feature 门，必须带令牌）
    let resp = client
        .get(&format!("{}/api/import/template", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // CSV 导入
    let csv_data = "公司名,联系人,电话,邮箱,地址,备注,阶段\n导入公司A,王五,13900139001,wangwu@example.com,上海,备注A,线索\n导入公司B,赵六,13900139002,zhaoliu@example.com,北京,备注B,意向\n";
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success_rows"], 2);
    assert_eq!(body["failed_rows"], 0);

    // 批次列表
    let resp = client
        .get(&format!("{}/api/import/batches", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 管理员导出 CSV
    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 成员不能导出
    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
}

/// 缺陷1修复：multipart/form-data 上传正常文件 → 只导入文件部分，信封行不入库
#[tokio::test]
async fn test_import_multipart_valid_file() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin14", "admin123", "admin").await;

    // 手工构造 multipart/form-data body（模拟 Apifox/Postman/HTML 文件表单）
    let boundary = "----TestBoundaryXYZ123";
    let csv_content = "公司名,联系人,电话,邮箱,地址,备注,阶段\nmultipart公司甲,孙七,13800138007,sunqi@example.com,广州,多部分上传,线索\nmultipart公司乙,周八,13800138008,zhouba@example.com,深圳,多部分上传,意向\n";
    let multipart_body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"customers.csv\"\r\nContent-Type: text/csv\r\n\r\n{csv}\r\n--{b}--\r\n",
        b = boundary,
        csv = csv_content
    );

    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(multipart_body)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "multipart 合法文件应导入成功: {}",
        resp.status()
    );
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success_rows"], 2, "只应导入文件部分里的 2 行客户");
    assert_eq!(body["failed_rows"], 0);

    // 确认入库的是文件内容，而不是 boundary/Content-Disposition 信封行
    let resp = client
        .get(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    let customers: Value = resp.json().await.unwrap();
    let companies: Vec<&str> = customers
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["company"].as_str())
        .collect();
    assert!(companies.contains(&"multipart公司甲"));
    assert!(companies.contains(&"multipart公司乙"));
    assert!(
        !companies.iter().any(|c| c.contains("Content-Disposition")
            || c.contains("Boundary")
            || c.contains("text/csv")),
        "multipart 信封行不得入库: {:?}",
        companies
    );
}

/// 缺陷1修复：multipart 垃圾内容（文件部分不是合法 CSV/xlsx）→ 400 且不入库
#[tokio::test]
async fn test_import_multipart_garbage_rejected() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin15", "admin123", "admin").await;

    // 垃圾内容：整个 multipart 信封直接当 body（即修复前的复现路径）
    // 且没有带 filename 的文件 part → 应 400
    let boundary = "----GarbageBoundary456";
    let garbage_body = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nnot-a-file-field\r\n--{b}--\r\n",
        b = boundary
    );
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(garbage_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "无文件 part 的 multipart 应被拒绝");
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("文件"),
        "应返回明确错误信息: {}",
        body
    );

    // 垃圾内容：有文件 part 但内容解析不出任何数据行 → 也应 400
    let garbage_body2 = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"junk.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n\r\n--{b}--\r\n",
        b = boundary
    );
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={}", boundary),
        )
        .body(garbage_body2)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "空/垃圾文件内容应被拒绝");
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].is_string(), "应返回明确错误信息: {}", body);

    // 确认垃圾内容没有入库（customers 应为空）
    let resp = client
        .get(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    let customers: Value = resp.json().await.unwrap();
    assert!(
        customers.as_array().unwrap().is_empty(),
        "垃圾 multipart 内容不得入库: {}",
        customers
    );

    // 匿名用户无权导入
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Content-Type", "text/csv")
        .body("公司名,联系人\n匿名公司,匿名")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

/// 缺陷2相关 API 守卫：POST /customers/:id/tasks 权限矩阵
#[tokio::test]
async fn test_task_create_role_matrix() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin16", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member7", "member123", "member").await;

    // admin 建客户，并在该客户下建待办 → 可做
    let customer = create_customer(&client, &base, &admin_token, "待办矩阵公司").await;
    let customer_id = customer["id"].as_str().unwrap().to_string();
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, customer_id))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({
            "title": "回访客户",
            "due_at": "2026-08-20T10:00:00Z",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "admin 应能创建待办");
    let task: Value = resp.json().await.unwrap();
    assert_eq!(task["title"], "回访客户");

    // 匿名 → 401
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, customer_id))
        .json(&serde_json::json!({
            "title": "匿名待办",
            "due_at": "2026-08-20T10:00:00Z",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "匿名不能创建待办");

    // member 对他人名下客户建待办 → 404（scope 隔离）
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .json(&serde_json::json!({
            "title": "越权待办",
            "due_at": "2026-08-20T10:00:00Z",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "member 不能在他人客户下建待办");

    // member 在自己客户下建待办 → 可做（证明不是一刀切）
    let own_customer = create_customer(&client, &base, &member_token, "成员自己的公司").await;
    let own_customer_id = own_customer["id"].as_str().unwrap().to_string();
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, own_customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .json(&serde_json::json!({
            "title": "自己的待办",
            "due_at": "2026-08-21T09:00:00Z",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "member 应能在自己客户下建待办");

    // admin 的待办列表能看到刚建的待办
    let resp = client
        .get(&format!("{}/api/tasks", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let tasks: Value = resp.json().await.unwrap();
    assert!(
        tasks
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"] == "回访客户"),
        "待办列表应包含新建待办"
    );
}


// ── 功能分级（entitlement 层打样）──────────────────────────────
// spec: RadishCode/planning/req-crm-tiering-spec.md
// 矩阵：有/无授权 × admin/member × 能/不能；存量补种断言；owner 配置端点

const OWNER_TOKEN: &str = "test-owner-token";

/// 新建一个连接池。每个 #[tokio::test] 有自己的 runtime，池必须在当前 runtime 创建；
/// 复用先前测试留下的全局池会拿到失效连接（pool timed out 的教训）。
/// 注意：init_db 会重放迁移（幂等），调用时机影响迁移补种覆盖的租户集合。
async fn test_pool() -> sqlx::PgPool {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });
    crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("pool init")
}

/// 取当前测试租户 id
async fn current_tenant_id() -> uuid::Uuid {
    let pool = test_pool().await;
    sqlx::query_scalar("SELECT id FROM tenants ORDER BY created_at LIMIT 1")
        .fetch_one(&pool)
        .await
        .expect("tenant")
}

/// owner API：配置某租户的功能包
async fn owner_put_features(
    client: &Client,
    base: &str,
    tenant_id: uuid::Uuid,
    changes: serde_json::Value,
) -> reqwest::Response {
    client
        .put(&format!(
            "{}/api/admin/tenants/{}/entitlements",
            base, tenant_id
        ))
        .header("Authorization", format!("Bearer {}", OWNER_TOKEN))
        .json(&changes)
        .send()
        .await
        .expect("owner put entitlements")
}

/// 缺省规则：新租户（无 entitlements 记录）按 feature_keys.tier 默认档
/// 基础档（crm.*）默认开，专业/旗舰档（pool.claim 等）默认关 —— 正负成对
#[tokio::test]
async fn test_entitlements_default_tier_rule() {
    let base = start_server().await;
    let client = Client::new();

    // 直接建租户+用户（不走 init_db，避免触发迁移重放补种），验证"表无记录按默认档"
    let pool = test_pool().await;
    let tenant_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tenants (name, code) VALUES ('默认档租户', 'tier-default') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("insert tenant");
    let hash = bcrypt::hash("member123", 8).expect("hash");
    sqlx::query(
        "INSERT INTO users (tenant_id, username, password_hash, role) VALUES ($1, 't2member', $2, 'member')",
    )
    .bind(tenant_id)
    .bind(&hash)
    .execute(&pool)
    .await
    .expect("insert user");

    let resp = client
        .post(&format!("{}/api/auth/login", base))
        .json(&serde_json::json!({ "username": "t2member", "password": "member123" }))
        .send()
        .await
        .expect("login");
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap().to_string();

    // 授权清单：默认档计算后结果
    let resp = client
        .get(&format!("{}/api/entitlements", base))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let list: Value = resp.json().await.unwrap();
    let arr = list.as_array().unwrap();
    let get = |key: &str| arr.iter().find(|e| e["key"] == key).cloned().expect(key);

    // 正：基础档默认开
    assert_eq!(get("crm.customers")["enabled"], true);
    assert_eq!(get("crm.customers")["source"], "default");
    assert_eq!(get("crm.audit")["enabled"], true);
    // 负：专业/旗舰档默认关
    assert_eq!(get("pool.claim")["enabled"], false);
    assert_eq!(get("import.export")["enabled"], false);
    assert_eq!(get("notify.webhook")["enabled"], false);

    // 负：无授权调用认领端点 → 403 且带 feature key
    let resp = client
        .post(&format!(
            "{}/api/customers/{}/claim",
            base,
            uuid::Uuid::new_v4()
        ))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "pool.claim");
    assert!(
        body["error"].as_str().unwrap().contains("当前版本不含此功能"),
        "403 必须带升级提示: {}",
        body
    );
}

/// 存量兼容：迁移对现有租户补种 3 个分级功能 enabled=true（防老客户功能消失）
#[tokio::test]
async fn test_entitlement_backfill_on_migration() {
    let base = start_server().await;
    let _ = &base;

    // start_server 内已跑过一轮迁移（当时库为空）。现在插入一个"存量租户"，
    // 再显式重放迁移（迁移器每次启动全量重放），断言补种生效。
    let pool = test_pool().await;
    let tenant_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tenants (name, code) VALUES ('存量租户', 'legacy') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("insert legacy tenant");

    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test".to_string()
    });
    crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("re-run migrations");

    let rows: Vec<(String, bool)> = sqlx::query_as(
        "SELECT feature_key, enabled FROM entitlements \
         WHERE scope = 'tenant' AND scope_id = $1 ORDER BY feature_key",
    )
    .bind(tenant_id.to_string())
    .fetch_all(&pool)
    .await
    .expect("query entitlements");
    // 003 补种 3 个打样键 + 004 补种 crm.reports/crm.reminders（RB-266FB3 新登记）
    assert_eq!(rows.len(), 5, "存量租户应补种 5 个分级功能: {:?}", rows);
    for (key, enabled) in &rows {
        assert!(enabled, "补种应为 enabled=true: {}", key);
        assert!(
            [
                "crm.reminders",
                "crm.reports",
                "import.export",
                "notify.webhook",
                "pool.claim"
            ]
            .contains(&key.as_str()),
            "补种键应在登记范围内: {}",
            key
        );
    }

    // 幂等断言：把 pool.claim 关掉后重放迁移，不得覆盖显式记录
    sqlx::query(
        "UPDATE entitlements SET enabled = false WHERE scope = 'tenant' AND scope_id = $1 AND feature_key = 'pool.claim'",
    )
    .bind(tenant_id.to_string())
    .execute(&pool)
    .await
    .expect("disable pool.claim");
    crm_rb26164f::db::init_db(&database_url)
        .await
        .expect("re-run migrations again");
    let enabled: bool = sqlx::query_scalar(
        "SELECT enabled FROM entitlements WHERE scope = 'tenant' AND scope_id = $1 AND feature_key = 'pool.claim'",
    )
    .bind(tenant_id.to_string())
    .fetch_one(&pool)
    .await
    .expect("query pool.claim");
    assert!(!enabled, "迁移重放不得覆盖 owner 显式关掉的授权");
}

/// pool.claim 矩阵：有/无授权 × admin/member × 能/不能 + owner 关闭/恢复全流程
#[tokio::test]
async fn test_pool_claim_feature_matrix() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin20", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member20", "member123", "member").await;
    // setup_user 触发了迁移重放 → 当前租户已补种 pool.claim=true（存量全开）
    let tenant_id = current_tenant_id().await;

    // 准备一个公海客户
    let pool = test_pool().await;
    let customer_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO customers (tenant_id, company, contact_name, owner_id, source) \
         VALUES ($1, '分级矩阵公司', '联系人', NULL, 'manual') RETURNING id",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .expect("insert pool customer");

    // 正：有授权 member 认领 → 能
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 member 应能认领");

    // 正：有授权 member 释放 → 能
    let resp = client
        .post(&format!("{}/api/customers/{}/release", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 member 应能释放");

    // owner 端点负矩阵：无 token → 403；租户 admin token → 403（不得自改授权）
    let resp = client
        .put(&format!(
            "{}/api/admin/tenants/{}/entitlements",
            base, tenant_id
        ))
        .json(&serde_json::json!({ "pool.claim": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无 owner token 不得配置功能包");
    let resp = client
        .put(&format!(
            "{}/api/admin/tenants/{}/entitlements",
            base, tenant_id
        ))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "pool.claim": false }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "租户 admin 不得自改授权");

    // owner 关掉 pool.claim
    let resp = owner_put_features(&client, &base, tenant_id, serde_json::json!({ "pool.claim": false })).await;
    assert!(resp.status().is_success(), "owner 关闭应成功: {}", resp.status());
    let list: Value = resp.json().await.unwrap();
    let entry = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == "pool.claim")
        .unwrap()
        .clone();
    assert_eq!(entry["enabled"], false);
    assert_eq!(entry["source"], "entitlement");

    // 负：无授权 member 认领 → 403 带 feature key
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 member 不得认领");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "pool.claim");

    // 负：无授权 admin 认领 → 403（分级与角色正交，admin 也不豁免）
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 admin 不得认领");

    // 负：无授权 member 释放 → 403（先让 member 名下有客户：直接 SQL 指定 owner）
    let member_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM users WHERE tenant_id = $1 AND username = 'member20'",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .expect("member id");
    sqlx::query("UPDATE customers SET owner_id = $1 WHERE id = $2")
        .bind(member_id)
        .bind(customer_id)
        .execute(&pool)
        .await
        .expect("assign owner");
    let resp = client
        .post(&format!("{}/api/customers/{}/release", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 member 不得释放");

    // 租户侧清单也反映关闭结果
    let resp = client
        .get(&format!("{}/api/entitlements", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    let list: Value = resp.json().await.unwrap();
    let entry = list
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["key"] == "pool.claim")
        .unwrap()
        .clone();
    assert_eq!(entry["enabled"], false);

    // owner 再开回来 → 恢复
    let resp = owner_put_features(&client, &base, tenant_id, serde_json::json!({ "pool.claim": true })).await;
    assert!(resp.status().is_success());
    sqlx::query("UPDATE customers SET owner_id = NULL WHERE id = $1")
        .bind(customer_id)
        .execute(&pool)
        .await
        .expect("back to pool");
    let resp = client
        .post(&format!("{}/api/customers/{}/claim", base, customer_id))
        .header("Authorization", format!("Bearer {}", member_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "恢复授权后 member 应能再次认领");

    // 审计：两次 entitlement.update 均入日志
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_logs WHERE tenant_id = $1 AND action = 'entitlement.update'",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .expect("audit count");
    assert!(audit_count >= 2, "功能包变更应进审计日志, got {}", audit_count);
}

/// import.export 矩阵：有/无授权 × admin/member × 能/不能
#[tokio::test]
async fn test_import_export_feature_matrix() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin21", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member21", "member123", "member").await;
    let tenant_id = current_tenant_id().await;

    let csv_data = "公司名,联系人\n分级导入公司,钱九";

    // 正：有授权 admin 导入 → 能
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 admin 应能导入");

    // 正：有授权 admin 导出 → 能
    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 admin 应能导出");

    // owner 关掉 import.export
    let resp =
        owner_put_features(&client, &base, tenant_id, serde_json::json!({ "import.export": false })).await;
    assert!(resp.status().is_success());

    // 负：无授权 admin 导入 → 403 带 feature key
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 admin 不得导入");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "import.export");

    // 负：无授权 member 导入 → 403
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 member 不得导入");

    // 负：无授权 admin 导出 → 403
    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 admin 不得导出");

    // 恢复：owner 开回来 → admin 导出 200
    let resp =
        owner_put_features(&client, &base, tenant_id, serde_json::json!({ "import.export": true })).await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "恢复授权后 admin 应能导出");
}

/// notify.webhook 矩阵：有/无授权 × admin/member × 能/不能
#[tokio::test]
async fn test_notify_webhook_feature_matrix() {
    let base = start_server().await;
    let client = Client::new();
    let admin_token = setup_user(&client, &base, "admin22", "admin123", "admin").await;
    let member_token = setup_user(&client, &base, "member22", "member123", "member").await;
    let tenant_id = current_tenant_id().await;

    // 正：有授权 admin 保存 webhook 设置 → 能
    let resp = client
        .put(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "wecom_webhook_url": "https://example.com/hook" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 admin 应能保存 webhook 设置");

    // owner 关掉 notify.webhook
    let resp =
        owner_put_features(&client, &base, tenant_id, serde_json::json!({ "notify.webhook": false })).await;
    assert!(resp.status().is_success());

    // 负：无授权 admin 保存 → 403 带 feature key
    let resp = client
        .put(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "wecom_webhook_url": "https://example.com/hook2" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "无授权 admin 不得保存 webhook 设置");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "notify.webhook");

    // 负：member 保存 → 403（角色守卫在先，维持既有行为）
    let resp = client
        .put(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", member_token))
        .json(&serde_json::json!({ "wecom_webhook_url": "https://example.com/hook3" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "member 不得改设置（角色守卫）");

    // 恢复：owner 开回来 → admin 保存 200
    let resp =
        owner_put_features(&client, &base, tenant_id, serde_json::json!({ "notify.webhook": true })).await;
    assert!(resp.status().is_success());
    let resp = client
        .put(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", admin_token))
        .json(&serde_json::json!({ "wecom_webhook_url": "https://example.com/hook4" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "恢复授权后 admin 应能保存");

    // 未登记键负测试：owner 配置天书键 → 400
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "not.registered": true }),
    )
    .await;
    assert_eq!(resp.status(), 400, "未登记的功能键必须拒绝（登记纪律）");
}


// ── RB-266FB3：CRM 完整版重做 ─────────────────────────────────
// 状态机（合法/非法流转成对）、数据域正负矩阵、换人时间线、
// 逾期提醒、报表口径、导入去重、导出时间线摘要、时间线 entitlement 门

/// 辅助：按名字找阶段 id
fn stage_id_by_name(stages: &Value, name: &str) -> String {
    stages
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("stage {} not found", name))
        .id_str()
}

trait StageExt {
    fn id_str(&self) -> String;
}
impl StageExt for Value {
    fn id_str(&self) -> String {
        self["id"].as_str().unwrap().to_string()
    }
}

/// 辅助：PUT 客户阶段并返回响应
async fn put_customer_stage(
    client: &Client,
    base: &str,
    token: &str,
    customer_id: &str,
    stage_id: &str,
) -> reqwest::Response {
    client
        .put(&format!("{}/api/customers/{}", base, customer_id))
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "stage": stage_id }))
        .send()
        .await
        .expect("put customer stage")
}

/// 2.1 状态机：合法流转放行 / 非法流转 409（后端强制）
#[tokio::test]
async fn test_stage_machine_transitions() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "sm_admin", "admin123", "admin").await;

    // 搭一条完整链：线索(0)→意向(1)→报价(2)→赢单(10,终态)/输单(11,终态)
    let resp = client
        .get(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let stages: Value = resp.json().await.unwrap();
    let lead = stage_id_by_name(&stages, "线索");
    let won = stage_id_by_name(&stages, "赢单");
    let lost = stage_id_by_name(&stages, "输单");
    // 把两个终态挪到尾部，腾出中间位置
    for (sid, order) in [(&won, 10), (&lost, 11)] {
        let resp = client
            .put(&format!("{}/api/stages/{}", base, sid))
            .header("Authorization", format!("Bearer {}", admin))
            .json(&serde_json::json!({ "order_index": order }))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
    }
    let mut intent = String::new();
    let mut quote = String::new();
    for (name, order) in [("意向", 1), ("报价", 2)] {
        let resp = client
            .post(&format!("{}/api/stages", base))
            .header("Authorization", format!("Bearer {}", admin))
            .json(&serde_json::json!({ "name": name, "order_index": order }))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success());
        let s: Value = resp.json().await.unwrap();
        if name == "意向" {
            intent = s.id_str();
        } else {
            quote = s.id_str();
        }
    }

    // 正：初始放置（NULL → 任意活性阶段）
    let c1 = create_customer(&client, &base, &admin, "状态机公司1").await;
    let c1_id = c1.id_str();
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &lead).await;
    assert_eq!(resp.status(), 200, "初始放置线索应合法");

    // 正：前进一步
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &intent).await;
    assert_eq!(resp.status(), 200, "线索→意向（前进一步）应合法");
    // 正：再前进一步
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &quote).await;
    assert_eq!(resp.status(), 200, "意向→报价（前进一步）应合法");
    // 正：原地放置 no-op
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &quote).await;
    assert_eq!(resp.status(), 200, "原地放置应合法");
    // 负：跨格跳跃（报价→线索，跨 2 格）
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &lead).await;
    assert_eq!(resp.status(), 409, "报价→线索跨格跳跃必须拒绝");
    // 正：后退一步（报价→意向）
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &intent).await;
    assert_eq!(resp.status(), 200, "报价→意向（后退一步）应合法");
    // 负：跨格前进（意向→赢单之外的跨格已被终态规则覆盖，这里测 意向→线索+报价相邻外目标）
    // 意向→赢单：进终态，合法
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &won).await;
    assert_eq!(resp.status(), 200, "非终态→赢单（终态）应合法");
    // 负：终态不可出
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &intent).await;
    assert_eq!(resp.status(), 409, "赢单→意向（终态出）必须拒绝");
    let resp = put_customer_stage(&client, &base, &admin, &c1_id, &lost).await;
    assert_eq!(resp.status(), 409, "赢单→输单（终态互转）必须拒绝");

    // 负：目标阶段已停用
    let resp = client
        .post(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "name": "休眠", "order_index": 20 }))
        .send()
        .await
        .unwrap();
    let dormant: Value = resp.json().await.unwrap();
    let dormant_id = dormant.id_str();
    let resp = client
        .put(&format!("{}/api/stages/{}", base, dormant_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "is_active": false }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let c2 = create_customer(&client, &base, &admin, "状态机公司2").await;
    let c2_id = c2.id_str();
    let resp = put_customer_stage(&client, &base, &admin, &c2_id, &dormant_id).await;
    assert_eq!(resp.status(), 409, "停用阶段不可作为流转目标");

    // pipeline 条目更新同样走状态机，且同步 customers.stage
    let c3 = create_customer(&client, &base, &admin, "状态机公司3").await;
    let c3_id = c3.id_str();
    let resp = put_customer_stage(&client, &base, &admin, &c3_id, &intent).await;
    assert_eq!(resp.status(), 200);
    let resp = client
        .post(&format!("{}/api/pipeline", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "customer_id": c3_id, "stage_id": intent }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let entry: Value = resp.json().await.unwrap();
    let entry_id = entry.id_str();
    // 正：意向→报价（相邻）
    let resp = client
        .put(&format!("{}/api/pipeline/{}", base, entry_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "stage_id": quote }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "pipeline 意向→报价应合法");
    // customers.stage 已同步
    let resp = client
        .get(&format!("{}/api/customers/{}", base, c3_id))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["stage"].as_str().unwrap(), quote, "pipeline 改阶段必须同步 customers.stage");
    // 负：报价→线索（跨格）
    let resp = client
        .put(&format!("{}/api/pipeline/{}", base, entry_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "stage_id": lead }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "pipeline 报价→线索跨格必须拒绝");
}

/// 2.3 数据域矩阵：销售互不可见（客户/时间线/任务 404），老板全见（200）——正负成对
#[tokio::test]
async fn test_role_matrix_data_domain() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "dd_admin", "admin123", "admin").await;
    let sales_a = setup_user(&client, &base, "dd_sales_a", "member123", "member").await;
    let sales_b = setup_user(&client, &base, "dd_sales_b", "member123", "member").await;

    // 销售 A 建客户 + 跟进 + 待办
    let ca = create_customer(&client, &base, &sales_a, "A的客户公司").await;
    let ca_id = ca.id_str();
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_a))
        .json(&serde_json::json!({ "type": "call", "content": "A的电话跟进" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_a))
        .json(&serde_json::json!({ "title": "A的待办", "due_at": "2030-01-01T00:00:00Z" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // 负：销售 B 访问 A 的客户 → 404
    let resp = client
        .get(&format!("{}/api/customers/{}", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "销售B不得看销售A的客户");
    // 负：销售 B 访问 A 的时间线 → 404
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "销售B不得看销售A的时间线");
    // 负：销售 B 访问 A 的客户任务 → 404
    let resp = client
        .get(&format!("{}/api/customers/{}/tasks", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "销售B不得看销售A的客户任务");
    // 负：销售 B 在 A 的客户下写跟进 → 404
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .json(&serde_json::json!({ "type": "note", "content": "越权写入" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "销售B不得在销售A的客户下写跟进");
    // 负：销售 B 的任务列表不含 A 的待办
    let resp = client
        .get(&format!("{}/api/tasks", base))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    let tasks: Value = resp.json().await.unwrap();
    assert!(
        !tasks.as_array().unwrap().iter().any(|t| t["title"] == "A的待办"),
        "销售B任务列表不得含销售A的待办"
    );

    // 正：老板全部可见
    let resp = client
        .get(&format!("{}/api/customers/{}", base, ca_id))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "老板应能看任一销售的客户");
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "老板应能看任一销售的时间线");
    let acts: Value = resp.json().await.unwrap();
    assert_eq!(acts.as_array().unwrap().len(), 1);
    assert_eq!(
        acts[0]["author_name"].as_str().unwrap(),
        "dd_sales_a",
        "时间线必须带原作者名"
    );
    // 正：老板任务列表全团队口径
    let resp = client
        .get(&format!("{}/api/tasks", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let tasks: Value = resp.json().await.unwrap();
    let t = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["title"] == "A的待办")
        .expect("老板任务列表应含全团队任务");
    assert_eq!(t["customer_company"], "A的客户公司", "任务应带客户公司名");
    assert_eq!(t["assignee_name"], "dd_sales_a", "任务应带负责人名");
    // 正：销售 A 自己可见（证明不是一刀切 404）
    let resp = client
        .get(&format!("{}/api/customers/{}", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_a))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "销售A应能看自己的客户");
}

/// 2.2 换人：负责人变更后新负责人看到完整历史时间线，原作者可追溯
#[tokio::test]
async fn test_transfer_keeps_timeline() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "tf_admin", "admin123", "admin").await;
    let sales_a = setup_user(&client, &base, "tf_sales_a", "member123", "member").await;
    let sales_b = setup_user(&client, &base, "tf_sales_b", "member123", "member").await;

    // 取销售 B 的用户 id（/auth/me）
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    let me: Value = resp.json().await.unwrap();
    let sales_b_id = me["id"].as_str().unwrap().to_string();

    // 销售 A 建客户 + 两条跟进
    let ca = create_customer(&client, &base, &sales_a, "换人测试公司").await;
    let ca_id = ca.id_str();
    for content in ["A的第一次跟进", "A的第二次跟进"] {
        let resp = client
            .post(&format!("{}/api/customers/{}/activities", base, ca_id))
            .header("Authorization", format!("Bearer {}", sales_a))
            .json(&serde_json::json!({ "type": "call", "content": content }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
    }

    // 换人前：销售 B 看不到
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "换人前销售B不得可见");

    // 负：member 无权换人
    let resp = client
        .post(&format!("{}/api/customers/{}/transfer", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_a))
        .json(&serde_json::json!({ "owner_id": sales_b_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "member 不得执行换人");

    // 负：换给不存在的用户 → 400
    let resp = client
        .post(&format!("{}/api/customers/{}/transfer", base, ca_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "owner_id": uuid::Uuid::new_v4() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "换给不存在用户必须拒绝");

    // 正：admin 换人 A → B
    let resp = client
        .post(&format!("{}/api/customers/{}/transfer", base, ca_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "owner_id": sales_b_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "admin 换人应成功");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["owner_id"].as_str().unwrap(), sales_b_id);

    // 正：新负责人 B 看到完整历史时间线（含原作者名 + 交接记录）
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "新负责人应能读全部历史");
    let acts: Value = resp.json().await.unwrap();
    let arr = acts.as_array().unwrap();
    assert_eq!(arr.len(), 3, "两条历史跟进 + 一条交接记录: {}", acts);
    let contents: Vec<&str> = arr.iter().filter_map(|a| a["content"].as_str()).collect();
    assert!(contents.contains(&"A的第一次跟进"));
    assert!(contents.contains(&"A的第二次跟进"));
    assert!(
        contents.iter().any(|c| c.contains("负责人变更")),
        "时间线必须含交接记录: {:?}",
        contents
    );
    // 历史记录原作者仍是 A（可追溯）
    let a_record = arr
        .iter()
        .find(|a| a["content"] == "A的第一次跟进")
        .unwrap();
    assert_eq!(
        a_record["author_name"].as_str().unwrap(),
        "tf_sales_a",
        "换人后历史记录原作者必须保留"
    );

    // 正：新负责人可继续写跟进（接手不断档）
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_b))
        .json(&serde_json::json!({ "type": "note", "content": "B接手后的跟进" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    // 负：原负责人 A 换人后失去访问权（数据域随 owner 走）
    let resp = client
        .get(&format!("{}/api/customers/{}", base, ca_id))
        .header("Authorization", format!("Bearer {}", sales_a))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "原负责人换人后不得再访问");
}

/// 2.1 跟进提醒：逾期任务列表接口（口径 + entitlement 门）
#[tokio::test]
async fn test_overdue_tasks() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "od_admin", "admin123", "admin").await;
    let member = setup_user(&client, &base, "od_member", "member123", "member").await;
    let tenant_id = current_tenant_id().await;

    let c = create_customer(&client, &base, &member, "逾期测试公司").await;
    let c_id = c.id_str();

    // 已过期 pending 任务
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, c_id))
        .header("Authorization", format!("Bearer {}", member))
        .json(&serde_json::json!({ "title": "已过期任务", "due_at": "2020-01-01T00:00:00Z" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    // 未来截止任务（不应逾期）
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, c_id))
        .header("Authorization", format!("Bearer {}", member))
        .json(&serde_json::json!({ "title": "未来任务", "due_at": "2030-01-01T00:00:00Z" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    // 已过期但已完成（不应逾期）
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, c_id))
        .header("Authorization", format!("Bearer {}", member))
        .json(&serde_json::json!({ "title": "已完成过期任务", "due_at": "2020-06-01T00:00:00Z" }))
        .send()
        .await
        .unwrap();
    let done_task: Value = resp.json().await.unwrap();
    let resp = client
        .put(&format!("{}/api/tasks/{}", base, done_task.id_str()))
        .header("Authorization", format!("Bearer {}", member))
        .json(&serde_json::json!({ "status": "done" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 正：member 逾期列表只含「已过期任务」
    let resp = client
        .get(&format!("{}/api/tasks/overdue", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: Value = resp.json().await.unwrap();
    let arr = list.as_array().unwrap();
    assert_eq!(arr.len(), 1, "逾期列表应只含过期未完成任务: {}", list);
    assert_eq!(arr[0]["title"], "已过期任务");
    assert_eq!(arr[0]["customer_company"], "逾期测试公司");

    // 正：admin 逾期列表全团队口径（含 member 的逾期任务）
    let resp = client
        .get(&format!("{}/api/tasks/overdue", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let list: Value = resp.json().await.unwrap();
    assert!(
        list.as_array().unwrap().iter().any(|t| t["title"] == "已过期任务"),
        "老板逾期列表应含全团队逾期任务"
    );

    // entitlement：owner 关掉 crm.reminders → 403 带 feature；开回 → 200
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.reminders": false }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/tasks/overdue", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "crm.reminders 关闭后逾期接口必须 403");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "crm.reminders");
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.reminders": true }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/tasks/overdue", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "恢复授权后逾期接口应可用");
}

/// 2.4 报表：口径断言（本月新增/成交率）+ 角色口径 + entitlement 门
#[tokio::test]
async fn test_reports_summary() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "rp_admin", "admin123", "admin").await;
    let member = setup_user(&client, &base, "rp_member", "member123", "member").await;
    let tenant_id = current_tenant_id().await;

    let resp = client
        .get(&format!("{}/api/stages", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let stages: Value = resp.json().await.unwrap();
    let lead = stage_id_by_name(&stages, "线索");
    let won = stage_id_by_name(&stages, "赢单");

    // member：2 个客户，1 个推进到赢单；admin：1 个客户（不推进）
    let m1 = create_customer(&client, &base, &member, "报表成员公司1").await;
    let m1_id = m1.id_str();
    create_customer(&client, &base, &member, "报表成员公司2").await;
    create_customer(&client, &base, &admin, "报表老板公司").await;
    let resp = put_customer_stage(&client, &base, &member, &m1_id, &lead).await;
    assert_eq!(resp.status(), 200);
    let resp = put_customer_stage(&client, &base, &member, &m1_id, &won).await;
    assert_eq!(resp.status(), 200);

    // member 口径：只算自己
    let resp = client
        .get(&format!("{}/api/reports/summary", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let s: Value = resp.json().await.unwrap();
    assert_eq!(s["month_new_customers"], 2, "member 本月新增=2: {}", s);
    assert_eq!(s["advanced_total"], 1, "member 总推进=1: {}", s);
    assert_eq!(s["won_count"], 1, "member 赢单=1: {}", s);
    assert_eq!(s["win_rate"], 100.0, "member 成交率=100%: {}", s);

    // admin 口径：全团队
    let resp = client
        .get(&format!("{}/api/reports/summary", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    let s: Value = resp.json().await.unwrap();
    assert_eq!(s["month_new_customers"], 3, "admin 本月新增=3（全团队）: {}", s);
    assert_eq!(s["advanced_total"], 1, "admin 总推进=1: {}", s);
    assert_eq!(s["won_count"], 1, "admin 赢单=1: {}", s);

    // entitlement：owner 关掉 crm.reports → admin/member 均 403 带 feature；开回恢复
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.reports": false }),
    )
    .await;
    assert!(resp.status().is_success());
    for (token, who) in [(&member, "member"), (&admin, "admin")] {
        let resp = client
            .get(&format!("{}/api/reports/summary", base))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403, "crm.reports 关闭后 {} 必须 403", who);
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["feature"], "crm.reports");
    }
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.reports": true }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/reports/summary", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "恢复授权后报表应可用");
}

/// 2.5 导入去重策略 + 错误行报告
#[tokio::test]
async fn test_import_dedup_and_error_report() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "dedup_admin", "admin123", "admin").await;

    // 既有客户：公司名「既有公司」、电话 13800000001
    let resp = client
        .post(&format!("{}/api/customers", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "company": "既有公司", "phone": "13800000001" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // 三行：公司名重复 / 电话重复 / 全新
    let csv_data = "公司名,联系人,电话,邮箱,地址,备注,阶段\n既有公司,张三,13999999999,,,,\n新公司但电话重复,李四,13800000001,,,,\n全新公司,王五,13800000002,,,,\n";
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success_rows"], 1, "只有全新公司应导入成功: {}", body);
    assert_eq!(body["failed_rows"], 2, "两行重复应计入失败: {}", body);
    let errors = body["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "错误行报告应含两条: {}", body);
    assert!(
        errors.iter().all(|e| e["error"].as_str().unwrap().contains("重复")),
        "重复行必须说明去重原因: {}",
        body
    );
    assert!(
        errors.iter().all(|e| e["row"].is_number()),
        "错误行报告必须带行号: {}",
        body
    );

    // 再次导入同一文件：三行全部重复
    let resp = client
        .post(&format!("{}/api/import", base))
        .header("Authorization", format!("Bearer {}", admin))
        .header("Content-Type", "text/csv")
        .body(csv_data.to_string())
        .send()
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success_rows"], 0, "重复导入应全部跳过: {}", body);
    assert_eq!(body["failed_rows"], 3, "重复导入三行全部失败: {}", body);
}

/// 2.5 导出：含时间线摘要列（跟进次数/最近跟进）+ UTF-8 BOM
#[tokio::test]
async fn test_export_timeline_summary() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "exp_admin", "admin123", "admin").await;

    let c = create_customer(&client, &base, &admin, "导出摘要公司").await;
    let c_id = c.id_str();
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, c_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "type": "meeting", "content": "最近跟进内容XYZ" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = client
        .get(&format!("{}/api/export/customers.csv", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.unwrap();
    assert!(text.starts_with('\u{feff}'), "CSV 必须带 UTF-8 BOM（Excel 兼容）");
    assert!(text.contains("跟进次数"), "导出必须含跟进次数列: {}", text);
    assert!(text.contains("最近跟进时间"), "导出必须含最近跟进时间列");
    assert!(text.contains("最近跟进内容XYZ"), "导出必须含时间线摘要: {}", text);
}

/// 2.6 时间线 entitlement 接入：crm.activities 关闭后后端 403
#[tokio::test]
async fn test_activities_feature_gate() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "tl_admin", "admin123", "admin").await;
    let tenant_id = current_tenant_id().await;

    let c = create_customer(&client, &base, &admin, "时间线门公司").await;
    let c_id = c.id_str();
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, c_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "type": "note", "content": "门控前跟进" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "默认（tier1 开）应能写跟进");

    // owner 关掉 crm.activities → GET/POST 均 403 带 feature key
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.activities": false }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, c_id))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "crm.activities 关闭后读时间线必须 403");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "crm.activities");
    let resp = client
        .post(&format!("{}/api/customers/{}/activities", base, c_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "type": "note", "content": "门控后跟进" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "crm.activities 关闭后写跟进必须 403");

    // 恢复
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.activities": true }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/customers/{}/activities", base, c_id))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "恢复授权后时间线应可用");
}

// ── req-crm-jwt-fix：JWT fail-fast / settings 凭据守卫 / 功能门补齐 ──────────

/// F1（C-P0-1）：未配置 JWT_SECRET（含空串/纯空白）→ 进程拒绝启动且明确报错。
/// 用独立子进程验证"启动即拒"，不污染测试进程环境；超时保护防止回归成"挂起不退出"。
#[test]
fn test_jwt_secret_missing_refuses_startup() {
    let cases: [(&str, Option<&str>); 3] = [
        ("未配置", None),
        ("空串", Some("")),
        ("纯空白", Some("   ")),
    ];
    for (label, secret) in cases {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_crm-rb26164f"));
        cmd.env_remove("JWT_SECRET")
            .env("PORT", "0")
            .env(
                "DATABASE_URL",
                "postgres://postgres:postgres@localhost:5432/crm_rb26164f_test",
            )
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(value) = secret {
            cmd.env("JWT_SECRET", value);
        }

        let mut child = cmd.spawn().expect("spawn crm binary");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match child.try_wait().expect("try_wait") {
                Some(_) => break,
                None if std::time::Instant::now() >= deadline => {
                    child.kill().ok();
                    panic!("{label}: 缺 JWT_SECRET 时进程没有拒绝启动（10s 仍在运行）");
                }
                None => std::thread::sleep(std::time::Duration::from_millis(50)),
            }
        }
        let out = child.wait_with_output().expect("collect output");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{label}: 缺 JWT_SECRET 必须非 0 退出");
        assert!(
            stderr.contains("JWT_SECRET"),
            "{label}: 报错必须点名 JWT_SECRET，实际 stderr={stderr}"
        );
    }
}

/// F2 回归：旧源码公开常量 `dev-secret` 签发的 token 必须 401；
/// 同一时刻配置密钥签发的正常登录 token 必须可用（正负成对）。
#[tokio::test]
async fn test_old_public_default_secret_token_rejected() {
    let base = start_server().await;
    let client = Client::new();
    let tenant_id = current_tenant_id().await;

    let now = chrono::Utc::now();
    let forged_claims = serde_json::json!({
        "sub": uuid::Uuid::new_v4().to_string(),
        "username": "forged-admin",
        "role": "admin",
        "tenant_id": tenant_id.to_string(),
        "iat": now.timestamp(),
        "exp": (now + chrono::Duration::hours(1)).timestamp(),
    });
    let forged = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &forged_claims,
        &jsonwebtoken::EncodingKey::from_secret(b"dev-secret"),
    )
    .expect("sign forged token");

    // 负：旧公开常量签发 → 401
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .bearer_auth(&forged)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "旧公开常量 dev-secret 签发的 token 必须被拒");

    // 正：配置密钥签发的真实登录 token → 200
    let token = setup_user(&client, &base, "admin-jwt", "admin123", "admin").await;
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "配置密钥签发的正常 token 应可用");
}

/// F3（C-P1-1）：admin GET /settings 见 webhook 明文；member 只见「已配置/未配置」布尔，
/// 响应体里不得出现 URL 明文（正负成对）。
#[tokio::test]
async fn test_settings_webhook_redacted_for_member() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "admin-settings", "admin123", "admin").await;
    let member = setup_user(&client, &base, "member-settings", "member123", "member").await;
    let hook = "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key=secret-key-abc";

    // 负：未授权读 → 401
    let resp = client
        .get(&format!("{}/api/settings", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "未授权不得读设置");

    // 正：admin 写入（一个已配置、一个未配置）
    let resp = client
        .put(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({
            "wecom_webhook_url": hook,
            "dingtalk_webhook_url": "",
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "admin 应能保存 webhook 设置");

    // 正：admin 读 → 明文
    let resp = client
        .get(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let admin_body: Value = resp.json().await.unwrap();
    assert_eq!(admin_body["wecom_webhook_url"], hook, "admin 应看到 URL 明文");
    assert_eq!(admin_body["dingtalk_webhook_url"], "", "admin 应看到原始值");

    // 负：member 读 → 布尔，且响应体不含 URL 明文
    let resp = client
        .get(&format!("{}/api/settings", base))
        .header("Authorization", format!("Bearer {}", member))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "member 可读设置（但内容脱敏）");
    let member_body: Value = resp.json().await.unwrap();
    assert_eq!(
        member_body["wecom_webhook_url"], true,
        "member 只应看到「已配置」布尔"
    );
    assert_eq!(
        member_body["dingtalk_webhook_url"], false,
        "member 只应看到「未配置」布尔"
    );
    assert!(
        !member_body.to_string().contains("qyapi.weixin.qq.com"),
        "member 响应体不得出现 webhook URL 明文: {member_body}"
    );
}

/// F4（C-P2-2）：/customers/check-duplicate、/dashboard、/import/template 三处
/// 未授权 401、有授权 200、功能关闭 403（带 feature key），全部正负成对。
#[tokio::test]
async fn test_check_duplicate_dashboard_template_gates() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "admin-gates", "admin123", "admin").await;
    let tenant_id = current_tenant_id().await;

    // ── 未授权：三处全部 401（template 此前完全公开，是本次修复点） ──
    let resp = client
        .post(&format!("{}/api/customers/check-duplicate", base))
        .json(&serde_json::json!({ "company": "任意公司" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "check-duplicate 未授权必须 401");

    let resp = client
        .get(&format!("{}/api/dashboard", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "dashboard 未授权必须 401");

    let resp = client
        .get(&format!("{}/api/import/template", base))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "import/template 未授权必须 401");

    // ── 有授权：三处全部 200 ──
    let resp = client
        .post(&format!("{}/api/customers/check-duplicate", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "company": "任意公司" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 check-duplicate 应可用");

    let resp = client
        .get(&format!("{}/api/dashboard", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权 dashboard 应可用");

    let resp = client
        .get(&format!("{}/api/import/template", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "有授权模板下载应可用");

    // ── 负：关掉对应功能 → 403 且带 feature key ──
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.customers": false, "crm.funnel": false, "import.export": false }),
    )
    .await;
    assert!(resp.status().is_success());

    let resp = client
        .post(&format!("{}/api/customers/check-duplicate", base))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({ "company": "任意公司" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "crm.customers 关闭后 check-duplicate 必须 403");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "crm.customers");

    let resp = client
        .get(&format!("{}/api/dashboard", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "crm.funnel 关闭后 dashboard 必须 403");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "crm.funnel");

    let resp = client
        .get(&format!("{}/api/import/template", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "import.export 关闭后模板下载必须 403");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["feature"], "import.export");

    // ── 恢复：三处回到可用 ──
    let resp = owner_put_features(
        &client,
        &base,
        tenant_id,
        serde_json::json!({ "crm.customers": true, "crm.funnel": true, "import.export": true }),
    )
    .await;
    assert!(resp.status().is_success());
    let resp = client
        .get(&format!("{}/api/import/template", base))
        .header("Authorization", format!("Bearer {}", admin))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "恢复授权后模板下载应可用");
}

// ── 到期提醒调度器（spec: req-crm-reminder-scheduler）──────────────────────
// 单轮驱动入口 `services::reminder::run_once`，测试无需等 60s interval。
// 建单测试任务时多数用例直接落库，便于精确控制 due_at / 渠道；API 全链用例
// 单独走 POST /tasks（见 test_reminder_full_chain_via_task_api）。

/// 最小 HTTP/1.1 mock webhook：读完请求（按 Content-Length）后回固定 status。
/// 返回 (URL, 请求计数, 请求体列表)；计数即「上游收到提醒」的次数，
/// 请求体用于断言提醒正文（spec 需求 4：content 含客户名与 due_at）。
async fn spawn_mock_webhook(
    status: u16,
) -> (
    String,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock webhook");
    let addr = listener.local_addr().expect("mock webhook addr");
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_inner = counter.clone();
    let bodies = Arc::new(Mutex::new(Vec::new()));
    let bodies_inner = bodies.clone();

    tokio::spawn(async move {
        loop {
            let (mut sock, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break, // 测试 runtime 结束时 listener 关闭
            };
            let counter = counter_inner.clone();
            let bodies = bodies_inner.clone();
            tokio::spawn(async move {
                // 必须读完请求体再回包：提前关闭连接会让 reqwest 读到 RST 而非响应
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => data.extend_from_slice(&buf[..n]),
                        Err(_) => break,
                    }
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..pos]);
                        let body_len = head
                            .lines()
                            .find_map(|line| {
                                let (k, v) = line.split_once(':')?;
                                if k.eq_ignore_ascii_case("content-length") {
                                    v.trim().parse::<usize>().ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        if data.len() >= pos + 4 + body_len {
                            break;
                        }
                    }
                }
                // 记下完整请求体（循环已按 Content-Length 读满），供正文断言
                let body_text = data
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|pos| String::from_utf8_lossy(&data[pos + 4..]).to_string())
                    .unwrap_or_default();
                bodies
                    .lock()
                    .expect("mock webhook bodies lock")
                    .push(body_text);
                counter.fetch_add(1, Ordering::SeqCst);
                let reason = if (200..300).contains(&status) { "OK" } else { "Not Found" };
                let body = "{}";
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });

    (format!("http://{}/webhook", addr), counter, bodies)
}

/// 写租户级 webhook 设置（调度器经 settings 读取；空值才会回退环境变量）
async fn set_webhook_url(pool: &sqlx::PgPool, tenant_id: uuid::Uuid, key: &str, url: &str) {
    sqlx::query(
        "INSERT INTO settings (tenant_id, key, value) VALUES ($1, $2, $3) \
         ON CONFLICT (tenant_id, key) DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(tenant_id)
    .bind(key)
    .bind(url)
    .execute(pool)
    .await
    .expect("写入 webhook 设置");
}

/// 直接落库一条到期任务（绕过 POST /tasks 的建单即发），返回 task id
async fn insert_due_task(
    pool: &sqlx::PgPool,
    tenant_id: uuid::Uuid,
    channel: &str,
    due_offset_secs: i64,
) -> uuid::Uuid {
    let user_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO users (tenant_id, username, password_hash, role) \
         VALUES ($1, 'reminder-fixture', 'x', 'member') \
         ON CONFLICT (tenant_id, username) DO UPDATE SET role = 'member' RETURNING id",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await
    .expect("fixture user");

    let customer_id: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO customers (tenant_id, company, owner_id, source) \
         VALUES ($1, '提醒调度测试公司', $2, 'manual') RETURNING id",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("fixture customer");

    let due_at = chrono::Utc::now() + chrono::Duration::seconds(due_offset_secs);
    sqlx::query_scalar(
        "INSERT INTO tasks (tenant_id, customer_id, user_id, title, due_at, reminder_channel) \
         VALUES ($1, $2, $3, '到期提醒测试任务', $4, $5::reminder_channel) RETURNING id",
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(user_id)
    .bind(due_at)
    .bind(channel)
    .fetch_one(pool)
    .await
    .expect("fixture task")
}

/// 读任务的 notified_at（None = 未发送/已回滚待重试）
async fn task_notified_at(
    pool: &sqlx::PgPool,
    task_id: uuid::Uuid,
) -> Option<chrono::DateTime<chrono::Utc>> {
    sqlx::query_scalar::<_, Option<chrono::DateTime<chrono::Utc>>>(
        "SELECT notified_at FROM tasks WHERE id = $1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .expect("查询 notified_at")
}

/// F1/F2（全链）：到期 wecom 任务 → 上游恰好 1 次提醒 + tasks.notified_at 非空；
/// 未到期任务不发；再跑两轮不重发。负向：webhook 不可达时不标 notified_at。
#[tokio::test]
async fn test_reminder_scheduler_fires_once_on_due() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    let (url, hits, bodies) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &url).await;

    let due_task = insert_due_task(&pool, tenant_id, "wecom", -1).await;
    let future_task = insert_due_task(&pool, tenant_id, "wecom", 300).await;

    let sent = crm_rb26164f::services::reminder::run_once(&pool)
        .await
        .expect("run_once");
    assert_eq!(sent, 1, "只应发送已到期的那条任务");
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "上游应恰好收到 1 次提醒");
    // 需求 4：content 必须含客户名与 due_at（不能只断言「发了」）
    let sent_body = {
        let list = bodies.lock().expect("mock webhook bodies lock");
        list.first().cloned().expect("上游应收到提醒正文")
    };
    assert!(
        sent_body.contains("提醒调度测试公司"),
        "提醒正文应含客户名: {sent_body}"
    );
    let due_at: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT due_at FROM tasks WHERE id = $1")
            .bind(due_task)
            .fetch_one(&pool)
            .await
            .expect("查询 due_at");
    assert!(
        sent_body.contains(&due_at.to_rfc3339()),
        "提醒正文应含 due_at（{}）: {sent_body}",
        due_at.to_rfc3339()
    );
    assert!(
        task_notified_at(&pool, due_task).await.is_some(),
        "发送成功后 notified_at 必须非空"
    );
    assert!(
        task_notified_at(&pool, future_task).await.is_none(),
        "未到期任务不得标记 notified_at"
    );

    // 连跑两轮：同一任务不重复发送
    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once round2"),
        0
    );
    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once round3"),
        0
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "多轮扫描后上游仍只应收到 1 次提醒"
    );
}

/// spec 约束原文：建 `due_at = now()+2s`、channel=wecom 的任务 → 到点后 notified_at 非空。
/// 正反两侧都断言：未到点不发、不标记；到点（真实等待越过 due_at）后恰好发 1 次。
#[tokio::test]
async fn test_reminder_scheduler_waits_until_due() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    let (url, hits, _bodies) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &url).await;

    let task_id = insert_due_task(&pool, tenant_id, "wecom", 2).await;

    // 未到点：扫描不发、不标记
    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once (before due)"),
        0,
        "未到点不得发送"
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "未到点上游不得收到提醒"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_none(),
        "未到点不得标记 notified_at"
    );

    // 真实等到越过 due_at
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;

    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once (after due)"),
        1,
        "到点后应发送"
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "到点后上游恰好收到 1 次提醒"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_some(),
        "到点发送成功后 notified_at 非空"
    );
}

/// F2：并发两实例（两个 run_once 同时扫同一批到期任务）→ 同一任务只发 1 次
#[tokio::test]
async fn test_reminder_scheduler_concurrent_instances_send_once() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    let (url, hits, _bodies) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &url).await;

    let task_id = insert_due_task(&pool, tenant_id, "wecom", -1).await;

    let (a, b) = tokio::join!(
        crm_rb26164f::services::reminder::run_once(&pool),
        crm_rb26164f::services::reminder::run_once(&pool)
    );
    let total = a.expect("instance a") + b.expect("instance b");
    assert_eq!(total, 1, "并发两实例合计只应发送 1 条任务");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "并发抢占下上游只应收到 1 次提醒"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_some(),
        "抢占成功后 notified_at 非空"
    );
}

/// F3：webhook 404 → notified_at 回 NULL（下轮重试）→ 修复 URL 后轮二成功
#[tokio::test]
async fn test_reminder_scheduler_retries_after_webhook_failure() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;

    let (bad_url, bad_hits, _bad_bodies) = spawn_mock_webhook(404).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &bad_url).await;
    let task_id = insert_due_task(&pool, tenant_id, "wecom", -1).await;

    // 轮一：404 → 发送失败，标记回滚为 NULL 供重试
    let sent = crm_rb26164f::services::reminder::run_once(&pool)
        .await
        .expect("run_once (failing webhook)");
    assert_eq!(sent, 0, "404 不应计入成功数");
    assert_eq!(bad_hits.load(std::sync::atomic::Ordering::SeqCst), 1, "失败路径确实请求了上游");
    assert!(
        task_notified_at(&pool, task_id).await.is_none(),
        "发送失败必须把 notified_at 清回 NULL"
    );

    // 轮二：修复 URL → 重试成功
    let (good_url, good_hits, _good_bodies) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &good_url).await;
    let sent = crm_rb26164f::services::reminder::run_once(&pool)
        .await
        .expect("run_once (recovered webhook)");
    assert_eq!(sent, 1, "修复 URL 后重试应成功");
    assert_eq!(good_hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        task_notified_at(&pool, task_id).await.is_some(),
        "重试成功后 notified_at 非空"
    );

    // 轮三：不重复发送
    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once round3"),
        0
    );
    assert_eq!(
        good_hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "成功后不得重复发送"
    );
}

/// F4：system 渠道到期 → 不请求 webhook、不报错，只标 notified_at
#[tokio::test]
async fn test_reminder_scheduler_system_channel_marks_without_webhook() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    // 故意配置一个会失败的 webhook：system 渠道根本不该请求它
    let (url, hits, _bodies) = spawn_mock_webhook(500).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &url).await;

    let task_id = insert_due_task(&pool, tenant_id, "system", -1).await;
    let sent = crm_rb26164f::services::reminder::run_once(&pool)
        .await
        .expect("system 渠道不应报错");
    assert_eq!(sent, 1, "system 渠道视为发送完成");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "system 渠道不得请求 webhook"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_some(),
        "system 渠道到期应直接标记 notified_at"
    );

    // 已标记后不重复处理
    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once round2"),
        0
    );
}

/// 约束：webhook URL 明文（含机器人 key）不得出现在错误信息/日志里。
/// 用连不通的地址触发 reqwest 网络错误（该错误默认会带 URL），断言已被剥离。
#[tokio::test]
async fn test_reminder_error_message_does_not_leak_webhook_url() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    let secret_url = "http://127.0.0.1:1/hook?key=top-secret-key";
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", secret_url).await;

    let err = crm_rb26164f::services::webhook::send_reminder(
        &pool,
        tenant_id,
        "wecom",
        "提醒标题",
        "提醒正文",
    )
    .await
    .expect_err("连不通的 webhook 必须报错（供调度器回滚重试）");
    let msg = format!("{err:#}");
    assert!(!msg.contains("top-secret-key"), "错误信息不得泄露 webhook key: {msg}");
    assert!(!msg.contains("127.0.0.1:1"), "错误信息不得包含 webhook URL: {msg}");
}

/// F1/F2 全链（API 建单 → 到期调度 → 上游）：
/// POST /tasks 建一条已到期的 wecom 任务 → 建单本身不得发提醒（否则建单+到期会重复），
/// 调度单轮后上游恰好 1 次、tasks.notified_at 非空；再跑不重复。
#[tokio::test]
async fn test_reminder_full_chain_via_task_api() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "reminder_api_admin", "admin123", "admin").await;
    let pool = test_pool().await;
    let tenant_id = current_tenant_id().await;
    let (url, hits, _bodies) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_id, "wecom_webhook_url", &url).await;

    let customer = create_customer(&client, &base, &admin, "全链提醒公司").await;
    let customer_id = customer["id"].as_str().expect("customer id");
    let resp = client
        .post(&format!("{}/api/customers/{}/tasks", base, customer_id))
        .header("Authorization", format!("Bearer {}", admin))
        .json(&serde_json::json!({
            "title": "全链到期任务",
            "due_at": "2020-01-01T00:00:00Z",
            "reminder_channel": "wecom",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "建单应成功");
    let task: Value = resp.json().await.unwrap();
    let task_id = uuid::Uuid::parse_str(task["id"].as_str().expect("task id")).expect("uuid");
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "建单接口不得即时发送提醒（到点由调度器发送恰好 1 次）"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_none(),
        "建单后尚未发送，notified_at 应为 NULL"
    );

    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once"),
        1
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "到期后上游应恰好收到 1 次提醒"
    );
    assert!(
        task_notified_at(&pool, task_id).await.is_some(),
        "调度发送成功后 notified_at 非空"
    );

    assert_eq!(
        crm_rb26164f::services::reminder::run_once(&pool)
            .await
            .expect("run_once round2"),
        0
    );
    assert_eq!(
        hits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "同一任务不得重复发送"
    );
}

/// 租户隔离（正负成对）：调度器是后台任务、绕过用户鉴权，
/// 必须按任务自身 tenant_id 取该租户的 webhook 设置——两个租户各一条到期任务，
/// 各自上游恰好 1 次，不能串到对方（错租户会表现为一边 2 次、一边 0 次）。
#[tokio::test]
async fn test_reminder_scheduler_tenant_scoped() {
    let _base = start_server().await;
    let pool = test_pool().await;
    let tenant_a = current_tenant_id().await;
    let tenant_b: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO tenants (name, code) VALUES ('提醒租户B', 'reminder-b') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("tenant b");

    let (url_a, hits_a, _bodies_a) = spawn_mock_webhook(200).await;
    let (url_b, hits_b, _bodies_b) = spawn_mock_webhook(200).await;
    set_webhook_url(&pool, tenant_a, "wecom_webhook_url", &url_a).await;
    set_webhook_url(&pool, tenant_b, "wecom_webhook_url", &url_b).await;

    let task_a = insert_due_task(&pool, tenant_a, "wecom", -1).await;
    let task_b = insert_due_task(&pool, tenant_b, "wecom", -1).await;

    let sent = crm_rb26164f::services::reminder::run_once(&pool)
        .await
        .expect("run_once");
    assert_eq!(sent, 2, "两个租户各一条到期任务都应发送");
    assert_eq!(
        hits_a.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "租户 A 的 webhook 只应收到自己那条"
    );
    assert_eq!(
        hits_b.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "租户 B 的 webhook 只应收到自己那条"
    );
    assert!(task_notified_at(&pool, task_a).await.is_some());
    assert!(task_notified_at(&pool, task_b).await.is_some());
}

// ── req-crm-member-lifecycle：成员停用/启用/重置密码 + 旧 token 失效 ─────────

/// 按用户名取 user id（setup_user 只回 token）
async fn user_id_by_name(username: &str) -> uuid::Uuid {
    let pool = test_pool().await;
    sqlx::query_scalar("SELECT id FROM users WHERE username = $1")
        .bind(username)
        .fetch_one(&pool)
        .await
        .expect("user id")
}

/// 登录并返回 (状态码, 响应体)
async fn login_body(
    client: &Client,
    base: &str,
    username: &str,
    password: &str,
) -> (reqwest::StatusCode, Value) {
    let resp = client
        .post(&format!("{}/api/auth/login", base))
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .expect("login request");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// 调管理端点：POST /api/users/:id/{action}
async fn manage_user(
    client: &Client,
    base: &str,
    token: &str,
    user_id: uuid::Uuid,
    action: &str,
    body: Option<Value>,
) -> reqwest::Response {
    let mut req = client
        .post(&format!("{}/api/users/{}/{}", base, user_id, action))
        .header("Authorization", format!("Bearer {}", token));
    if let Some(body) = body {
        req = req.json(&body);
    }
    req.send().await.expect("manage user request")
}

/// F1 + F2（全链）：admin 停用 member →
/// users.is_active=false 且登录 401（文案与密码错误一致）且已签发旧 token 401；
/// 重新启用 → users.is_active=true 且可正常登录。
#[tokio::test]
async fn test_member_lifecycle_deactivate_revokes_login_and_old_token() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "life-admin", "admin123", "admin").await;
    let member = setup_user(&client, &base, "life-member", "member123", "member").await;
    let member_id = user_id_by_name("life-member").await;

    // 正基线：用户详情（/auth/me）响应必须带 is_active
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "启用中的成员应能读取本人详情");
    let me: Value = resp.json().await.unwrap();
    assert_eq!(me["is_active"], true, "用户详情响应必须带 is_active=true");

    // 正基线：停用前 member 的 token 可用
    let resp = client
        .get(&format!("{}/api/customers", base))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "停用前 member token 应可用");

    // 密码错误的基线文案（停用后的报错必须与之完全一致，防账号枚举）
    let (status, body) = login_body(&client, &base, "life-member", "wrong-password").await;
    assert_eq!(status, 401, "错误密码应 401");
    let wrong_password_error = body["error"]
        .as_str()
        .expect("登录错误文案")
        .to_string();

    // ── A 处操作：admin 停用 member ──
    let resp = manage_user(&client, &base, &admin, member_id, "deactivate", None).await;
    assert_eq!(resp.status(), 200, "admin 停用 member 应成功");
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["is_active"], false, "停用响应应带 is_active=false");
    assert_eq!(updated["username"], "life-member");

    // ── B 处最终状态（全链断言）──
    // ① 库内状态
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
        .bind(member_id)
        .fetch_one(&test_pool().await)
        .await
        .expect("read is_active");
    assert!(!is_active, "停用后 users.is_active 必须为 false");
    // ② 正确密码登录 401，且文案与密码错误完全一致
    let (status, body) = login_body(&client, &base, "life-member", "member123").await;
    assert_eq!(status, 401, "停用后即使密码正确也必须 401");
    assert_eq!(
        body["error"].as_str().expect("停用登录错误文案"),
        wrong_password_error,
        "停用报错文案必须与密码错误一致（防账号枚举）"
    );
    // ③ 停用前签发的旧 token 立即 401（无需等过期）
    let resp = client
        .get(&format!("{}/api/customers", base))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "停用后已签发的旧 token 请求必须 401");
    let resp = client
        .get(&format!("{}/api/auth/me", base))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "停用后旧 token 访问详情也必须 401");
    // ④ 写端点同样被拦（"一律 401" 不只覆盖读端点）：请求体合法也必须先被鉴权拒绝
    let resp = client
        .post(&format!("{}/api/customers", base))
        .bearer_auth(&member)
        .json(&serde_json::json!({ "company": "停用后不应创建" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "停用后旧 token 调写端点也必须 401");

    // 列表响应带 is_active（admin 可见）
    let resp = client
        .get(&format!("{}/api/users", base))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    let users: Value = resp.json().await.unwrap();
    let listed = users
        .as_array()
        .expect("user list array")
        .iter()
        .find(|u| u["username"] == "life-member")
        .expect("list 中应包含目标成员");
    assert_eq!(listed["is_active"], false, "列表响应必须带 is_active");

    // ── F2：重新启用 ──
    let resp = manage_user(&client, &base, &admin, member_id, "activate", None).await;
    assert_eq!(resp.status(), 200, "admin 启用 member 应成功");
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["is_active"], true, "启用响应应带 is_active=true");
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
        .bind(member_id)
        .fetch_one(&test_pool().await)
        .await
        .expect("read is_active");
    assert!(is_active, "启用后 users.is_active 必须为 true");

    // 启用后可正常登录
    let (status, body) = login_body(&client, &base, "life-member", "member123").await;
    assert_eq!(status, 200, "启用后应可重新登录");
    let fresh_token = body["token"].as_str().expect("token").to_string();
    let resp = client
        .get(&format!("{}/api/customers", base))
        .bearer_auth(&fresh_token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "启用后新 token 应可用");
    // 说明：旧 token 的有效性由 is_active 实时决定（spec 第 4 条即按 token sub 查库校验），
    // 因此启用后同一 token 也恢复可用；若要「一次性永久吊销」需 token 版本化，本单未做。
    let resp = client
        .get(&format!("{}/api/customers", base))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "启用后旧 token 随 is_active 恢复可用（本单吊销语义 = 停用期间一律 401）"
    );
}

/// F3（正负成对）：admin 可调停用/启用/重置；member 调同三个端点一律 403；无 token 401。
#[tokio::test]
async fn test_member_lifecycle_admin_only() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "role-admin", "admin123", "admin").await;
    let member = setup_user(&client, &base, "role-member", "member123", "member").await;
    let target_token = setup_user(&client, &base, "role-target", "member123", "member").await;
    let target_id = user_id_by_name("role-target").await;

    // 正：admin 能调（用启用做正向控件，不改状态也能验证 200）
    let resp = manage_user(&client, &base, &admin, target_id, "activate", None).await;
    assert_eq!(resp.status(), 200, "admin 调启用应 200");

    // 负：member 调三个端点全部 403
    for action in ["deactivate", "activate", "reset-password"] {
        let body = if action == "reset-password" {
            Some(serde_json::json!({ "password": "newpass123" }))
        } else {
            None
        };
        let resp = manage_user(&client, &base, &member, target_id, action, body).await;
        assert_eq!(
            resp.status(),
            403,
            "member 调 /users/:id/{} 必须 403",
            action
        );
    }
    // 负：即使请求体畸形，member 也应先收到 403（权限判定先于请求体解析）
    let resp = client
        .post(&format!("{}/api/users/{}/reset-password", base, target_id))
        .bearer_auth(&member)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "member 带畸形请求体也必须 403（不得先报 400）");

    // 未认证一律 401
    let resp = client
        .post(&format!("{}/api/users/{}/deactivate", base, target_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401, "无 token 调管理端点必须 401");

    // 负向调用不得改变目标状态
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
        .bind(target_id)
        .fetch_one(&test_pool().await)
        .await
        .expect("read is_active");
    assert!(is_active, "member 的 403 调用不得改状态");
    // member token 仍可用（admin 的正向 activate 没有影响）
    let resp = client
        .get(&format!("{}/api/customers", base))
        .bearer_auth(&target_token)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
}

/// F4 自锁防线（正负成对）：
/// 有多名 admin 时停自己 → 400；把其他 admin 停掉、只剩自己是最后一名启用 admin → 409。
#[tokio::test]
async fn test_member_lifecycle_self_lock_guards() {
    let base = start_server().await;
    let client = Client::new();
    let admin_a = setup_user(&client, &base, "lock-admin-a", "admin123", "admin").await;
    let _admin_b = setup_user(&client, &base, "lock-admin-b", "admin123", "admin").await;
    let admin_a_id = user_id_by_name("lock-admin-a").await;
    let admin_b_id = user_id_by_name("lock-admin-b").await;

    // 负：多 admin 时停自己 → 400（禁止停用自己）
    let resp = manage_user(&client, &base, &admin_a, admin_a_id, "deactivate", None).await;
    assert_eq!(resp.status(), 400, "停用自己必须被拒");
    // 负向调用不改状态
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
        .bind(admin_a_id)
        .fetch_one(&test_pool().await)
        .await
        .expect("read is_active");
    assert!(is_active, "被拒的停自己不得改状态");

    // 正：停用另一名 admin 成功（此刻启用中 admin 还剩 admin_a 一人）
    let resp = manage_user(&client, &base, &admin_a, admin_b_id, "deactivate", None).await;
    assert_eq!(resp.status(), 200, "停用另一名 admin 应成功");

    // 负：admin_a 已是最后一名启用 admin → 停自己 409（最后管理员防线）
    let resp = manage_user(&client, &base, &admin_a, admin_a_id, "deactivate", None).await;
    assert_eq!(
        resp.status(),
        409,
        "停用本租户最后一个启用管理员必须被拒（409）"
    );
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap_or("").contains("最后一个管理员"),
        "报错应说明最后管理员防线，实际: {body}"
    );
    let is_active: bool = sqlx::query_scalar("SELECT is_active FROM users WHERE id = $1")
        .bind(admin_a_id)
        .fetch_one(&test_pool().await)
        .await
        .expect("read is_active");
    assert!(is_active, "最后一名 admin 不得被停用");

    // 正：重新启用 admin_b 后，admin_a 停自己回到「禁止停用自己」的 400
    let resp = manage_user(&client, &base, &admin_a, admin_b_id, "activate", None).await;
    assert_eq!(resp.status(), 200);
    let resp = manage_user(&client, &base, &admin_a, admin_a_id, "deactivate", None).await;
    assert_eq!(resp.status(), 400, "有同僚 admin 时停自己仍是 400");
}

/// F5 审计 + 重置密码正负成对：
/// admin 重置 → 旧密码 401 / 新密码 200；停用/启用/重置各留一条审计且审计不含明文密码。
#[tokio::test]
async fn test_member_lifecycle_audit_and_reset_password() {
    let base = start_server().await;
    let client = Client::new();
    let admin = setup_user(&client, &base, "audit-admin", "admin123", "admin").await;
    let _member = setup_user(&client, &base, "audit-member", "member123", "member").await;
    let member_id = user_id_by_name("audit-member").await;
    let tenant_id = current_tenant_id().await;

    // 重置前旧密码可登录（正基线）
    let (status, _) = login_body(&client, &base, "audit-member", "member123").await;
    assert_eq!(status, 200, "重置前旧密码应可登录");

    // 正：admin 重置密码
    let new_password = "brand-new-pass-2026";
    let resp = manage_user(
        &client,
        &base,
        &admin,
        member_id,
        "reset-password",
        Some(serde_json::json!({ "password": new_password })),
    )
    .await;
    assert_eq!(resp.status(), 200, "admin 重置密码应成功");
    // 负：旧密码 401
    let (status, _) = login_body(&client, &base, "audit-member", "member123").await;
    assert_eq!(status, 401, "重置后旧密码必须失效");
    // 正：新密码可登录
    let (status, _) = login_body(&client, &base, "audit-member", new_password).await;
    assert_eq!(status, 200, "重置后新密码应可登录");

    // 另两类操作（停用/启用）也各做一次，供审计断言
    let resp = manage_user(&client, &base, &admin, member_id, "deactivate", None).await;
    assert_eq!(resp.status(), 200);
    let resp = manage_user(&client, &base, &admin, member_id, "activate", None).await;
    assert_eq!(resp.status(), 200);

    // F5：三类操作各一条审计记录
    let pool = test_pool().await;
    for action in ["user.deactivate", "user.activate", "user.reset_password"] {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_logs \
             WHERE tenant_id = $1 AND entity_type = 'user' AND entity_id = $2 AND action = $3",
        )
        .bind(tenant_id)
        .bind(member_id.to_string())
        .bind(action)
        .fetch_one(&pool)
        .await
        .expect("count audit");
        assert_eq!(count, 1, "{action} 应恰好一条审计记录");
    }

    // 负：审计 detail 不得出现重置后的明文密码
    let details: Vec<String> = sqlx::query_scalar(
        "SELECT COALESCE(detail, '') FROM audit_logs \
         WHERE tenant_id = $1 AND entity_id = $2 AND action = 'user.reset_password'",
    )
    .bind(tenant_id)
    .bind(member_id.to_string())
    .fetch_all(&pool)
    .await
    .expect("audit details");
    for detail in details {
        assert!(
            !detail.contains(new_password),
            "审计 detail 不得写入明文密码: {detail}"
        );
    }
}
