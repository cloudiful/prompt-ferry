//! The integrated entrypoint: relay, worker, and admin UI in one process.
//!
//! Both `prompt-ferry` with no subcommand and the `prompt-ferry serve`
//! compatibility alias land here, so there is exactly one integrated startup.
//! Every listener address is resolved and reserved before anything starts, the
//! worker's relay URL is derived from the bridge listener that will really
//! accept it, and the admin listener reports the address it bound.

use crate::{
    cli::ServeArgs,
    config::{
        AppConfig, BridgeEncryptionMode, IntegratedStartup, RelayConfig, ServeConfig, TlsMode,
        WorkerConfig, WorkerTlsMode,
        binds::{ReservedBind, local_ui_url, reserve_bind, worker_bridge_url},
    },
    relay, worker,
};
use std::time::Duration;
use tracing::{info, warn};

/// How long the admin listener gets to report the address it bound. This bounds
/// a log line only: the relay and the worker keep running either way, and a
/// cold database migration can legitimately take longer than this.
const ADMIN_READY_TIMEOUT: Duration = Duration::from_secs(60);

/// Which entrypoint reached the integrated startup.
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
    let binds = IntegratedBinds::reserve(&app_config, &args)?;
    let (relay_config, worker_config) = derive_configs(app_config, &binds);

    info!(
        public_bind = %binds.public.addr(),
        public_bind_kind = ?binds.public.kind(),
        internal_worker_bind = %binds.worker_bridge.addr(),
        internal_worker_bind_kind = ?binds.worker_bridge.kind(),
        admin_bind = %binds.admin.addr(),
        admin_bind_kind = ?binds.admin.kind(),
        admin_ui_url = local_ui_url(binds.admin.addr()),
        "integrated mode starting"
    );

    // The admin socket stays reserved from here until the worker adopts it, so
    // a fixed admin port cannot be taken while the worker runs its database
    // bootstrap. Installing the hand-off also makes a failure to bind that
    // socket fail this run instead of leaving the process without its UI.
    let readiness = IntegratedStartup::install(Some(binds.admin.into_listener()?));
    report_admin_ready_when_bound(readiness, entrypoint);

    // Every listener was reserved by `IntegratedBinds`, so the relay serves on
    // exactly the addresses the worker was told about.
    tokio::try_join!(
        relay::run_with_listeners(
            relay_config,
            binds.public.into_listener()?,
            binds.worker_bridge.into_listener()?,
        ),
        worker::run_embedded(worker_config),
    )?;
    Ok(())
}

/// The addresses this process serves on, reserved before startup.
#[derive(Debug)]
struct IntegratedBinds {
    /// Relay public API.
    public: ReservedBind,
    /// Relay/worker bridge inside this process; loopback-only by construction.
    worker_bridge: ReservedBind,
    /// Admin UI/API address. The worker adopts this socket during its own
    /// startup, and the bound address is reported back on the hand-off.
    admin: ReservedBind,
}

impl IntegratedBinds {
    fn reserve(app_config: &AppConfig, args: &ServeArgs) -> anyhow::Result<Self> {
        let serve_config = app_config.serve.clone().merge_args(args.clone());
        Ok(Self {
            public: reserve_bind(&app_config.relay.bind, "relay.bind", false)?,
            worker_bridge: reserve_bridge(&serve_config)?,
            admin: reserve_bind(&app_config.worker.admin_bind, "worker.admin_bind", false)?,
        })
    }
}

fn reserve_bridge(serve_config: &ServeConfig) -> anyhow::Result<ReservedBind> {
    reserve_bind(
        &serve_config.internal_worker_bind,
        "serve.internal_worker_bind",
        true,
    )
}

/// Derive the relay and worker configuration for integrated mode.
///
/// Every listener address is already reserved on `binds`, so this points the
/// worker at the bridge and hands it the resolved admin address, then applies
/// the in-process security overrides.
fn derive_configs(app_config: AppConfig, binds: &IntegratedBinds) -> (RelayConfig, WorkerConfig) {
    let mut relay_config = app_config.relay;
    let mut worker_config = app_config.worker;

    relay_config.worker_bind = binds.worker_bridge.addr().to_string();
    worker_config.relay_urls = vec![worker_bridge_url(&binds.worker_bridge)];
    // The admin socket is already reserved above, so the worker receives the
    // resolved address instead of the configured string. An empty value or a
    // port of `0` would otherwise be re-parsed and rebound by the admin
    // server, landing on a different port than the one this process reserved.
    worker_config.admin_bind = binds.admin.addr().to_string();

    // The worker-side token is authoritative in integrated mode: an explicitly
    // empty worker token fully opens integrated worker auth even when the relay
    // config carries its default or a custom token.
    let worker_token = worker_config.worker_token.trim().to_string();
    relay_config.worker_token = worker_token.clone();
    worker_config.worker_token = worker_token;

    // The bridge never leaves this process, so transport security and payload
    // encryption for it cannot apply and are switched off rather than silently
    // mismatched between the two halves.
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
                "worker admin listener did not report a bound address yet; the relay and worker keep running"
            ),
        }
    });
}

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
