//! The relay's own management entry point.
//!
//! The relay serves the same frontend build the worker serves, and splits the
//! requests that reach it: the relay control plane answers locally, and
//! everything under the worker's API is forwarded over the bridge to whichever
//! worker is attached. That split is why a relay-only host still opens the page
//! and configures itself while no worker exists, and why an integrated host
//! keeps managing its worker through the relay.
//!
//! The listener is loopback-only and every non-static route is authenticated.
//! The relay's public listener is untouched: it keeps forwarding unknown paths
//! to the worker and never serves these routes.

pub(super) mod auth;
pub(super) mod control;
// The request and response bodies are part of the generated admin API contract, so
// the export tree reaches them from outside this module.
pub(crate) mod dto;
pub mod restart;
pub(crate) mod state;
mod worker_api;

#[cfg(test)]
mod tests;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{any, get, post, put},
};
use tokio::net::TcpListener;
use tracing::info;

use crate::{
    bridge_wire, relay::request_compression::capture_request_compression, web_assets_server,
};

use state::RelayAdminState;

/// An error in the shape the rest of the API uses.
fn dto_error(
    status: axum::http::StatusCode,
    code: &str,
    message: &str,
) -> axum::response::Response {
    crate::worker_admin::state::error(status, code, message)
}

/// Grace given to in-flight management requests after a shutdown is signalled.
const ADMIN_SHUTDOWN_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// The management listener's router.
///
/// The order is the contract. The routes that must answer a caller who has not
/// authenticated yet — liveness, the login that exchanges a token for a session,
/// the session probe the page uses to decide whether to ask for one — sit
/// outside the guard, because a guard they cannot pass would make
/// authenticating impossible. Everything else, including every worker request
/// proxied through this listener, is behind the guard. The SPA fallback is last,
/// so the page is what an unknown browser path resolves to.
pub fn router(state: RelayAdminState) -> Router {
    let open = Router::new()
        .route("/healthz", get(control::healthz))
        .route("/relay/auth/login", post(auth::login))
        .route("/relay/auth/logout", post(auth::logout))
        .route("/relay/auth/me", get(auth::me));

    let guarded = Router::new()
        .route("/relay/status", get(control::status))
        .route(
            "/relay/settings",
            get(control::settings).patch(control::update_settings),
        )
        .route("/relay/host", get(control::host))
        .route("/relay/host/role", put(control::set_role))
        .route("/relay/host/restart", post(control::request_restart))
        .fallback(worker_api::proxy_worker_admin)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_admin,
        ));

    // axum's `nest` matches `/api/v1` and `/api/v1/<path>` but not `/api/v1/`,
    // so that one spelling of the prefix would escape the API entirely and
    // resolve to the page. Naming it explicitly keeps every spelling of the
    // prefix behind the guard and routes it like any other API path: an unknown
    // one is the worker's to answer, exactly as it already is.
    let prefix = Router::new()
        .route("/api/v1/", any(worker_api::proxy_worker_admin))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_admin,
        ));

    Router::new()
        .merge(prefix)
        .nest("/api/v1", open.merge(guarded))
        .merge(web_assets_server::frontend_routes::<RelayAdminState>())
        .layer(middleware::from_fn(capture_request_compression))
        .layer(DefaultBodyLimit::max(
            bridge_wire::PUBLIC_API_BODY_LIMIT_BYTES,
        ))
        .with_state(state)
}

/// Serve the management listener on a socket the startup already reserved.
///
/// A restart request and an operating-system signal stop this listener the same
/// way, and both are capped by [`ADMIN_SHUTDOWN_BUDGET`] once they arrive: a
/// healthy host is never cut off, while a reload cannot hang on a keep-alive
/// connection.
pub async fn serve(state: RelayAdminState, listener: TcpListener) -> anyhow::Result<()> {
    let addr = listener.local_addr()?;
    info!(%addr, ui_url = %crate::config::binds::local_ui_url(addr), "relay management listening");
    let app = router(state);
    let serve = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<crate::relay::RemoteAddr>(),
    );
    tokio::select! {
        result = serve.with_graceful_shutdown(stop_signal()) => result.map_err(anyhow::Error::from),
        _ = drain_deadline() => {
            tracing::warn!(
                budget_seconds = ADMIN_SHUTDOWN_BUDGET.as_secs(),
                "relay management listener exceeded its drain budget; forcing stop"
            );
            Ok(())
        }
    }
}

/// Resolve once this host is asked to stop, and never before.
async fn stop_signal() {
    let requested = restart::subscribe();
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
            _ = restart::wait(requested) => {
                info!("relay management listener stopping for a requested restart");
            }
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await;
    }
}

/// Resolve [`ADMIN_SHUTDOWN_BUDGET`] after the stop signal fires, and never before.
async fn drain_deadline() {
    stop_signal().await;
    tokio::time::sleep(ADMIN_SHUTDOWN_BUDGET).await;
}
