use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Which services this host runs when prompt-ferry starts without a subcommand.
///
/// The role is host-local: it picks the listeners and the running components of
/// this machine, never which remote relays a worker may dial. A subcommand
/// names its own component (`relay`, `worker`, `serve`) and does not read it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HostRole {
    /// Local relay plus an embedded worker. The default, and the behavior of
    /// every installation that carries no role configuration.
    #[default]
    Integrated,
    /// Worker only: no local relay, and the worker dials the enabled remote
    /// relay list. An empty list is not an error — the worker stays up and
    /// waits for a connection to appear.
    Worker,
    /// Relay only: no local worker. Remote workers connect to the relay's
    /// worker bind.
    Relay,
}

impl HostRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Integrated => "integrated",
            Self::Worker => "worker",
            Self::Relay => "relay",
        }
    }

    /// Whether this host serves a relay, either publicly or as the bridge an
    /// embedded worker connects to.
    pub fn runs_relay(self) -> bool {
        self != Self::Worker
    }

    /// Whether this host runs a worker, embedded or on its own.
    pub fn runs_worker(self) -> bool {
        self != Self::Relay
    }

    /// Whether the worker dials a relay bridge inside this same process.
    ///
    /// A host that runs no local relay has no such bridge, so the static
    /// `worker.relay_urls` list — which names that bridge — is not one of its
    /// connection targets.
    pub fn uses_local_bridge(self) -> bool {
        self == Self::Integrated
    }
}

/// Host-local settings, resolved once per startup.
///
/// The role selects listeners and running components, so a change to it takes
/// effect on the next restart rather than while the host is serving.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HostConfig {
    pub role: HostRole,
}

#[cfg(test)]
mod tests {
    use super::{HostConfig, HostRole};
    use crate::config::AppConfig;

    #[test]
    fn a_host_without_role_configuration_runs_the_integrated_role() {
        assert_eq!(HostConfig::default().role, HostRole::Integrated);
        assert_eq!(
            serde_json::from_str::<HostConfig>("{}").expect("empty host config"),
            HostConfig::default(),
            "an absent role section must resolve to the default"
        );
    }

    #[test]
    fn a_configuration_written_before_the_role_existed_still_deserializes() {
        // Every existing installation has a config file without a `host`
        // section; it must keep starting in the integrated mode.
        let config: AppConfig =
            serde_json::from_str(r#"{"relay":{"bind":"127.0.0.1:9000"}}"#).expect("legacy config");

        assert_eq!(config.host.role, HostRole::Integrated);
        assert_eq!(config.relay.bind, "127.0.0.1:9000");
    }

    #[test]
    fn every_role_round_trips_through_its_lowercase_name() {
        for (role, name) in [
            (HostRole::Integrated, "integrated"),
            (HostRole::Worker, "worker"),
            (HostRole::Relay, "relay"),
        ] {
            let json = serde_json::to_string(&role).expect("serialize role");
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(role.as_str(), name);
            assert_eq!(
                serde_json::from_str::<HostRole>(&json).expect("parse role"),
                role
            );
        }
    }

    #[test]
    fn an_unknown_role_is_rejected_instead_of_silently_defaulting() {
        let error = serde_json::from_str::<HostConfig>(r#"{"role":"worker-only"}"#)
            .expect_err("an unrecognized role must not start a host in the wrong mode");

        assert!(
            error.to_string().contains("worker-only"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn each_role_selects_its_own_components() {
        assert!(HostRole::Integrated.runs_relay() && HostRole::Integrated.runs_worker());
        assert!(HostRole::Integrated.uses_local_bridge());
        assert!(HostRole::Worker.runs_worker() && !HostRole::Worker.runs_relay());
        assert!(!HostRole::Worker.uses_local_bridge());
        assert!(HostRole::Relay.runs_relay() && !HostRole::Relay.runs_worker());
        assert!(!HostRole::Relay.uses_local_bridge());
    }
}
