use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "import_status", rename_all = "snake_case")]
pub enum ImportStatus {
    Pending,
    Processing,
    Done,
    Failed,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ImportBatch {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub filename: String,
    pub user_id: Uuid,
    pub total_rows: i32,
    pub success_rows: i32,
    pub failed_rows: i32,
    pub status: ImportStatus,
    pub created_at: DateTime<Utc>,
}
