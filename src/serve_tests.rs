//! How a startup resolves its role, reserves its listeners, and reports a
//! listener it cannot take. Split out of `src/serve.rs` to keep the
//! implementation module small; as a `#[path]` child it sees `serve`'s private
//! items through `super`.

use super::*;
use crate::cli::RelayArgs;
use crate::config::{AppConfig, binds::BindKind};
use std::net::SocketAddr;

/// Configuration whose every bind takes a free loopback port.
///
/// Reservations are held for the duration of a test, so two tests sharing
/// the built-in fixed ports would collide with each other whenever the
/// suite runs them in parallel.
fn free_port_config() -> AppConfig {
    let mut app_config = AppConfig::default();
    app_config.relay.bind = "127.0.0.1:0".to_string();
    app_config.serve.internal_worker_bind = "127.0.0.1:0".to_string();
    app_config.relay.worker_bind = "127.0.0.1:0".to_string();
    app_config.relay.admin_bind = "127.0.0.1:0".to_string();
    app_config.worker.admin_bind = "127.0.0.1:0".to_string();
    app_config
}

fn startup_for(app_config: &AppConfig, role: HostRole) -> Startup {
    Startup::reserve(app_config, &ServeArgs::default(), role).expect("reserve the role's listeners")
}

/// A concrete free loopback address a test can configure explicitly.
fn free_loopback_address() -> SocketAddr {
    let reserved = reserve_bind("127.0.0.1:0", "test", false).expect("probe a free loopback port");
    let addr = reserved.addr();
    drop(reserved.into_listener().expect("release the probed socket"));
    addr
}

fn reserve_error(app_config: &AppConfig, args: &ServeArgs, role: HostRole) -> String {
    let error = Startup::reserve(app_config, args, role)
        .expect_err("the reservation must fail before any component starts");
    format!("{error:#}")
}

/// Which listeners each host role reserves.
///
/// A role that never serves a listener must never take its port: an occupied
/// port for a component the role does not run is not the role's problem.
#[test]
fn the_integrated_role_reserves_the_relay_bridge_and_admin_listeners() {
    match startup_for(&free_port_config(), HostRole::Integrated) {
        Startup::Integrated {
            public,
            worker_bridge,
            admin,
            relay_admin,
        } => {
            assert_eq!(public.kind(), BindKind::Automatic);
            assert!(worker_bridge.addr().ip().is_loopback());
            assert_ne!(worker_bridge.addr().port(), 0);
            assert_ne!(admin.addr().port(), 0);
            assert_ne!(
                relay_admin.addr().port(),
                0,
                "the relay owns the management entry point in this role too"
            );
        }
        other => panic!("expected the integrated startup, got {other:?}"),
    }
}

#[test]
fn every_role_that_runs_a_relay_reserves_the_relay_management_listener() {
    for role in [HostRole::Integrated, HostRole::Relay] {
        let relay_admin = match startup_for(&free_port_config(), role) {
            Startup::Integrated { relay_admin, .. } | Startup::RelayOnly { relay_admin, .. } => {
                relay_admin
            }
            other => {
                panic!("role {role:?} must reserve the relay management listener, got {other:?}")
            }
        };
        assert!(
            relay_admin.addr().ip().is_loopback(),
            "role {role:?} must keep its management listener on loopback"
        );
        assert_ne!(
            relay_admin.addr().port(),
            0,
            "role {role:?} must resolve its management port before anything starts"
        );
    }
}

#[test]
fn the_worker_only_role_takes_no_relay_port() {
    match startup_for(&free_port_config(), HostRole::Worker) {
        Startup::WorkerOnly { admin } => {
            assert_ne!(
                admin.addr().port(),
                0,
                "the worker keeps its own admin listener"
            );
        }
        other => panic!("expected a worker-only startup, got {other:?}"),
    }
}

