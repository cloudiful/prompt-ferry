//! Startup for the host role: no subcommand, or the `serve` compatibility alias.
//!
//! Both entrypoints land here, so there is exactly one startup that resolves
//! addresses and starts components. The host-local role decides which of them
//! run: the relay, an embedded worker, and the admin listener that serves the
//! management UI. Every listener a role needs is reserved before anything
//! starts, so a port collision is reported before a component connects, the
//! worker's relay URL names the listener that will really accept it, and the
//! admin listener reports the address it bound.
//!
//! A role also decides what stays untouched. The integrated role owns both
//! halves of an in-process bridge, so it switches the transport security and
//! payload encryption that cannot apply to a loopback bridge off. The
//! worker-only and relay-only roles each keep the security settings they were
//! configured with, because each of them talks to the other half over a network
//! it does not control.

use crate::{
    browser::{LaunchContext, SystemBrowserLauncher, open_admin_ui},
    cli::ServeArgs,
    config::{
        AppConfig, HostRole, IntegratedStartup,
        binds::{ReservedBind, local_ui_url, reserve_bind},
        host_startup, integrated_bridge, management_token,
    },
    relay, worker,
};
use std::{net::SocketAddr, time::Duration};
use tracing::{info, warn};

/// How long the admin listener gets to report the address it bound. This bounds
/// a log line only: the running components keep serving either way, and a cold
/// database migration can legitimately take longer than this.
const ADMIN_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a requested restart lets in-flight requests finish before the
/// components it is replacing are dropped.
const RESTART_DRAIN: Duration = Duration::from_secs(5);

/// Which entrypoint reached this startup.
///
/// Only the no-subcommand path is allowed to open a browser; the `serve`
/// alias keeps its scripted, quiet behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entrypoint {
    NoSubcommand,
    ServeAlias,
}

impl Entrypoint {
    fn launch_context(self) -> crate::browser::LaunchContext {
        crate::browser::LaunchContext {
            no_subcommand: self == Entrypoint::NoSubcommand,
            windows: cfg!(windows),
        }
    }
}

pub async fn run(
    app_config: AppConfig,
    args: ServeArgs,
    entrypoint: Entrypoint,
) -> anyhow::Result<()> {
    let role = resolve_role(&app_config, entrypoint);
    run_role(app_config, args, role, entrypoint).await
}

/// Start the relay named by the `relay` subcommand.
///
/// The subcommand stays a compatibility entry point, but it is no longer a
/// shortcut around the role-driven startup. It runs the same relay-only host, so
/// it reserves the same three listeners, refuses a management bind that is not
/// loopback, resolves and persists a management token before anything is served,
/// and stops when the management page asks for a restart. What stays specific to
/// the command line is the set of address and token overrides it applies, and the
/// fact that it never opens a browser.
pub async fn run_relay_command(
    mut app_config: AppConfig,
    args: crate::cli::RelayArgs,
) -> anyhow::Result<()> {
    app_config.relay = app_config.relay.merge_args(args);
    run_role(
        app_config,
        ServeArgs::default(),
        HostRole::Relay,
        Entrypoint::ServeAlias,
    )
    .await
}

/// Run one resolved role for the whole life of the process.
///
/// Every entrypoint comes through here, so no path into this process can skip
/// what a role is responsible for: the listeners it reserves, the management
/// token the relay's own control plane needs, and the stop a restart request
/// asks for.
async fn run_role(
    app_config: AppConfig,
    args: ServeArgs,
    role: HostRole,
    entrypoint: Entrypoint,
) -> anyhow::Result<()> {
    host_startup::activate(role);
    if !role.uses_local_bridge()
        && let Some(bind) = args.internal_worker_bind.as_deref()
    {
        warn!(
            role = role.as_str(),
            %bind,
            "`--internal-worker-bind` names the in-process relay bridge and is not used by this role"
        );
    }
    tokio::select! {
        result = start_role(app_config, &args, role, entrypoint) => result,
        () = stop_for_restart() => {
            info!("this host is stopping so the settings saved in its management page take effect");
            Ok(())
        }
    }
}

/// Resolve once the relay's management page asks for a restart, and never before.
///
/// The startup future is dropped when this resolves, so the drain below is what
/// lets a response that is already on the wire finish before the components go
/// away. It is bounded, so a request that never completes cannot hold the host.
async fn stop_for_restart() {
    relay::admin::restart::wait(relay::admin::restart::subscribe()).await;
    tokio::time::sleep(RESTART_DRAIN).await;
}

/// Start the components the resolved role runs.
async fn start_role(
    app_config: AppConfig,
    args: &ServeArgs,
    role: HostRole,
    entrypoint: Entrypoint,
) -> anyhow::Result<()> {
    // Each role's startup is boxed so this dispatch stays small: inlining all
    // three of them would make the whole process future oversized.
    match Startup::reserve(&app_config, args, role)? {
        Startup::Integrated {
            public,
            worker_bridge,
            admin,
            relay_admin,
        } => {
            Box::pin(run_integrated(
                app_config,
                public,
                worker_bridge,
                admin,
                relay_admin,
                entrypoint,
            ))
            .await
        }
        Startup::WorkerOnly { admin } => {
            Box::pin(run_worker_only(app_config, admin, entrypoint)).await
        }
        Startup::RelayOnly {
            public,
            worker_bridge,
            relay_admin,
        } => {
            Box::pin(run_relay_only(
                app_config,
                public,
                worker_bridge,
                relay_admin,
            ))
            .await
        }
    }
}

