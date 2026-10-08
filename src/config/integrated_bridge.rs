//! The relay/worker pair of the integrated role.
//!
//! The integrated role bridges its own relay to its own worker, so both halves
//! are configured from the listeners this process already reserved: the worker
//! is pointed at the bridge that will really accept it, and it receives the
//! admin address that is already bound rather than the configured string. The
//! payload encryption and transport security of a bridge that never leaves the
//! process cannot apply, and are switched off here instead of being silently
//! mismatched between the two halves.

use crate::config::{
    BridgeEncryptionMode, RelayConfig, TlsMode, WorkerConfig, WorkerTlsMode,
    binds::{ReservedBind, worker_bridge_url},
};

/// Derive the relay and worker configuration for the integrated role.
///
/// `bridge` and `admin` must be the reserved listeners the relay and the worker
/// will serve on, so the addresses derived here are the ones that will really
/// exist.
pub fn bridge_connected_configs(
    app_config: crate::config::AppConfig,
    bridge: &ReservedBind,
    admin: &ReservedBind,
) -> (RelayConfig, WorkerConfig) {
    let mut relay_config = app_config.relay;
    let mut worker_config = app_config.worker;

    relay_config.worker_bind = bridge.addr().to_string();
    worker_config.relay_urls = vec![worker_bridge_url(bridge)];
    // The admin socket is already reserved before the worker starts, so the
    // worker receives the resolved address instead of the configured string. An
    // empty value or a port of `0` would otherwise be re-parsed and rebound by
    // the admin server, landing on a different port than the one reserved.
    worker_config.admin_bind = admin.addr().to_string();

    // The worker-side token is authoritative in this role: an explicitly empty
    // worker token fully opens integrated worker auth even when the relay
    // config carries its default or a custom token.
    let worker_token = worker_config.worker_token.trim().to_string();
    relay_config.worker_token = worker_token.clone();
    worker_config.worker_token = worker_token;

    relay_config.worker_tls_mode = TlsMode::Off;
    relay_config.worker_tls_cert.clear();
    relay_config.worker_tls_key.clear();
    relay_config.worker_tls_client_ca.clear();

    worker_config.tls_mode = WorkerTlsMode::Off;
    worker_config.relay_ca.clear();
    worker_config.client_cert.clear();
    worker_config.client_key.clear();

    relay_config.bridge_encryption_mode = BridgeEncryptionMode::Off;
    relay_config.bridge_encryption_key.clear();
    worker_config.bridge_encryption_mode = BridgeEncryptionMode::Off;
    worker_config.bridge_encryption_key.clear();

    (relay_config, worker_config)
}

#[cfg(test)]
mod tests {
    use super::bridge_connected_configs;
    use crate::config::{
        AppConfig, BridgeEncryptionMode, TlsMode, WorkerTlsMode, binds::reserve_bind,
    };

    /// Two reserved loopback listeners; the addresses are free ports.
    fn reserved_pair() -> (
        crate::config::binds::ReservedBind,
        crate::config::binds::ReservedBind,
    ) {
        let bridge = reserve_bind("127.0.0.1:0", "serve.internal_worker_bind", true)
            .expect("reserve a bridge");
        let admin =
            reserve_bind("127.0.0.1:0", "worker.admin_bind", false).expect("reserve an admin bind");
        (bridge, admin)
    }

    #[test]
    fn the_worker_is_pointed_at_the_reserved_bridge() {
        let (bridge, admin) = reserved_pair();
        let expected_bridge = bridge.addr();

        let (relay_config, worker_config) =
            bridge_connected_configs(AppConfig::default(), &bridge, &admin);

        assert_eq!(relay_config.worker_bind, expected_bridge.to_string());
        assert_eq!(
            worker_config.relay_urls,
            vec![format!("ws://{expected_bridge}/ws/worker")],
            "the derived URL must name the bridge listener that will really accept it"
        );
    }

    #[test]
    fn the_worker_receives_the_resolved_admin_address_not_the_configured_string() {
        for configured in ["", "   ", "0.0.0.0:0", "127.0.0.1:0", "127.0.0.1:8789"] {
            let mut app_config = AppConfig::default();
            app_config.worker.admin_bind = configured.to_string();
            let (bridge, admin) = reserved_pair();
            let expected_admin = admin.addr();

            let (_, worker_config) = bridge_connected_configs(app_config, &bridge, &admin);

            assert_eq!(
                worker_config.admin_bind,
                expected_admin.to_string(),
                "configured admin bind `{configured}` must propagate as the resolved address"
            );
            assert_ne!(
                worker_config
                    .admin_bind
                    .parse::<std::net::SocketAddr>()
                    .expect("resolved")
                    .port(),
                0,
                "the worker must never be told to bind port 0 again"
            );
        }
    }

    #[test]
    fn the_in_process_bridge_drops_the_security_settings_it_cannot_use() {
        let mut app_config = AppConfig::default();
        app_config.relay.worker_tls_mode = TlsMode::Mtls;
        app_config.relay.worker_tls_cert = "/tmp/worker.crt".to_string();
        app_config.relay.worker_tls_key = "/tmp/worker.key".to_string();
        app_config.relay.worker_tls_client_ca = "/tmp/worker-ca.pem".to_string();
        app_config.relay.bridge_encryption_mode = BridgeEncryptionMode::Required;
        app_config.relay.bridge_encryption_key = "relay-key".to_string();
        app_config.worker.tls_mode = WorkerTlsMode::Mtls;
        app_config.worker.relay_ca = "/tmp/relay-ca.pem".to_string();
        app_config.worker.client_cert = "/tmp/client.crt".to_string();
        app_config.worker.client_key = "/tmp/client.key".to_string();
        app_config.worker.bridge_encryption_mode = BridgeEncryptionMode::Required;
        app_config.worker.bridge_encryption_key = "worker-key".to_string();
        let (bridge, admin) = reserved_pair();

        let (relay_config, worker_config) = bridge_connected_configs(app_config, &bridge, &admin);

        assert_eq!(relay_config.worker_tls_mode, TlsMode::Off);
        assert!(relay_config.worker_tls_cert.is_empty());
        assert!(relay_config.worker_tls_key.is_empty());
        assert!(relay_config.worker_tls_client_ca.is_empty());
        assert_eq!(worker_config.tls_mode, WorkerTlsMode::Off);
        assert!(worker_config.relay_ca.is_empty());
        assert!(worker_config.client_cert.is_empty());
        assert!(worker_config.client_key.is_empty());
        assert_eq!(
            relay_config.bridge_encryption_mode,
            BridgeEncryptionMode::Off
        );
        assert!(relay_config.bridge_encryption_key.is_empty());
        assert_eq!(
            worker_config.bridge_encryption_mode,
            BridgeEncryptionMode::Off
        );
        assert!(worker_config.bridge_encryption_key.is_empty());
    }

    #[test]
    fn the_worker_token_is_authoritative_for_both_halves() {
        for (relay_token, worker_token, expected) in [
            ("relay-default-token", "worker-token", "worker-token"),
            ("relay-token", "", ""),
            ("relay-default-token", "", ""),
        ] {
            let mut app_config = AppConfig::default();
            app_config.relay.worker_token = relay_token.to_string();
            app_config.worker.worker_token = worker_token.to_string();
            let (bridge, admin) = reserved_pair();

            let (relay_config, worker_config) =
                bridge_connected_configs(app_config, &bridge, &admin);

            assert_eq!(
                relay_config.worker_token, expected,
                "relay side of {worker_token:?}"
            );
            assert_eq!(
                worker_config.worker_token, expected,
                "worker side of {worker_token:?}"
            );
        }
    }
}
