use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "customer_source", rename_all = "snake_case")]
pub enum CustomerSource {
    Manual,
    Excel,
    Claim,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Customer {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub company: String,
    pub contact_name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub owner_id: Option<Uuid>,
    pub stage: Option<Uuid>,
    pub source: CustomerSource,
    pub duplicate_of: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateCustomerRequest {
    pub company: String,
    pub contact_name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub stage: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCustomerRequest {
    pub company: Option<String>,
    pub contact_name: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Option<String>,
    pub notes: Option<String>,
    pub stage: Option<Uuid>,
}

/// 防撞单检测结果
#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateCheckResult {
    pub is_duplicate: bool,
    pub duplicates: Vec<Customer>,
}
