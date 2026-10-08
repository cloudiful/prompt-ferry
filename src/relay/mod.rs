pub mod admin;
mod public_proxy;
mod request_compression;
mod response_forward;
mod response_pump;
mod response_queue;
mod router;
mod state;
mod worker_bridge;
mod worker_proxy;

pub use router::*;
pub use state::{RelayHandle, RemoteAddr};

pub(crate) use state::AppState;

/// Build the relay state and its handle without serving anything.
///
/// The management listener needs both halves, and the tests need them without
/// binding a socket.
#[cfg(test)]
pub(crate) fn test_handle(config: crate::config::RelayConfig) -> (AppState, RelayHandle) {
    let inner = router::test_relay_state();
    let state = AppState {
        config,
        inner: inner.clone(),
    };
    (state, RelayHandle { inner })
}
