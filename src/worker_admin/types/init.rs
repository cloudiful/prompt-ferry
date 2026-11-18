use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Serialize, ToSchema)]
pub struct SetupStatus {
    pub initialized: bool,
    pub setup_available: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct SetupRequest {
    #[schema(write_only)]
    pub master_secret: String,
    pub login_name: String,
    #[schema(write_only)]
    pub password: String,
    pub display_name: Option<String>,
}
