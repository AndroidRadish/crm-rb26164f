//! Excel 导入 handler（模板下载 + 上传导入 + 批次管理 + CSV 导出）

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::models::import_batch::ImportBatch;
use crate::services::{audit, auth};

fn unauthorized() -> impl IntoResponse {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": "Missing or invalid Authorization header" })),
    )
}

/// GET /api/import/template — 下载导入模板（CSV 格式）
pub async fn template() -> impl IntoResponse {
    let csv_content = "公司名,联系人,电话,邮箱,地址,备注,阶段\n示例公司,张三,13800138000,zhangsan@example.com,北京市朝阳区,示例备注,线索\n";
    (
        StatusCode::OK,
        [
            ("Content-Type", "text/csv; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"customer_template.csv\"",
            ),
        ],
        csv_content,
    )
        .into_response()
}

/// POST /api/import — 上传 CSV 导入客户
pub async fn upload(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    let content = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "Invalid UTF-8 content" })),
            )
                .into_response();
        }
    };

    // 创建批次记录
    let batch = match sqlx::query_as::<_, ImportBatch>(
        "INSERT INTO import_batches (tenant_id, filename, user_id, total_rows, success_rows, failed_rows, status) \
         VALUES ($1, $2, $3, 0, 0, 0, 'processing') \
         RETURNING id, tenant_id, filename, user_id, total_rows, success_rows, failed_rows, status, created_at",
    )
    .bind(user.tenant_id)
    .bind("upload.csv")
    .bind(user.user_id)
    .fetch_one(&pool)
    .await
    {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // 解析 CSV
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(content.as_bytes());

    let mut success_rows = 0i32;
    let mut failed_rows = 0i32;
    let mut total_rows = 0i32;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for (idx, record) in rdr.records().enumerate() {
        total_rows += 1;
        let row_num = idx + 2; // +1 for header, +1 for 1-based
        let record = match record {
            Ok(r) => r,
            Err(e) => {
                failed_rows += 1;
                errors.push(serde_json::json!({
                    "row": row_num,
                    "error": format!("CSV parse error: {}", e),
                }));
                continue;
            }
        };

        let company = record.get(0).unwrap_or("").trim().to_string();
        let contact_name = record
            .get(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let phone = record
            .get(2)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let email = record
            .get(3)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let address = record
            .get(4)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let notes = record
            .get(5)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let stage_name = record
            .get(6)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if company.is_empty() {
            failed_rows += 1;
            errors.push(serde_json::json!({
                "row": row_num,
                "error": "公司名不能为空",
            }));
            continue;
        }

        // 查找阶段
        let stage_id: Option<Uuid> = if let Some(ref sn) = stage_name {
            match sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM stages WHERE tenant_id = $1 AND name = $2 AND is_active = true LIMIT 1",
            )
            .bind(user.tenant_id)
            .bind(sn)
            .fetch_optional(&pool)
            .await
            {
                Ok(Some(id)) => Some(id),
                _ => None,
            }
        } else {
            None
        };

        // 插入客户
        let insert_result = sqlx::query(
            "INSERT INTO customers (tenant_id, company, contact_name, phone, email, address, notes, owner_id, stage, source) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'excel')",
        )
        .bind(user.tenant_id)
        .bind(&company)
        .bind(&contact_name)
        .bind(&phone)
        .bind(&email)
        .bind(&address)
        .bind(&notes)
        .bind(user.user_id)
        .bind(stage_id)
        .execute(&pool)
        .await;

        match insert_result {
            Ok(_) => success_rows += 1,
            Err(e) => {
                failed_rows += 1;
                errors.push(serde_json::json!({
                    "row": row_num,
                    "error": format!("插入失败: {}", e),
                }));
            }
        }
    }

    // 更新批次状态
    let status = if success_rows > 0 { "done" } else { "failed" };
    let _ = sqlx::query(
        "UPDATE import_batches SET total_rows = $1, success_rows = $2, failed_rows = $3, status = $4 WHERE id = $5",
    )
    .bind(total_rows)
    .bind(success_rows)
    .bind(failed_rows)
    .bind(status)
    .bind(batch.id)
    .execute(&pool)
    .await;

    audit::log_from_headers(
        &pool,
        &headers,
        "import.upload",
        "import_batch",
        &batch.id.to_string(),
        &format!(
            "total={} success={} failed={}",
            total_rows, success_rows, failed_rows
        ),
    )
    .await;

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "batch_id": batch.id,
            "total_rows": total_rows,
            "success_rows": success_rows,
            "failed_rows": failed_rows,
            "status": status,
            "errors": errors,
        })),
    )
        .into_response()
}

