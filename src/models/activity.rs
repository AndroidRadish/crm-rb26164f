use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "activity_type", rename_all = "snake_case")]
pub enum ActivityType {
    Call,
    Email,
    Meeting,
    Note,
    Other,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Activity {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub customer_id: Uuid,
    pub user_id: Uuid,
    pub r#type: ActivityType,
    pub content: String,
    pub occurred_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateActivityRequest {
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    pub r#type: ActivityType,
    pub content: String,
    pub occurred_at: Option<DateTime<Utc>>,
}
