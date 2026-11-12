//! Hand-off between the integrated entrypoint and the worker's admin listener.
//!
//! The admin listener is bound inside the worker runtime, long after the
//! integrated entrypoint has resolved its address and possibly before a
//! database migration finishes. This module carries the two things the worker
//! cannot work out for itself across that gap:
//!
//! * the **reserved socket**, so a fixed admin port chosen at startup cannot
//!   be taken by another process while the worker is still initializing, and
//! * the **address that is actually being served**, which a configured port of
//!   `0` only resolves once the socket exists.
//!
//! It also decides how a failure is reported. With a hand-off installed the
//! worker fails startup when the admin listener cannot be bound, because an
//! integrated process that cannot serve its UI is broken. Without one the
//! standalone `worker` command keeps its existing behaviour of logging the
//! failure and staying up.
//!
//! The registry is process-wide and inert until the integrated entrypoint
//! installs a hand-off, so the standalone `relay` and `worker` commands share
//! no state.

use std::{
    net::{SocketAddr, TcpListener},
    sync::{Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use tokio::sync::watch;

/// The startup side of the hand-off: waits for a listener to report the address
/// it bound.
#[derive(Debug, Clone)]
pub struct IntegratedStartup {
    bound: watch::Sender<Option<SocketAddr>>,
}

impl IntegratedStartup {
    /// Register this process's hand-off, taking over the reserved admin
    /// socket, and return the handle to wait on.
    ///
    /// Installing again replaces the previous hand-off, so a restart inside one
    /// process cannot observe a stale address. `admin_listener` is `None` only
    /// when the caller could not reserve one; the worker then binds the
    /// already-resolved address from its configuration instead.
    pub fn install(admin_listener: Option<TcpListener>) -> Self {
        let (bound, _rx) = watch::channel(None);
        let handle = Self { bound };
        *registry() = Some(State {
            bound: handle.clone(),
            admin_listener,
        });
        handle
    }

    /// Resolve with the address a listener published, or `None` once
    /// `timeout` elapses without one.
    ///
    /// A timeout is not an error: the listener may still be starting, and the
    /// process keeps serving either way.
    pub async fn wait_bound(&self, timeout: Duration) -> Option<SocketAddr> {
        let mut rx = self.bound.subscribe();
        let wait = async {
            loop {
                if let Some(addr) = *rx.borrow_and_update() {
                    return Some(addr);
                }
                if rx.changed().await.is_err() {
                    return None;
                }
            }
        };
        tokio::time::timeout(timeout, wait).await.ok().flatten()
    }

    fn publish(&self, addr: SocketAddr) {
        // `send` would drop the value while nobody is subscribed yet, which is
        // exactly the window between installing the hand-off and the listener
        // reporting its address.
        self.bound.send_replace(Some(addr));
    }
}

struct State {
    bound: IntegratedStartup,
    admin_listener: Option<TcpListener>,
}

/// Whether an integrated entrypoint installed a hand-off.
pub fn is_installed() -> bool {
    registry().is_some()
}

/// Take over the reserved admin socket.
///
/// `None` means either that no hand-off is installed (the standalone worker,
/// which binds its own socket) or that no socket was reserved and the worker
/// must bind the already-resolved address from its own configuration.
pub fn take_reserved_admin_listener() -> Option<TcpListener> {
    registry().as_mut()?.admin_listener.take()
}

/// Publish the address the admin listener is serving on.
///
/// A no-op unless the integrated entrypoint installed a hand-off, so a
/// standalone command pays nothing for this.
pub fn publish_bound(addr: SocketAddr) {
    if let Some(state) = registry().as_ref() {
        state.bound.publish(addr);
    }
}

fn registry() -> MutexGuard<'static, Option<State>> {
    static REGISTRY: OnceLock<Mutex<Option<State>>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
