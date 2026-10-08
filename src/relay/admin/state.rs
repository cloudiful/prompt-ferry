//! What the relay's management listener knows about this host.
//!
//! The control plane is owned by the relay rather than by the worker, so this
//! state deliberately holds only relay facts: the bridge the worker's business
//! API is proxied over, the role this process started as, and the settings the
//! management page may change. There is no worker state here, which is what
//! lets a relay-only host answer without a worker or a worker database.

use std::{collections::HashMap, time::Instant};

use crate::{
    config::{HostRole, binds::reserve_bind, host_store},
    relay::{AppState, RelayHandle},
};

use super::dto::{
    RelayConfigView, RelaySettingsResponse, RelaySettingsUpdate, RelayStatusResponse,
    WorkerStatusResponse,
};

/// How long a management session stays valid.
pub(super) const SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(12 * 60 * 60);

#[derive(Clone)]
pub struct RelayAdminState {
    /// The relay state the worker-facing proxy and the status read from.
    pub(super) relay: AppState,
    pub(super) handle: RelayHandle,
    /// The role this process actually started as, never one being edited.
    pub(super) role: HostRole,
    /// The relay configuration this process actually started with.
    pub(super) settings: RelayConfigView,
    /// The host-local token guarding every route on this listener.
    admin_token: String,
    /// Live management sessions, keyed by their opaque session id.
    pub(super) sessions: std::sync::Arc<tokio::sync::Mutex<HashMap<String, Instant>>>,
    /// Whether a save may write. `false` makes the control plane read-only,
    /// which is how a test exercises the handlers without touching the real
    /// host configuration.
    pub(super) writable: bool,
}

/// Whether a recorded host-local change is waiting for the next start.
///
/// Both settings the page can change name something this process already owns:
/// the role decides which components run and which listeners are taken, and the
/// management bind names a listener that is bound for the life of the host. A
/// recorded value that differs from what the host started with therefore cannot
/// take effect here, and the answer stays "yes" until the next start brings them
/// back into line. An overlay that records nothing, or records the values this
/// host is already running, is not a pending change.
///
/// Taking the running values and the overlay as arguments keeps the rule itself
/// free of process-wide state, so it can be reasoned about and tested directly.
pub(super) fn restart_pending(
    running_role: HostRole,
    running_admin_bind: &str,
    overlay: &crate::config::host_store::HostOverlay,
) -> bool {
    overlay.host.role.is_some_and(|role| role != running_role)
        || overlay
            .relay
            .admin_bind
            .as_deref()
            .is_some_and(|bind| bind != running_admin_bind)
}

impl RelayAdminState {
    pub(crate) fn new(
        relay_config: &crate::config::RelayConfig,
        role: HostRole,
        relay: AppState,
        handle: RelayHandle,
    ) -> Self {
        Self {
            settings: RelayConfigView {
                bind: relay_config.bind.clone(),
                worker_bind: relay_config.worker_bind.clone(),
                admin_bind: relay_config.admin_bind.clone(),
                client_token_configured: !relay_config.client_token.trim().is_empty(),
                worker_token_configured: !relay_config.worker_token.trim().is_empty(),
                tls_enabled: relay_config.tls_mode.enabled(),
                worker_tls_enabled: relay_config.worker_tls_mode.enabled(),
                bridge_encryption_required: relay_config.bridge_encryption_mode.required(),
                request_timeout_seconds: relay_config.request_timeout_seconds,
                worker_heartbeat_timeout_seconds: relay_config.worker_heartbeat_timeout_seconds,
            },
            admin_token: relay_config.admin_token.clone(),
            relay,
            handle,
            role,
            sessions: std::sync::Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            writable: true,
        }
    }

    /// A state that answers reads but refuses every save.
    #[cfg(test)]
    pub(super) fn read_only(relay_config: crate::config::RelayConfig, token: &str) -> Self {
        let mut relay_config = relay_config;
        relay_config.admin_token = token.to_string();
        let (relay, handle) = crate::relay::test_handle(relay_config.clone());
        let mut state = Self::new(&relay_config, HostRole::Relay, relay, handle);
        state.writable = false;
        state
    }

    /// The token every route on this listener is compared against.
    pub(super) fn admin_token(&self) -> String {
        self.admin_token.clone()
    }

    /// The role this host runs as, and what the next start would run instead.
    ///
    /// After a save the reported role is the recorded one, because that is what
    /// the page is about to be running; the running process is still the role it
    /// started as, which is why the two are reported separately.
    pub(super) fn reported_role(&self) -> (HostRole, Option<HostRole>) {
        let overlay = host_store::active_overlay();
        (
            overlay.host.role.unwrap_or(self.role),
            overlay.host.role.filter(|pending| *pending != self.role),
        )
    }

