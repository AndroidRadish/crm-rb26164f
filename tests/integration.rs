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

    // 模板下载
    let resp = client
        .get(&format!("{}/api/import/template", base))
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
