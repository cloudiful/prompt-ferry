//! The relay control plane's request and response bodies.
//!
//! These are the shapes the management page reads. Everything here is a relay
//! fact — this host's own listeners, its bridge, its role, and whether a worker
//! is attached. The worker's business resources (providers, routes, quotas,
//! usage) are not modelled: those stay behind the worker bridge and keep their
//! existing admin API.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::config::HostRole;

/// Which services this host runs, and whether it must restart to change them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelayHostResponse {
    /// The role this process started as.
    pub role: HostRole,
    /// The role recorded for the next start, when one differs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_role: Option<HostRole>,
    /// A saved change only takes effect on the next start.
    pub restart_required: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelayHostRoleRequest {
    pub role: HostRole,
}

/// The relay's own state, without touching a worker.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelayStatusResponse {
    pub role: HostRole,
    /// A saved role change is waiting for a restart.
    pub restart_required: bool,
    pub relay: RelayConfigView,
    pub worker: WorkerStatusResponse,
    /// Whether the relay can already route public business traffic.
    pub relay_ready: bool,
}

/// This host's relay listeners and bridge security.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelayConfigView {
    pub bind: String,
    pub worker_bind: String,
    pub admin_bind: String,
    /// Whether a client token is configured; the token itself is never returned.
    pub client_token_configured: bool,
    pub worker_token_configured: bool,
    pub tls_enabled: bool,
    pub worker_tls_enabled: bool,
    pub bridge_encryption_required: bool,
    pub request_timeout_seconds: u64,
    pub worker_heartbeat_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelaySettingsResponse {
    pub restart_required: bool,
    pub relay: RelayConfigView,
}

/// What the relay knows about the worker attached to it.
///
/// A relay-only host reports `connected: false` here rather than failing: the
/// management page stays usable and only the worker-owned views report that
/// there is nothing behind them.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct WorkerStatusResponse {
    pub connected: bool,
    pub connected_workers: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_version: Option<i64>,
}

/// Whether the caller currently holds a management session.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RelayAuthResponse {
    pub authenticated: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelayLoginRequest {
    pub admin_token: String,
}

/// The relay settings the management page may change.
///
/// Everything here is relay-local: it names this machine's own management
/// listener and never which remote relays a worker may dial. Each field needs a
/// restart to take effect, because the listener it names is already bound.
#[derive(Debug, Default, serde::Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RelaySettingsUpdate {
    /// The loopback bind of the relay's management listener.
    pub admin_bind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{RelayHostRoleRequest, RelayStatusResponse};
    use crate::config::HostRole;
    use crate::relay::admin::dto::{RelayConfigView, WorkerStatusResponse};

    #[test]
    fn an_unknown_role_is_rejected_instead_of_defaulting_to_integrated() {
        let error = serde_json::from_str::<RelayHostRoleRequest>(r#"{"role":"relay-only"}"#)
            .expect_err("a role the product does not have must not be accepted");
        assert!(error.to_string().contains("relay-only"), "{error}");
    }

    #[test]
    fn the_status_body_never_carries_a_token() {
        let body = serde_json::to_string(&RelayStatusResponse {
            role: HostRole::Relay,
            restart_required: false,
            relay: RelayConfigView {
                bind: "127.0.0.1:8787".to_string(),
                worker_bind: "127.0.0.1:8788".to_string(),
                admin_bind: "127.0.0.1:8790".to_string(),
                client_token_configured: true,
                worker_token_configured: true,
                tls_enabled: false,
                worker_tls_enabled: false,
                bridge_encryption_required: false,
                request_timeout_seconds: 300,
                worker_heartbeat_timeout_seconds: 90,
            },
            worker: WorkerStatusResponse {
                connected: false,
                connected_workers: 0,
                config_version: None,
            },
            relay_ready: false,
        })
        .expect("serialize the status");

        assert!(!body.contains("token\":\""), "got: {body}");
        assert!(
            body.contains("\"client_token_configured\":true"),
            "got: {body}"
        );
        assert!(body.contains("\"connected\":false"), "got: {body}");
        assert!(!body.contains("config_version"), "got: {body}");
    }
}