    /// Whether a recorded host-local change is waiting for the next start.
    pub(super) fn restart_pending(&self) -> bool {
        restart_pending(
            self.role,
            &self.settings.admin_bind,
            &host_store::active_overlay(),
        )
    }

    pub(super) async fn worker_status(&self) -> WorkerStatusResponse {
        let count = self.handle.worker_count().await;
        WorkerStatusResponse {
            connected: count > 0,
            connected_workers: count,
            config_version: self.handle.config_version().await,
        }
    }

    pub(super) async fn status(&self) -> RelayStatusResponse {
        let (role, _) = self.reported_role();
        RelayStatusResponse {
            role,
            restart_required: self.restart_pending(),
            relay: self.settings.clone(),
            worker: self.worker_status().await,
            relay_ready: self.handle.is_ready().await,
        }
    }

    pub(super) fn settings_response(&self) -> RelaySettingsResponse {
        RelaySettingsResponse {
            restart_required: self.restart_pending(),
            relay: self.settings.clone(),
        }
    }

    /// Persist a settings change, or refuse when this state is read-only.
    ///
    /// The answer is whether the saved change needs a restart. Every field here
    /// names a listener, and a listener that is already bound cannot move inside
    /// the running process.
    pub(super) fn save_settings(&self, update: &RelaySettingsUpdate) -> Result<bool, &'static str> {
        if !self.writable {
            return Err("the relay management API cannot write host configuration here");
        }
        let Some(admin_bind) = update.admin_bind.clone() else {
            return Ok(false);
        };
        reserve_bind(&admin_bind, "relay.admin_bind", true)
            .map_err(|_| "the relay management bind must be a loopback address")?;
        host_store::update(|overlay| overlay.relay.admin_bind = Some(admin_bind))
            .map_err(|_| "failed to write the host-local configuration")?;
        Ok(true)
    }

    /// Persist the role this host should start as next time.
    pub(super) fn save_role(&self, role: HostRole) -> Result<(), &'static str> {
        if !self.writable {
            return Err("the relay management API cannot write host configuration here");
        }
        host_store::save_role(role).map_err(|_| "failed to write the host-local configuration")
    }
}

#[cfg(test)]
mod tests {
    use super::restart_pending;
    use crate::config::{
        HostRole,
        host_store::{HostOverlay, HostOverlayRelay, HostOverlayRole},
    };

    fn overlay(role: Option<HostRole>, admin_bind: Option<&str>) -> HostOverlay {
        HostOverlay {
            host: HostOverlayRole { role },
            relay: HostOverlayRelay {
                admin_bind: admin_bind.map(str::to_string),
                admin_token: None,
            },
        }
    }

    /// An overlay that records nothing is not a pending change: this is every
    /// installation that has never saved a role or a bind.
    #[test]
    fn an_empty_overlay_never_asks_for_a_restart() {
        assert!(!restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(None, None)
        ));
    }

    /// A recorded bind that differs from the running one is a pending change,
    /// even though the role has not moved.
    #[test]
    fn a_saved_management_bind_alone_asks_for_a_restart() {
        assert!(restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(None, Some("127.0.0.1:8791"))
        ));
    }

    /// Once the host is running the bind that was saved, there is nothing left to
    /// apply, and reporting a restart would leave the page asking for one forever.
    #[test]
    fn a_saved_management_bind_that_matches_the_running_one_is_settled() {
        assert!(!restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(None, Some("127.0.0.1:8790"))
        ));
    }

    /// An automatic bind stays settled: the port it resolves to is not what was
    /// recorded, so comparing resolved addresses would never settle.
    #[test]
    fn an_automatic_bind_is_settled_against_the_value_it_was_configured_with() {
        assert!(!restart_pending(
            HostRole::Relay,
            "127.0.0.1:0",
            &overlay(None, Some("127.0.0.1:0"))
        ));
    }

    /// A saved role is a pending change on its own, and either reason is reported
    /// as the same single answer.
    #[test]
    fn a_saved_role_alone_asks_for_a_restart() {
        assert!(restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(Some(HostRole::Integrated), None)
        ));
        assert!(!restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(Some(HostRole::Relay), None)
        ));
        assert!(restart_pending(
            HostRole::Relay,
            "127.0.0.1:8790",
            &overlay(Some(HostRole::Relay), Some("127.0.0.1:8791"))
        ));
    }
}
