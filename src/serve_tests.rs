//! How a startup resolves its role and reports its static launch decisions.
//! Split out of `src/serve.rs` to keep the implementation module small; as a
//! `#[path]` child it sees `serve`'s private items through `super`.

use super::*;
use crate::config::AppConfig;

fn free_port_config() -> AppConfig {
    let mut app_config = AppConfig::default();
    app_config.relay.bind = "127.0.0.1:0".to_string();
    app_config.serve.internal_worker_bind = "127.0.0.1:0".to_string();
    app_config.relay.worker_bind = "127.0.0.1:0".to_string();
    app_config.relay.admin_bind = "127.0.0.1:0".to_string();
    app_config.worker.admin_bind = "127.0.0.1:0".to_string();
    app_config
}

#[test]
fn the_no_subcommand_startup_honors_every_configured_role() {
    for role in [HostRole::Integrated, HostRole::Worker, HostRole::Relay] {
        let mut app_config = free_port_config();
        app_config.host.role = role;

        assert_eq!(
            resolve_role(&app_config, Entrypoint::NoSubcommand),
            role,
            "the no-subcommand startup must run the configured role {role:?}"
        );
    }
}

#[test]
fn the_no_subcommand_startup_runs_the_configured_role() {
    let mut app_config = free_port_config();
    app_config.host.role = HostRole::Worker;

    assert_eq!(
        resolve_role(&app_config, Entrypoint::NoSubcommand),
        HostRole::Worker
    );
}

#[test]
fn a_host_without_a_role_configuration_keeps_the_integrated_startup() {
    let app_config = free_port_config();

    assert_eq!(app_config.host.role, HostRole::Integrated);
    assert_eq!(
        resolve_role(&app_config, Entrypoint::NoSubcommand),
        HostRole::Integrated
    );
}

#[test]
fn the_serve_alias_still_starts_the_relay_and_worker_together() {
    for role in [HostRole::Integrated, HostRole::Worker, HostRole::Relay] {
        let mut app_config = free_port_config();
        app_config.host.role = role;

        assert_eq!(
            resolve_role(&app_config, Entrypoint::ServeAlias),
            HostRole::Integrated,
            "the `serve` alias names the integrated startup for role {role:?}"
        );
    }
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
        "127.0.0.1:8790".parse().expect("loopback socket"),
        &launcher,
    );
    assert!(launcher.urls().is_empty());
}

#[test]
fn a_requested_restart_is_bounded_so_a_stuck_request_cannot_hold_the_host() {
    // The restart path returns once the drain is over, so a request that never
    // completes cannot keep the old components alive forever.
    assert!(RESTART_DRAIN > Duration::ZERO);
    assert!(
        RESTART_DRAIN <= ADMIN_READY_TIMEOUT,
        "a restart must not wait longer than the startup's own reporting bound"
    );
}

#[test]
fn the_relay_command_runs_the_relay_role() {
    assert!(HostRole::Relay.runs_relay() && !HostRole::Relay.runs_worker());
    assert!(
        !HostRole::Relay.uses_local_bridge(),
        "the command starts no local worker, so it bridges nothing in-process"
    );
}
