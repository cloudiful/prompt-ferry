use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Serialize, ToSchema)]
pub struct NodeStatusResponse {
    pub node_id: Uuid,
    pub group_id: Uuid,
    pub has_database: bool,
    pub runtime_version: String,
    pub protocol_version: String,
    pub peers: Vec<NodePeerSummary>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct NodePeerSummary {
    pub node_id: Uuid,
    pub has_database: bool,
    pub runtime_version: String,
    pub protocol_version: String,
    pub connected_at: Option<chrono::DateTime<chrono::Utc>>,
}