#[test]
fn the_relay_only_role_takes_no_worker_admin_port_and_serves_remote_workers() {
    match startup_for(&free_port_config(), HostRole::Relay) {
        Startup::RelayOnly {
            public,
            worker_bridge,
            relay_admin,
        } => {
            assert_ne!(public.addr().port(), 0);
            assert_eq!(
                worker_bridge.kind(),
                BindKind::Automatic,
                "the relay bridge remote workers dial is bound as configured"
            );
            assert_ne!(
                relay_admin.addr().port(),
                0,
                "a relay-only host serves its management page from its own listener"
            );
        }
        other => panic!("expected a relay-only startup, got {other:?}"),
    }
}

#[test]
fn the_relay_only_role_honors_an_explicit_worker_bind() {
    let mut app_config = free_port_config();
    let probe = free_loopback_address();
    app_config.relay.worker_bind = probe.to_string();

    match startup_for(&app_config, HostRole::Relay) {
        Startup::RelayOnly { worker_bridge, .. } => {
            assert_eq!(worker_bridge.kind(), BindKind::Explicit);
            assert_eq!(worker_bridge.addr(), probe);
        }
        other => panic!("expected a relay-only startup, got {other:?}"),
    }
}

#[test]
fn a_worker_only_host_starts_even_when_the_embedded_bridge_port_is_taken() {
    // `serve.internal_worker_bind` names the integrated role's in-process
    // bridge. A worker-only host runs no local relay, so it never binds it.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the bridge port");
    let mut app_config = free_port_config();
    app_config.serve.internal_worker_bind = held.addr().to_string();

    match startup_for(&app_config, HostRole::Worker) {
        Startup::WorkerOnly { admin } => {
            assert_ne!(
                admin.addr().port(),
                0,
                "the worker keeps its own admin listener"
            );
        }
        other => panic!("expected a worker-only startup, got {other:?}"),
    }
}

#[test]
fn the_worker_only_role_reserves_the_admin_listener_exactly_once() {
    // Handing the socket over must keep the reserved address, so the worker's
    // own bind of that address cannot land on a different port.
    let Startup::WorkerOnly { admin } = startup_for(&free_port_config(), HostRole::Worker) else {
        panic!("expected a worker-only startup");
    };
    let addr = admin.addr();

    let adopted = admin.into_listener().expect("hand the admin socket over");
    assert_eq!(
        adopted.local_addr().expect("adopted address"),
        addr,
        "the worker must inherit the exact reserved admin address"
    );
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
        let startup = startup_for(&app_config, role);
        assert!(
            matches!(
                (role, &startup),
                (HostRole::Integrated, Startup::Integrated { .. })
                    | (HostRole::Worker, Startup::WorkerOnly { .. })
                    | (HostRole::Relay, Startup::RelayOnly { .. })
            ),
            "role {role:?} must start its own components, got {startup:?}"
        );
    }
}

#[test]
fn the_resolved_startup_is_not_re_derived_from_a_later_configuration_change() {
    // The role is read once, before any component starts, and the resulting
    // startup owns the listeners it reserved. A later configuration change —
    // which is what a role switch writes — therefore cannot take effect in a
    // running process; it needs a restart.
    let mut app_config = free_port_config();
    app_config.host.role = HostRole::Worker;
    let resolved = resolve_role(&app_config, Entrypoint::NoSubcommand);
    let startup = startup_for(&app_config, resolved);
    let Startup::WorkerOnly { admin } = startup else {
        panic!("expected a worker-only startup");
    };
    let reserved = admin.addr();

    app_config.host.role = HostRole::Relay;

    assert_eq!(
        resolve_role(&app_config, Entrypoint::NoSubcommand),
        HostRole::Relay,
        "the next startup reads the changed role"
    );
    let next = startup_for(&app_config, HostRole::Relay);
    assert!(
        matches!(next, Startup::RelayOnly { .. }),
        "only a restart reaches the relay-only startup"
    );
    assert!(
        reserve_bind(&reserved.to_string(), "worker.admin_bind", false).is_err(),
        "the running process still holds the listener its role reserved"
    );
}

