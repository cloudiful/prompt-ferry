use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BridgeStatus {
    pub configured_relays: usize,
    pub connected_relays: usize,
    pub snapshot_version: i64,
    pub relays: Vec<RelayBridgeStatus>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelayBridgeStatus {
    pub relay_id: Option<uuid::Uuid>,
    pub relay_url: String,
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub last_snapshot_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SessionUser {
    pub user_id: i64,
    pub login_name: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub login_name: String,
    pub password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub user_id: i64,
    pub login_name: String,
    pub display_name: String,
    pub is_admin: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub login_name: String,
    pub password: String,
    pub display_name: String,
    pub is_admin: Option<bool>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ResetPasswordRequest {
    pub password: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateClientKeyRequest {
    pub label: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateClientKeyResponse {
    pub key_id: i64,
    pub user_id: i64,
    pub key_prefix: String,
    pub label: String,
    pub enabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub secret: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateClientKeyRequest {
    pub label: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AvailableModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AvailableModelsResponse {
    pub models: Vec<AvailableModel>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserPageResponse {
    pub users: Vec<db::User>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ClientKeyPageResponse {
    pub keys: Vec<db::ClientKey>,
    pub total: i64,
    pub first: i64,
    pub rows: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UserOptionsResponse {
    pub users: Vec<db::User>,
}
