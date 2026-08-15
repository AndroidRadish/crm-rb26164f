use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Stage {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub order_index: i32,
    pub is_won: bool,
    pub is_lost: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateStageRequest {
    pub name: String,
    pub order_index: Option<i32>,
    pub is_won: Option<bool>,
    pub is_lost: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateStageRequest {
    pub name: Option<String>,
    pub order_index: Option<i32>,
    pub is_won: Option<bool>,
    pub is_lost: Option<bool>,
    pub is_active: Option<bool>,
}