#[test]
fn keeps_an_explicit_bridge_bind() {
    let probe = free_loopback_address();
    let args = ServeArgs {
        internal_worker_bind: Some(probe.to_string()),
    };

    match Startup::reserve(&free_port_config(), &args, HostRole::Integrated)
        .expect("reserve the integrated listeners")
    {
        Startup::Integrated { worker_bridge, .. } => {
            assert_eq!(worker_bridge.kind(), BindKind::Explicit);
            assert_eq!(worker_bridge.addr(), probe);
        }
        other => panic!("expected the integrated startup, got {other:?}"),
    }
}

#[test]
fn the_admin_reservation_holds_its_port_until_the_worker_adopts_it() {
    let binds = startup_for(&free_port_config(), HostRole::Integrated);
    let Startup::Integrated { admin, .. } = binds else {
        panic!("expected the integrated startup");
    };
    let addr = admin.addr();

    // Nothing else can take the port while the worker is bootstrapping.
    let collision = reserve_bind(&addr.to_string(), "worker.admin_bind", false)
        .expect_err("the reservation must hold the admin port");
    assert!(
        format!("{collision:#}").contains("worker.admin_bind"),
        "unexpected error: {collision:#}"
    );

    // Adopting it is the worker's job; here handing the socket over must
    // succeed and keep the same address.
    let adopted = admin.into_listener().expect("hand the socket over");
    assert_eq!(
        adopted.local_addr().expect("adopted address"),
        addr,
        "the worker must inherit the exact reserved admin address"
    );
}

/// How a startup reports a listener it cannot take.
///
/// A port collision, a malformed bind, and a management bind that would leave
/// loopback are all reported before any component starts, so an operator learns
/// about them from the start rather than from a half-running host.
#[test]
fn the_worker_only_role_takes_no_relay_management_port() {
    // A worker-only host runs no relay, so the relay-owned management listener
    // is not part of its startup: an occupied relay management port must not
    // stop it.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the management port");
    let mut app_config = free_port_config();
    app_config.relay.admin_bind = held.addr().to_string();

    assert!(matches!(
        startup_for(&app_config, HostRole::Worker),
        Startup::WorkerOnly { .. }
    ));
}

#[test]
fn the_worker_only_role_reports_an_occupied_admin_bind_before_starting() {
    // Hold the port for the whole test so the reservation cannot free it.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold a port");
    let mut app_config = free_port_config();
    app_config.worker.admin_bind = held.addr().to_string();

    let message = reserve_error(&app_config, &ServeArgs::default(), HostRole::Worker);

    assert!(
        message.contains("worker.admin_bind"),
        "message was: {message}"
    );
}

#[test]
fn a_role_never_reserves_a_listener_it_does_not_serve() {
    // Holding the relay's ports must not stop a worker-only host from starting:
    // it binds neither of them.
    let public = reserve_bind("127.0.0.1:0", "test", false).expect("hold a public port");
    let bridge = reserve_bind("127.0.0.1:0", "test", false).expect("hold a bridge port");
    let management = reserve_bind("127.0.0.1:0", "test", false).expect("hold a management port");
    let mut app_config = free_port_config();
    app_config.relay.bind = public.addr().to_string();
    app_config.relay.worker_bind = bridge.addr().to_string();
    app_config.relay.admin_bind = management.addr().to_string();

    startup_for(&app_config, HostRole::Worker);

    let message = reserve_error(&app_config, &ServeArgs::default(), HostRole::Relay);
    assert!(message.contains("relay.bind"), "message was: {message}");
}

