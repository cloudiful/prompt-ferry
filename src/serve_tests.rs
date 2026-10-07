//! Tests for the integrated startup, split out of `src/serve.rs` to keep the
//! implementation module under the repository's 400-line cap. As a `#[path]`
//! child of `serve`, the module sees `serve`'s private items through `super`.

use super::*;
use crate::config::{AppConfig, binds::BindKind};
use std::net::SocketAddr;

fn binds_for(app_config: &AppConfig, args: &ServeArgs) -> IntegratedBinds {
    IntegratedBinds::reserve(app_config, args).expect("reserve integrated binds")
}

/// Configuration whose every bind takes a free loopback port.
///
/// Reservations are held for the duration of a test, so two tests sharing
/// the built-in fixed ports would collide with each other whenever the
/// suite runs them in parallel.
fn free_port_config() -> AppConfig {
    let mut app_config = AppConfig::default();
    app_config.relay.bind = "127.0.0.1:0".to_string();
    app_config.serve.internal_worker_bind = "127.0.0.1:0".to_string();
    app_config.worker.admin_bind = "127.0.0.1:0".to_string();
    app_config
}

#[test]
fn derives_the_worker_relay_url_from_the_reserved_bridge() {
    let app_config = free_port_config();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (relay_config, worker_config) = derive_configs(app_config, &binds);

    assert_eq!(
        relay_config.worker_bind,
        binds.worker_bridge.addr().to_string()
    );
    assert_eq!(
        worker_config.relay_urls,
        vec![format!("ws://{}/ws/worker", binds.worker_bridge.addr())],
        "the derived URL must name the bridge listener that will really accept it"
    );
}

#[test]
fn derives_an_automatic_bridge_port_when_configuration_asks_for_one() {
    let mut app_config = free_port_config();
    app_config.serve.internal_worker_bind = "127.0.0.1:0".to_string();
    let args = ServeArgs {
        internal_worker_bind: Some("127.0.0.1:0".to_string()),
    };
    let binds = binds_for(&app_config, &args);

    assert_eq!(binds.worker_bridge.kind(), BindKind::Automatic);
    assert_ne!(binds.worker_bridge.addr().port(), 0);
    assert!(binds.worker_bridge.addr().ip().is_loopback());

    let (_, worker_config) = derive_configs(app_config, &binds);
    assert_eq!(
        worker_config.relay_urls,
        vec![format!("ws://{}/ws/worker", binds.worker_bridge.addr())]
    );
}

#[test]
fn keeps_an_explicit_bridge_bind() {
    let app_config = free_port_config();
    let probe = free_loopback_address();
    let args = ServeArgs {
        internal_worker_bind: Some(probe.to_string()),
    };
    let binds = binds_for(&app_config, &args);

    assert_eq!(binds.worker_bridge.kind(), BindKind::Explicit);
    assert_eq!(binds.worker_bridge.addr(), probe);
}

/// A concrete free loopback address a test can configure explicitly.
fn free_loopback_address() -> SocketAddr {
    let reserved = reserve_bind("127.0.0.1:0", "test", false).expect("probe a free loopback port");
    let addr = reserved.addr();
    drop(reserved.into_listener().expect("release the probed socket"));
    addr
}

#[test]
fn rejects_a_bridge_bind_that_is_not_loopback() {
    let args = ServeArgs {
        internal_worker_bind: Some("0.0.0.0:8788".to_string()),
    };
    let error = IntegratedBinds::reserve(&free_port_config(), &args)
        .expect_err("the in-process bridge must stay loopback-only");

    assert!(
        error.to_string().contains("loopback"),
        "unexpected error: {error:#}"
    );
}

#[test]
fn reports_an_occupied_explicit_public_bind_before_starting() {
    // Hold the port for the whole test; a released reservation would free it
    // again and stop being a collision.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold a port");
    let mut app_config = free_port_config();
    app_config.relay.bind = held.addr().to_string();

    let error = IntegratedBinds::reserve(&app_config, &ServeArgs::default())
        .expect_err("the port is taken");

    let message = format!("{error:#}");
    assert!(message.contains("relay.bind"), "message was: {message}");
}

