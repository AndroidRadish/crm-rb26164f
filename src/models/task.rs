use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "task_status", rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "reminder_channel", rename_all = "snake_case")]
pub enum ReminderChannel {
    System,
    Wecom,
    Dingtalk,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub customer_id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub due_at: DateTime<Utc>,
    pub status: TaskStatus,
    pub reminder_channel: ReminderChannel,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    pub title: String,
    pub due_at: DateTime<Utc>,
    pub reminder_channel: Option<ReminderChannel>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTaskRequest {
    pub title: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub status: Option<TaskStatus>,
    pub reminder_channel: Option<ReminderChannel>,
}