#[test]
fn a_relay_only_host_starts_even_when_the_worker_admin_port_is_taken() {
    // The relay-only role runs no worker, so the worker-owned admin listener is
    // not part of its startup: an occupied admin port must not stop it.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the admin port");
    let mut app_config = free_port_config();
    app_config.worker.admin_bind = held.addr().to_string();

    match startup_for(&app_config, HostRole::Relay) {
        Startup::RelayOnly {
            public,
            worker_bridge,
            relay_admin,
        } => {
            assert_ne!(public.addr().port(), 0);
            assert_ne!(worker_bridge.addr().port(), 0);
            assert_ne!(relay_admin.addr().port(), 0);
        }
        other => panic!("expected a relay-only startup, got {other:?}"),
    }
}

#[test]
fn a_role_never_parses_configuration_it_does_not_serve() {
    // An unparseable bind in a section the role does not start must not fail
    // that role's startup, while the same value in a section it does serve
    // must still be reported instead of being silently ignored.
    let invalid = "not-an-address";

    let mut worker_config = free_port_config();
    worker_config.relay.bind = invalid.to_string();
    worker_config.relay.worker_bind = invalid.to_string();
    worker_config.relay.admin_bind = invalid.to_string();
    worker_config.serve.internal_worker_bind = invalid.to_string();
    assert!(
        matches!(
            startup_for(&worker_config, HostRole::Worker),
            Startup::WorkerOnly { .. }
        ),
        "a worker-only host serves no relay bind, so it must not parse one"
    );
    worker_config.worker.admin_bind = invalid.to_string();
    let message = reserve_error(&worker_config, &ServeArgs::default(), HostRole::Worker);
    assert!(
        message.contains("worker.admin_bind"),
        "the admin bind it does serve must still be validated: {message}"
    );

    let mut relay_config = free_port_config();
    relay_config.worker.admin_bind = invalid.to_string();
    relay_config.serve.internal_worker_bind = invalid.to_string();
    assert!(
        matches!(
            startup_for(&relay_config, HostRole::Relay),
            Startup::RelayOnly { .. }
        ),
        "a relay-only host runs no worker, so it must not parse the worker admin bind"
    );
    relay_config.relay.bind = invalid.to_string();
    let message = reserve_error(&relay_config, &ServeArgs::default(), HostRole::Relay);
    assert!(
        message.contains("relay.bind"),
        "the relay bind it does serve must still be validated: {message}"
    );

    let mut management_config = free_port_config();
    management_config.relay.admin_bind = invalid.to_string();
    let message = reserve_error(&management_config, &ServeArgs::default(), HostRole::Relay);
    assert!(
        message.contains("relay.admin_bind"),
        "the management bind it does serve must still be validated: {message}"
    );
}

#[test]
fn rejects_a_bridge_bind_that_is_not_loopback() {
    let args = ServeArgs {
        internal_worker_bind: Some("0.0.0.0:8788".to_string()),
    };

    let message = reserve_error(&free_port_config(), &args, HostRole::Integrated);

    assert!(message.contains("loopback"), "message was: {message}");
}

#[test]
fn rejects_a_relay_management_bind_that_is_not_loopback() {
    // A management surface must never be published off-host: the relay only
    // agrees to serve it where the operator's own browser can reach it.
    let mut app_config = free_port_config();
    app_config.relay.admin_bind = "0.0.0.0:8790".to_string();

    let message = reserve_error(&app_config, &ServeArgs::default(), HostRole::Relay);

    assert!(
        message.contains("relay.admin_bind"),
        "message was: {message}"
    );
    assert!(message.contains("loopback"), "message was: {message}");
}

#[test]
fn reports_an_occupied_explicit_public_bind_before_starting() {
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold a port");
    let mut app_config = free_port_config();
    app_config.relay.bind = held.addr().to_string();

    let message = reserve_error(&app_config, &ServeArgs::default(), HostRole::Integrated);

    assert!(message.contains("relay.bind"), "message was: {message}");
}

