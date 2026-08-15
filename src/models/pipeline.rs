use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PipelineEntry {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub customer_id: Uuid,
    pub stage_id: Uuid,
    pub entered_at: DateTime<Utc>,
    pub exited_at: Option<DateTime<Utc>>,
    pub amount_estimate: Option<rust_decimal::Decimal>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreatePipelineEntryRequest {
    pub customer_id: Uuid,
    pub stage_id: Uuid,
    pub amount_estimate: Option<rust_decimal::Decimal>,
}