#[test]
fn integrated_mode_overrides_internal_bridge_security_settings() {
    let mut app_config = free_port_config();
    app_config.relay.worker_token = "relay-token".to_string();
    app_config.worker.worker_token = "worker-token".to_string();
    app_config.worker.tls_mode = WorkerTlsMode::Mtls;
    app_config.worker.relay_ca = "/tmp/relay-ca.pem".to_string();
    app_config.worker.client_cert = "/tmp/client.crt".to_string();
    app_config.worker.client_key = "/tmp/client.key".to_string();
    app_config.worker.bridge_encryption_mode = BridgeEncryptionMode::Required;
    app_config.worker.bridge_encryption_key = "worker-key".to_string();
    app_config.relay.worker_tls_mode = TlsMode::Mtls;
    app_config.relay.worker_tls_cert = "/tmp/worker.crt".to_string();
    app_config.relay.worker_tls_key = "/tmp/worker.key".to_string();
    app_config.relay.worker_tls_client_ca = "/tmp/worker-ca.pem".to_string();
    app_config.relay.bridge_encryption_mode = BridgeEncryptionMode::Required;
    app_config.relay.bridge_encryption_key = "relay-key".to_string();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (relay_config, worker_config) = derive_configs(app_config, &binds);

    assert_eq!(relay_config.worker_token, "worker-token");
    assert_eq!(worker_config.worker_token, "worker-token");
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
fn integrated_mode_preserves_an_explicitly_empty_worker_token_on_both_sides() {
    let mut app_config = free_port_config();
    app_config.relay.worker_token = String::new();
    app_config.worker.worker_token = String::new();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (relay_config, worker_config) = derive_configs(app_config, &binds);

    assert_eq!(relay_config.worker_token, "");
    assert_eq!(worker_config.worker_token, "");
}

#[test]
fn an_empty_worker_token_overrides_a_nonempty_relay_default() {
    let mut app_config = free_port_config();
    app_config.relay.worker_token = "relay-default-token".to_string();
    app_config.worker.worker_token = String::new();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (relay_config, worker_config) = derive_configs(app_config, &binds);

    assert_eq!(relay_config.worker_token, "");
    assert_eq!(worker_config.worker_token, "");
}

#[test]
fn integrated_mode_leaves_the_public_relay_bind_to_the_relay_configuration() {
    let mut app_config = free_port_config();
    let probe = free_loopback_address();
    app_config.relay.bind = probe.to_string();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (relay_config, _) = derive_configs(app_config, &binds);

    assert_eq!(relay_config.bind, probe.to_string());
}

#[test]
fn the_worker_receives_the_resolved_admin_address_not_the_configured_string() {
    for configured in ["", "   ", "0.0.0.0:0", "127.0.0.1:0", "127.0.0.1:8789"] {
        let mut app_config = free_port_config();
        app_config.worker.admin_bind = configured.to_string();
        let binds = binds_for(&app_config, &ServeArgs::default());

        let (_, worker_config) = derive_configs(app_config, &binds);

        assert_eq!(
            worker_config.admin_bind,
            binds.admin.addr().to_string(),
            "configured admin bind `{configured}` must propagate as the resolved address"
        );
        assert_ne!(
            worker_config
                .admin_bind
                .parse::<SocketAddr>()
                .expect("resolved")
                .port(),
            0,
            "the worker must never be told to bind port 0 again"
        );
    }
}

#[test]
fn an_explicit_admin_bind_is_propagated_verbatim() {
    let mut app_config = free_port_config();
    let probe = free_loopback_address();
    app_config.worker.admin_bind = probe.to_string();
    let binds = binds_for(&app_config, &ServeArgs::default());

    let (_, worker_config) = derive_configs(app_config, &binds);

    assert_eq!(binds.admin.kind(), BindKind::Explicit);
    assert_eq!(worker_config.admin_bind, probe.to_string());
}

#[test]
fn an_occupied_explicit_admin_bind_fails_before_the_worker_starts() {
    // Hold the port for the whole test so the reservation cannot free it.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold a port");
    let mut app_config = free_port_config();
    app_config.worker.admin_bind = held.addr().to_string();

    let error = IntegratedBinds::reserve(&app_config, &ServeArgs::default())
        .expect_err("the admin port is taken");

    let message = format!("{error:#}");
    assert!(
        message.contains("worker.admin_bind"),
        "message was: {message}"
    );
}

#[test]
fn the_admin_reservation_holds_its_port_until_the_worker_adopts_it() {
    let binds = binds_for(&free_port_config(), &ServeArgs::default());
    let addr = binds.admin.addr();

    // Nothing else can take the port while the worker is bootstrapping.
    let collision = reserve_bind(&addr.to_string(), "worker.admin_bind", false)
        .expect_err("the reservation must hold the admin port");
    assert!(
        format!("{collision:#}").contains("worker.admin_bind"),
        "unexpected error: {collision:#}"
    );

    // Adopting it is the worker's job; here handing the socket over must
    // succeed and keep the same address.
    let adopted = binds.admin.into_listener().expect("hand the socket over");
    assert_eq!(
        adopted.local_addr().expect("adopted address"),
        addr,
        "the worker must inherit the exact reserved address"
    );
}

#[test]
fn the_launch_context_tracks_the_entrypoint_and_the_build_target() {
    // Only the no-subcommand path may launch, and only on Windows builds;
    // on other platforms cfg!(windows) is false, so nothing launches.
    let expected_windows = cfg!(windows);
    assert_eq!(
        Entrypoint::NoSubcommand.launch_context(),
        crate::browser::LaunchContext {
            no_subcommand: true,
            windows: expected_windows
        }
    );
    assert_eq!(
        Entrypoint::ServeAlias.launch_context(),
        crate::browser::LaunchContext {
            no_subcommand: false,
            windows: expected_windows
        }
    );
}

#[test]
fn no_entrypoint_launches_a_browser_off_windows() {
    if cfg!(windows) {
        return;
    }
    let launcher = crate::browser::RecordingLauncher::default();
    crate::browser::open_admin_ui(
        Entrypoint::NoSubcommand.launch_context(),
        "127.0.0.1:8789".parse().expect("loopback socket"),
        &launcher,
    );
    assert!(launcher.urls().is_empty());
}