/// GET /api/import/batches — 列出导入批次
pub async fn list_batches(State(pool): State<PgPool>, headers: HeaderMap) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    match sqlx::query_as::<_, ImportBatch>(
        "SELECT id, tenant_id, filename, user_id, total_rows, success_rows, failed_rows, status, created_at \
         FROM import_batches WHERE tenant_id = $1 ORDER BY created_at DESC LIMIT 50",
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

/// GET /api/import/batches/:id — 获取批次详情
pub async fn get_batch(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };

    match sqlx::query_as::<_, ImportBatch>(
        "SELECT id, tenant_id, filename, user_id, total_rows, success_rows, failed_rows, status, created_at \
         FROM import_batches WHERE id = $1 AND tenant_id = $2",
    )
    .bind(id)
    .bind(user.tenant_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(batch)) => (StatusCode::OK, Json(batch)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Batch not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /api/export/customers.csv — 导出客户数据（管理员）
pub async fn export_customers_csv(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    if !auth::is_admin(&user) {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({ "error": "Only admin can export data" })),
        )
            .into_response();
    }

    let scope = auth::scope_where(&user, "c");
    let rows = match sqlx::query(&format!(
        "SELECT c.company, c.contact_name, c.phone, c.email, c.address, c.notes, \
         s.name as stage_name, u.username as owner_name, c.created_at \
         FROM customers c \
         LEFT JOIN stages s ON c.stage = s.id \
         LEFT JOIN users u ON c.owner_id = u.id \
         WHERE {} ORDER BY c.created_at DESC",
        scope
    ))
    .fetch_all(&pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
                .into_response();
        }
    };

    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "公司名",
        "联系人",
        "电话",
        "邮箱",
        "地址",
        "备注",
        "阶段",
        "负责人",
        "创建时间",
    ])
    .ok();

    for row in rows {
        let company: String = row.try_get("company").unwrap_or_default();
        let contact_name: Option<String> = row.try_get("contact_name").unwrap_or(None);
        let phone: Option<String> = row.try_get("phone").unwrap_or(None);
        let email: Option<String> = row.try_get("email").unwrap_or(None);
        let address: Option<String> = row.try_get("address").unwrap_or(None);
        let notes: Option<String> = row.try_get("notes").unwrap_or(None);
        let stage_name: Option<String> = row.try_get("stage_name").unwrap_or(None);
        let owner_name: Option<String> = row.try_get("owner_name").unwrap_or(None);
        let created_at: chrono::DateTime<chrono::Utc> =
            row.try_get("created_at").unwrap_or_default();

        wtr.write_record([
            company,
            contact_name.unwrap_or_default(),
            phone.unwrap_or_default(),
            email.unwrap_or_default(),
            address.unwrap_or_default(),
            notes.unwrap_or_default(),
            stage_name.unwrap_or_default(),
            owner_name.unwrap_or_default(),
            created_at.to_rfc3339(),
        ])
        .ok();
    }

    let csv_data = wtr.into_inner().unwrap_or_default();

    audit::log_from_headers(
        &pool,
        &headers,
        "export.customers",
        "customers",
        "",
        "CSV export",
    )
    .await;

    (
        StatusCode::OK,
        [
            ("Content-Type", "text/csv; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"customers_export.csv\"",
            ),
        ],
        csv_data,
    )
        .into_response()
}
