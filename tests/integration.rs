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
    sqlx::query("DELETE FROM stages").execute(pool).await.ok();
    sqlx::query("DELETE FROM users").execute(pool).await.ok();
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
