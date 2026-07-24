use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::db;

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ApprovalPageQuery {
    pub status: Option<db::ApprovalStatusFilter>,
    pub first: Option<i64>,
    pub rows: Option<i64>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ApprovalPageResponse {
    pub total: i64,
    pub approvals: Vec<db::ApprovalRequest>,
    pub first: i64,
    pub rows: i64,
}

impl From<db::ApprovalRequestPage> for ApprovalPageResponse {
    fn from(value: db::ApprovalRequestPage) -> Self {
        Self {
            total: value.total,
            approvals: value.approvals,
            first: value.first,
            rows: value.rows,
        }
    }
}
