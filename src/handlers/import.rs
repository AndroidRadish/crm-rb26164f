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
use crate::services::{audit, auth, feature};

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

/// POST /api/import — 上传 CSV/xlsx 导入客户
///
/// 支持两种提交方式：
/// 1. multipart/form-data（Apifox/Postman/HTML 文件表单）——取文件部分再解析
/// 2. 原始 body（text/csv 或直接贴 CSV 文本 / xlsx 二进制）
pub async fn upload(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let user = match auth::extract_user(&headers) {
        Ok(u) => u,
        Err(_) => return unauthorized().into_response(),
    };
    // 功能分级：导入/导出属专业档（import.export）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "import.export").await {
        return resp;
    }

    // 缺陷修复：multipart/form-data 上传时不能直接拿整个 body 当 CSV 解析，
    // 否则 boundary/Content-Disposition 等信封行会被当成客户数据入库。
    let content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let body: axum::body::Bytes = if content_type.starts_with("multipart/form-data") {
        match extract_multipart_file(&body, &content_type) {
            Ok(bytes) => bytes.into(),
            Err(msg) => {
                // 错误路径：记录日志并返回 400，禁止带病入库
                tracing::warn!("import upload multipart 解析失败: {}", msg);
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
        }
    } else {
        body
    };

    let content = match std::str::from_utf8(&body) {
        Ok(s) => Some(s),
        Err(_) => None, // xlsx 是二进制，非 UTF-8 属正常（走 calamine 分支）
    };
    let is_xlsx = body.starts_with(b"PK\x03\x04");

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

    // 解析输入：xlsx（PK zip 头）走 calamine，否则按 CSV 文本
    let rows: Vec<Vec<String>> = if is_xlsx {
        match parse_xlsx(&body) {
            Ok(r) => r,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("xlsx 解析失败: {e}") })),
                )
                    .into_response()
            }
        }
    } else {
        match content {
            Some(c) => parse_csv_rows(c),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "无法识别的文件格式（支持 .xlsx 和 UTF-8 CSV）" })),
                )
                    .into_response()
            }
        }
    };

    // 拒绝没有任何数据行的合法解析结果（空文件/只有表头视为可入库 0 行，
    // 但完全解析不出内容的内容不允许静默成功）
    if rows.is_empty() {
        let msg = if is_xlsx {
            "xlsx 文件为空或第一个工作表没有数据行".to_string()
        } else {
            "CSV 内容为空或格式不合法（未解析到任何数据行）".to_string()
        };
        tracing::warn!("import upload 内容校验失败: {}", msg);
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }

    let mut success_rows = 0i32;
    let mut failed_rows = 0i32;
    let mut total_rows = 0i32;
    let mut errors: Vec<serde_json::Value> = Vec::new();

    for (idx, record) in rows.iter().enumerate() {
        total_rows += 1;
        let row_num = idx + 2; // +1 for header, +1 for 1-based
        let cell = |i: usize| record.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let company = cell(0);
        let contact_name = Some(cell(1)).filter(|s| !s.is_empty());
        let phone = Some(cell(2)).filter(|s| !s.is_empty());
        let email = Some(cell(3)).filter(|s| !s.is_empty());
        let address = Some(cell(4)).filter(|s| !s.is_empty());
        let notes = Some(cell(5)).filter(|s| !s.is_empty());
        let stage_name = Some(cell(6)).filter(|s| !s.is_empty());

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
    // 功能分级：导入/导出属专业档（import.export）
    if let Err(resp) = feature::require_feature(&pool, user.tenant_id, "import.export").await {
        return resp;
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

// ── multipart 信封解析（不引第三方依赖，手动按 boundary 拆 part） ──

/// 从 Content-Type 头里取 boundary 参数，如 `multipart/form-data; boundary=----abc`
fn parse_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|part| {
        let part = part.trim();
        part.strip_prefix("boundary=")
            .map(|b| b.trim_matches('"').to_string())
    })
}

/// 截取 part 头部块中 `Content-Disposition` 的某个参数值（name / filename）
fn disposition_param(header_block: &str, key: &str) -> Option<String> {
    let line = header_block
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-disposition"))?;
    line.split(';').skip(1).find_map(|kv| {
        let kv = kv.trim();
        kv.strip_prefix(&format!("{}=", key))
            .map(|v| v.trim_matches('"').to_string())
    })
}

/// 手动拆 multipart body：找到含 filename 的 part（即文件部分）并返回其字节。
/// 没有文件 part / 找不到 boundary 视为客户端请求非法，返回 Err 交给上层转 400。
fn extract_multipart_file(body: &[u8], content_type: &str) -> Result<Vec<u8>, String> {
    let boundary = parse_boundary(content_type)
        .ok_or_else(|| "multipart 请求缺少 boundary 参数".to_string())?;
    let delimiter = format!("--{}", boundary);

    let body_str = String::from_utf8_lossy(body);
    for part in body_str.split(&delimiter).skip(1) {
        // 结束标记 --boundary--
        if part.trim_start().starts_with("--") {
            break;
        }
        // part 格式：\r\n头部\r\n\r\n内容\r\n
        let part = part.strip_prefix("\r\n").unwrap_or(part);
        let Some(split_at) = part.find("\r\n\r\n") else {
            // 找不到头/内容分隔的 part 视为损坏，跳过
            continue;
        };
        let header_block = &part[..split_at];
        let mut content = &part[split_at + 4..];
        // 去掉末尾 part 结束换行
        if let Some(stripped) = content.strip_suffix("\r\n") {
            content = stripped;
        }
        if disposition_param(header_block, "filename").is_some() {
            return Ok(content.as_bytes().to_vec());
        }
    }
    Err("multipart 请求中未找到文件部分（需要带 filename 的文件字段）".to_string())
}

// ── 文件解析（CSV / xlsx 统一为行数组） ──────────────────────

fn parse_csv_rows(content: &str) -> Vec<Vec<String>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(content.as_bytes());
    rdr.records()
        .filter_map(|r| r.ok())
        .map(|r| (0..r.len()).map(|i| r.get(i).unwrap_or("").to_string()).collect())
        .collect()
}

fn parse_xlsx(bytes: &[u8]) -> Result<Vec<Vec<String>>, String> {
    use calamine::{open_workbook_auto_from_rs, Reader as _};
    let mut wb = open_workbook_auto_from_rs(std::io::Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
    let range = wb
        .worksheet_range_at(0)
        .ok_or_else(|| "xlsx 中没有工作表".to_string())?
        .map_err(|e| e.to_string())?;
    let mut out: Vec<Vec<String>> = range
        .rows()
        .map(|row| row.iter().map(|c| c.to_string()).collect())
        .collect();
    if !out.is_empty() {
        out.remove(0); // 表头
    }
    Ok(out)
}