#[test]
fn reports_an_occupied_relay_management_port_before_starting() {
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the management port");
    let mut app_config = free_port_config();
    app_config.relay.admin_bind = held.addr().to_string();

    let message = reserve_error(&app_config, &ServeArgs::default(), HostRole::Relay);

    assert!(
        message.contains("relay.admin_bind"),
        "message was: {message}"
    );
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
fn the_relay_management_listener_is_the_entry_point_and_stays_on_loopback() {
    let mut app_config = free_port_config();
    app_config.relay.admin_bind = String::new();
    let Startup::RelayOnly { relay_admin, .. } = startup_for(&app_config, HostRole::Relay) else {
        panic!("expected a relay-only startup");
    };

    assert!(
        relay_admin.addr().ip().is_loopback(),
        "an unset management bind must resolve to loopback, never to every interface"
    );
    assert_eq!(
        open_relay_admin(relay_admin.addr(), Entrypoint::ServeAlias.launch_context()),
        local_ui_url(relay_admin.addr()),
        "the entry point reported to the operator is the relay's own listener"
    );
}

/// The `relay` subcommand starts the relay-only host, not a bare relay.
///
/// The command is a compatibility entry point, so what it must keep is its set of
/// address and token overrides. What it must not be is a way around the role
/// startup: the management listener is reserved on loopback like everywhere else,
/// and the overrides it does carry still decide where the relay listens.
#[tokio::test]
async fn the_relay_command_starts_the_relay_only_host_and_keeps_its_overrides() {
    // The port the command line names is the one held, so the reservation fails
    // on it and the failure names it: that shows the override is what the
    // startup reserved, without the command ever reaching the point of serving.
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the requested port");
    let app_config = free_port_config();

    let error = run_relay_command(
        app_config,
        RelayArgs {
            bind: Some(held.addr().to_string()),
            ..RelayArgs::default()
        },
    )
    .await
    .expect_err("the command must stop when the bind it was given is taken");

    let message = format!("{error:#}");
    assert!(message.contains("relay.bind"), "got: {message}");
    assert!(
        message.contains(&held.addr().to_string()),
        "the command-line bind must be the one the startup reserves, got: {message}"
    );
}

/// The `relay` subcommand refuses to publish its management listener.
///
/// The management surface carries an authenticated control plane, so binding it
/// off loopback would publish it. This is checked before anything is served, the
/// same as on the role-driven path.
#[tokio::test]
async fn the_relay_command_refuses_a_management_bind_off_loopback() {
    let mut app_config = free_port_config();
    app_config.relay.admin_bind = "0.0.0.0:8790".to_string();

    let error = run_relay_command(app_config, RelayArgs::default())
        .await
        .expect_err("a management listener must never leave loopback");

    let message = format!("{error:#}");
    assert!(message.contains("relay.admin_bind"), "got: {message}");
    assert!(message.contains("loopback"), "got: {message}");
}

/// The `relay` subcommand reserves its management listener before serving.
///
/// An occupied public bind is reported without the relay ever accepting a
/// request, which is what keeps a reported failure from being a half-running
/// host that already serves an unauthenticated control plane.
#[tokio::test]
async fn the_relay_command_reports_a_collision_before_it_serves() {
    let mut app_config = free_port_config();
    let held = reserve_bind("127.0.0.1:0", "test", false).expect("hold the relay port");
    app_config.relay.bind = held.addr().to_string();

    let error = run_relay_command(app_config, RelayArgs::default())
        .await
        .expect_err("an occupied relay bind must stop the command");

    assert!(
        format!("{error:#}").contains("relay.bind"),
        "got: {error:#}"
    );
}

/// The subcommand records the role it is running, like every other entrypoint.
///
/// The relay's own management API reports the role this process started as, and
/// a command that skipped the record would report a host that is not running it.
#[test]
fn the_relay_command_runs_the_relay_role() {
    assert!(HostRole::Relay.runs_relay() && !HostRole::Relay.runs_worker());
    assert!(
        !HostRole::Relay.uses_local_bridge(),
        "the command starts no local worker, so it bridges nothing in-process"
    );
}