/// The role this startup runs as.
///
/// The no-subcommand startup reads the host-local role, so a machine can be
/// reconfigured without changing how it is started. The `serve` alias names the
/// integrated startup explicitly and keeps it, which is what makes it a stable
/// entry point for scripted and headless deployments.
fn resolve_role(app_config: &AppConfig, entrypoint: Entrypoint) -> HostRole {
    match entrypoint {
        Entrypoint::NoSubcommand => app_config.host.role,
        Entrypoint::ServeAlias => {
            if app_config.host.role != HostRole::Integrated {
                warn!(
                    configured_role = app_config.host.role.as_str(),
                    "the `serve` command always starts the relay and worker together; start without \
                     a subcommand to run the configured role"
                );
            }
            HostRole::Integrated
        }
    }
}

/// The components one startup runs, with every listener it needs reserved.
///
/// Each role reserves exactly the listeners it serves, so a role that runs no
/// local relay never takes a relay port and a role with no local worker never
/// takes the admin port. The relay's own management listener belongs to the two
/// roles that run a relay, because that listener is the relay's entry point and
/// has to exist before any worker is asked to start.
#[derive(Debug)]
enum Startup {
    /// Local relay with an embedded worker, bridged inside this process.
    Integrated {
        public: ReservedBind,
        worker_bridge: ReservedBind,
        admin: ReservedBind,
        relay_admin: ReservedBind,
    },
    /// The worker alone, keeping the management entry point it serves itself.
    WorkerOnly { admin: ReservedBind },
    /// The relay alone, with the bridge remote workers dial.
    RelayOnly {
        public: ReservedBind,
        worker_bridge: ReservedBind,
        relay_admin: ReservedBind,
    },
}

impl Startup {
    fn reserve(app_config: &AppConfig, args: &ServeArgs, role: HostRole) -> anyhow::Result<Self> {
        let admin = || reserve_bind(&app_config.worker.admin_bind, "worker.admin_bind", false);
        let public = || reserve_bind(&app_config.relay.bind, "relay.bind", false);
        // The relay's management listener is the entry point of every role that
        // runs a relay, so it is loopback-only exactly like the worker's own
        // admin listener: a management surface is never published off-host.
        let relay_admin = || reserve_bind(&app_config.relay.admin_bind, "relay.admin_bind", true);
        match role {
            HostRole::Integrated => {
                let serve_config = app_config.serve.clone().merge_args(args.clone());
                Ok(Self::Integrated {
                    public: public()?,
                    worker_bridge: reserve_bind(
                        &serve_config.internal_worker_bind,
                        "serve.internal_worker_bind",
                        true,
                    )?,
                    admin: admin()?,
                    relay_admin: relay_admin()?,
                })
            }
            HostRole::Worker => Ok(Self::WorkerOnly { admin: admin()? }),
            HostRole::Relay => Ok(Self::RelayOnly {
                public: public()?,
                worker_bridge: reserve_bind(
                    &app_config.relay.worker_bind,
                    "relay.worker_bind",
                    false,
                )?,
                relay_admin: relay_admin()?,
            }),
        }
    }
}

/// Start the relay, the embedded worker, and the management UI.
///
/// The relay owns the management entry point in this role too, so the browser
/// opens the relay's own listener rather than the worker's: the page, the relay
/// settings, and the role are reachable whether or not a worker is attached,
/// while the worker's admin listener keeps serving the worker itself.
async fn run_integrated(
    app_config: AppConfig,
    public: ReservedBind,
    worker_bridge: ReservedBind,
    admin: ReservedBind,
    relay_admin: ReservedBind,
    entrypoint: Entrypoint,
) -> anyhow::Result<()> {
    info!(
        role = HostRole::Integrated.as_str(),
        public_bind = %public.addr(),
        public_bind_kind = ?public.kind(),
        internal_worker_bind = %worker_bridge.addr(),
        internal_worker_bind_kind = ?worker_bridge.kind(),
        admin_bind = %admin.addr(),
        admin_bind_kind = ?admin.kind(),
        relay_admin_bind = %relay_admin.addr(),
        relay_admin_bind_kind = ?relay_admin.kind(),
        relay_admin_ui_url = local_ui_url(relay_admin.addr()),
        "integrated mode starting"
    );
    let (mut relay_config, worker_config) =
        integrated_bridge::bridge_connected_configs(app_config, &worker_bridge, &admin);
    management_token::resolve(&mut relay_config)?;

    // The admin socket stays reserved from here until the worker adopts it, so
    // a fixed admin port cannot be taken while the worker runs its database
    // bootstrap. Installing the hand-off also makes a failure to bind that
    // socket fail this run instead of leaving the process without its UI.
    let startup = IntegratedStartup::install(Some(admin.into_listener()?));
    let relay_admin_url = open_relay_admin(relay_admin.addr(), entrypoint.launch_context());

    // Every listener was reserved above, so the relay serves on exactly the
    // addresses the worker was told about.
    let relay = relay::run_with_listeners(
        relay_config,
        public.into_listener()?,
        worker_bridge.into_listener()?,
        relay_admin.into_listener()?,
    );
    report_worker_admin_ready(startup, Some(relay_admin_url), None);
    tokio::try_join!(relay, worker::run_embedded(worker_config))?;
    Ok(())
}

