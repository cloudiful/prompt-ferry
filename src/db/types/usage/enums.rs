use serde::{Deserialize, Serialize};
use sqlx::Type;
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageEventKind {
    Request,
}

impl UsageEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestRecordCategory {
    #[default]
    Ai,
    Mcp,
}

impl RequestRecordCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestRecordState {
    Received,
    AwaitingApproval,
    UpstreamProcessing,
    Completed,
    Failed,
    Aborted,
}

impl RequestRecordState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::AwaitingApproval => "awaiting_approval",
            Self::UpstreamProcessing => "upstream_processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Aborted => "aborted",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Aborted)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestAbortReason {
    DownstreamClosed,
    BridgeBackpressureFull,
    BridgeBackpressureBytesLimit,
    WorkerLeaseExpired,
    ValkeyLeaseMissing,
    RelayUnknown,
}

impl RequestAbortReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DownstreamClosed => "downstream_closed",
            Self::BridgeBackpressureFull => "bridge_backpressure_full",
            Self::BridgeBackpressureBytesLimit => "bridge_backpressure_bytes_limit",
            Self::WorkerLeaseExpired => "worker_lease_expired",
            Self::ValkeyLeaseMissing => "valkey_lease_missing",
            Self::RelayUnknown => "relay_unknown",
        }
    }

    pub fn from_relay_reason(reason: &str) -> Self {
        match reason {
            "request_cancelled" | "downstream_closed" => Self::DownstreamClosed,
            "bridge_backpressure" | "bridge_backpressure_full" => Self::BridgeBackpressureFull,
            "bridge_backpressure_bytes_limit" => Self::BridgeBackpressureBytesLimit,
            "worker_lease_expired" => Self::WorkerLeaseExpired,
            "valkey_lease_missing" => Self::ValkeyLeaseMissing,
            _ => Self::RelayUnknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type, ToSchema)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum RequestToolCallStatus {
    Emitted,
    OutputReceived,
    Failed,
    Skipped,
}

impl RequestToolCallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Emitted => "emitted",
            Self::OutputReceived => "output_received",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}
