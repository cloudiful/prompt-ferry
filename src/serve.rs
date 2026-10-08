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
    cli::ServeArgs,
    config::{
        AppConfig, HostRole, IntegratedStartup,
        binds::{ReservedBind, local_ui_url, reserve_bind},
        host_startup, integrated_bridge,
    },
    relay, worker,
};
use std::time::Duration;
use tracing::{info, warn};

/// How long the admin listener gets to report the address it bound. This bounds
/// a log line only: the running components keep serving either way, and a cold
/// database migration can legitimately take longer than this.
const ADMIN_READY_TIMEOUT: Duration = Duration::from_secs(60);

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
    // Each role's startup is boxed so this dispatch stays small: inlining all
    // three of them would make the whole process future oversized.
    match Startup::reserve(&app_config, &args, role)? {
        Startup::Integrated {
            public,
            worker_bridge,
            admin,
        } => {
            Box::pin(run_integrated(
                app_config,
                public,
                worker_bridge,
                admin,
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
        } => Box::pin(run_relay_only(app_config, public, worker_bridge)).await,
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
/// takes the admin port.
#[derive(Debug)]
enum Startup {
    /// Local relay with an embedded worker, bridged inside this process.
    Integrated {
        public: ReservedBind,
        worker_bridge: ReservedBind,
        admin: ReservedBind,
    },
    /// The worker alone, keeping the management entry point it serves itself.
    WorkerOnly { admin: ReservedBind },
    /// The relay alone, with the bridge remote workers dial.
    RelayOnly {
        public: ReservedBind,
        worker_bridge: ReservedBind,
    },
}

impl Startup {
    fn reserve(app_config: &AppConfig, args: &ServeArgs, role: HostRole) -> anyhow::Result<Self> {
        let admin = || reserve_bind(&app_config.worker.admin_bind, "worker.admin_bind", false);
        let public = || reserve_bind(&app_config.relay.bind, "relay.bind", false);
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
            }),
        }
    }
}

/// Start the relay, the embedded worker, and the management UI.
async fn run_integrated(
    app_config: AppConfig,
    public: ReservedBind,
    worker_bridge: ReservedBind,
    admin: ReservedBind,
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
        admin_ui_url = local_ui_url(admin.addr()),
        "integrated mode starting"
    );
    let (relay_config, worker_config) =
        integrated_bridge::bridge_connected_configs(app_config, &worker_bridge, &admin);

    // The admin socket stays reserved from here until the worker adopts it, so
    // a fixed admin port cannot be taken while the worker runs its database
    // bootstrap. Installing the hand-off also makes a failure to bind that
    // socket fail this run instead of leaving the process without its UI.
    let startup = IntegratedStartup::install(Some(admin.into_listener()?));
    report_admin_ready_when_bound(startup, entrypoint);

    // Every listener was reserved above, so the relay serves on exactly the
    // addresses the worker was told about.
    tokio::try_join!(
        relay::run_with_listeners(
            relay_config,
            public.into_listener()?,
            worker_bridge.into_listener()?,
        ),
        worker::run_embedded(worker_config),
    )?;
    Ok(())
}

/// Start the worker alone.
///
/// The worker keeps its own management entry point, so its admin listener is
/// reserved here and handed over the same way the integrated role hands it
/// over. Nothing else changes: this role dials remote relays, so its bridge
/// TLS material, client certificates, and payload encryption are the settings
/// it was configured with.
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
    report_admin_ready_when_bound(startup, entrypoint);
    worker::run_embedded(worker_config).await
}

/// Start the relay alone.
///
/// Remote workers connect to the relay's worker bind, which is the same
/// listener the integrated role bridges its embedded worker to. No worker runs
/// in this process, so the worker-owned admin listener is not started: this role
/// serves the relay's public API and its worker bridge, and nothing else.
async fn run_relay_only(
    app_config: AppConfig,
    public: ReservedBind,
    worker_bridge: ReservedBind,
) -> anyhow::Result<()> {
    let mut relay_config = app_config.relay;
    info!(
        role = HostRole::Relay.as_str(),
        public_bind = %public.addr(),
        public_bind_kind = ?public.kind(),
        worker_bind = %worker_bridge.addr(),
        worker_bind_kind = ?worker_bridge.kind(),
        "relay-only mode starting; no local worker is started"
    );
    // Both sockets are already bound, so the relay is told the addresses it
    // really serves on rather than the configured strings; a configured port of
    // `0` has resolved by now.
    relay_config.bind = public.addr().to_string();
    relay_config.worker_bind = worker_bridge.addr().to_string();
    relay::run_with_listeners(
        relay_config,
        public.into_listener()?,
        worker_bridge.into_listener()?,
    )
    .await
}

/// Report the admin address once the worker adopts the reserved socket, and
/// open it in the browser when this entrypoint does that.
///
/// The worker takes the socket over during its own startup, and the address it
/// serves is published there; this only reports it. A browser launch is
/// best-effort and never fails the run.
fn report_admin_ready_when_bound(startup: IntegratedStartup, entrypoint: Entrypoint) {
    let context = entrypoint.launch_context();
    tokio::spawn(async move {
        match startup.wait_bound(ADMIN_READY_TIMEOUT).await {
            Some(addr) => {
                info!(admin_addr = %addr, ui_url = %local_ui_url(addr), "worker admin listener ready");
                crate::browser::open_admin_ui(
                    context,
                    addr,
                    &crate::browser::SystemBrowserLauncher,
                );
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
