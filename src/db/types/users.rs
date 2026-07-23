use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct User {
    pub user_id: i64,
    pub login_name: String,
    pub display_name: String,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct UserPassword {
    pub user_id: i64,
    pub login_name: String,
    pub password_hash: String,
    pub display_name: String,
    pub is_admin: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, FromRow, ToSchema)]
pub struct ClientKey {
    pub key_id: i64,
    pub user_id: i64,
    pub key_prefix: String,
    pub label: String,
    pub enabled: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub secret: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ClientKeyIdentity {
    pub key_id: i64,
    pub label: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UserCreate {
    pub login_name: String,
    pub password_hash: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UserUpdate {
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}