/// Start the worker alone.
///
/// The worker keeps its own management entry point: this role runs no relay, so
/// there is nothing local to serve a page or the relay's own settings, and the
/// worker's admin listener is the entry point it has always been. Nothing else
/// changes: this role dials remote relays, so its bridge TLS material, client
/// certificates, and payload encryption are the settings it was configured with.
async fn run_worker_only(
    app_config: AppConfig,
    admin: ReservedBind,
    entrypoint: Entrypoint,
) -> anyhow::Result<()> {
    let mut worker_config = app_config.worker;
    info!(
        role = HostRole::Worker.as_str(),
        admin_bind = %admin.addr(),
        admin_bind_kind = ?admin.kind(),
        admin_ui_url = local_ui_url(admin.addr()),
        "worker-only mode starting; the worker connects to the enabled remote relay list"
    );
    worker_config.admin_bind = admin.addr().to_string();
    let startup = IntegratedStartup::install(Some(admin.into_listener()?));
    report_worker_admin_ready(startup, None, Some(entrypoint.launch_context()));
    worker::run_embedded(worker_config).await
}

/// Start the relay alone.
///
/// Remote workers connect to the relay's worker bind, which is the same
/// listener the integrated role bridges its embedded worker to. The relay's own
/// management listener is the whole user interface here: the page, the relay
/// settings, the role, and the worker connection state are served without a
/// worker or a worker database anywhere in the process.
async fn run_relay_only(
    app_config: AppConfig,
    public: ReservedBind,
    worker_bridge: ReservedBind,
    relay_admin: ReservedBind,
) -> anyhow::Result<()> {
    let mut relay_config = app_config.relay;
    info!(
        role = HostRole::Relay.as_str(),
        public_bind = %public.addr(),
        public_bind_kind = ?public.kind(),
        worker_bind = %worker_bridge.addr(),
        worker_bind_kind = ?worker_bridge.kind(),
        relay_admin_bind = %relay_admin.addr(),
        relay_admin_bind_kind = ?relay_admin.kind(),
        relay_admin_ui_url = local_ui_url(relay_admin.addr()),
        "relay-only mode starting; no local worker is started"
    );
    // Both sockets are already bound, so the relay is told the addresses it
    // really serves on rather than the configured strings; a configured port of
    // `0` has resolved by now. The management bind is deliberately left as
    // configured: it is the value the page saves and compares against, so
    // overwriting it with a resolved ephemeral port would make every host look
    // like it had a pending change.
    relay_config.bind = public.addr().to_string();
    relay_config.worker_bind = worker_bridge.addr().to_string();
    management_token::resolve(&mut relay_config)?;
    relay::run_with_listeners(
        relay_config,
        public.into_listener()?,
        worker_bridge.into_listener()?,
        relay_admin.into_listener()?,
    )
    .await
}

/// Open the relay's own management page, and return its URL.
///
/// The relay's management listener is reserved before this runs and the relay
/// starts serving it immediately, so the address is already the one a browser
/// can reach: no readiness wait, and no dependency on a worker having booted.
fn open_relay_admin(addr: SocketAddr, context: LaunchContext) -> String {
    let url = local_ui_url(addr);
    open_admin_ui(context, addr, &SystemBrowserLauncher);
    url
}

/// Report the worker admin address once the worker adopts the reserved socket.
///
/// `relay_admin_url` names the page a host that runs a relay actually opens, and
/// `open` is set only for the role that owns no relay. The report itself is the
/// same either way: the worker publishes the address it serves during its own
/// startup, and nothing here waits on it.
fn report_worker_admin_ready(
    startup: IntegratedStartup,
    relay_admin_url: Option<String>,
    open: Option<LaunchContext>,
) {
    tokio::spawn(async move {
        match startup.wait_bound(ADMIN_READY_TIMEOUT).await {
            Some(addr) => {
                info!(admin_addr = %addr, ui_url = %local_ui_url(addr), ?relay_admin_url, "worker admin listener ready");
                if let Some(context) = open {
                    open_admin_ui(context, addr, &SystemBrowserLauncher);
                }
            }
            None => warn!(
                timeout_seconds = ADMIN_READY_TIMEOUT.as_secs(),
                "worker admin listener did not report a bound address yet; the running components keep serving"
            ),
        }
    });
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
