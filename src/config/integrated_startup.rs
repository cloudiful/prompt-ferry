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

    /// A hand-off that is not registered process-wide.
    #[cfg(test)]
    fn detached() -> Self {
        let (bound, _rx) = watch::channel(None);
        Self { bound }
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

/// Remove the installed hand-off.
///
/// Tests use this to reach the inert behaviour a standalone command sees; it is
/// not part of the production startup path.
#[cfg(test)]
pub fn clear() {
    registry().take();
}

#[cfg(test)]
mod tests {
    use super::{
        IntegratedStartup, clear, is_installed, publish_bound, take_reserved_admin_listener,
    };
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
        sync::{Mutex, MutexGuard, OnceLock},
        time::Duration,
    };

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn reserve() -> TcpListener {
        TcpListener::bind(("127.0.0.1", 0)).expect("reserve a free port")
    }

    /// The registry is process-wide, so the tests that reach it run one at a
    /// time and leave it empty again.
    fn registry_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[tokio::test]
    async fn resolves_with_the_address_a_listener_publishes() {
        let startup = IntegratedStartup::detached();
        let waiter = tokio::spawn({
            let startup = startup.clone();
            async move { startup.wait_bound(Duration::from_secs(5)).await }
        });

        startup.publish(addr(8123));

        assert_eq!(waiter.await.expect("waiter joined"), Some(addr(8123)));
    }

    #[tokio::test]
    async fn resolves_even_when_the_address_arrives_before_the_wait() {
        let startup = IntegratedStartup::detached();
        startup.publish(addr(8126));

        assert_eq!(
            startup.wait_bound(Duration::from_secs(5)).await,
            Some(addr(8126)),
            "a late waiter must still observe the bound address"
        );
    }

    #[tokio::test]
    async fn a_listener_that_never_publishes_times_out_without_failing() {
        let startup = IntegratedStartup::detached();

        let observed = startup.wait_bound(Duration::from_millis(20)).await;

        assert_eq!(
            observed, None,
            "readiness must degrade, never block forever"
        );
    }

    #[test]
    fn the_reserved_admin_socket_is_handed_to_the_worker() {
        let _guard = registry_lock();
        clear();
        let reserved = reserve();
        let expected = reserved.local_addr().expect("reserved address");

        IntegratedStartup::install(Some(reserved));

        assert!(is_installed());
        let adopted = take_reserved_admin_listener().expect("the worker adopts the socket");
        assert_eq!(adopted.local_addr().expect("adopted address"), expected);
        clear();
    }

    #[test]
    fn a_reserved_socket_can_only_be_taken_once() {
        let _guard = registry_lock();
        clear();
        IntegratedStartup::install(Some(reserve()));

        assert!(take_reserved_admin_listener().is_some());
        assert!(
            take_reserved_admin_listener().is_none(),
            "the reservation must not be adopted twice"
        );
        clear();
    }

    #[test]
    fn installing_without_a_reservation_still_yields_no_socket() {
        let _guard = registry_lock();
        clear();

        IntegratedStartup::install(None);

        assert!(is_installed());
        assert!(
            take_reserved_admin_listener().is_none(),
            "the worker then binds the resolved address from its own configuration"
        );
        clear();
    }

    #[test]
    fn a_standalone_worker_sees_no_handoff_at_all() {
        let _guard = registry_lock();
        clear();

        assert!(!is_installed());
        assert!(take_reserved_admin_listener().is_none());
        clear();
    }

    #[tokio::test]
    async fn publishing_without_a_handoff_never_reaches_a_later_installer() {
        let startup = {
            let _guard = registry_lock();
            clear();
            publish_bound(addr(8125));
            IntegratedStartup::install(None)
        };

        assert_eq!(
            startup.wait_bound(Duration::from_millis(20)).await,
            None,
            "a standalone command must not inherit a foreign address"
        );
        clear();
    }

    #[tokio::test]
    async fn installing_again_does_not_expose_a_stale_address() {
        let (first, second) = {
            let _guard = registry_lock();
            clear();
            let first = IntegratedStartup::install(None);
            publish_bound(addr(8124));
            let second = IntegratedStartup::install(None);
            (first, second)
        };

        assert!(
            second.wait_bound(Duration::from_millis(20)).await.is_none(),
            "a fresh hand-off must start empty"
        );
        assert_eq!(
            first.wait_bound(Duration::from_millis(20)).await,
            Some(addr(8124)),
            "the earlier handle still sees what it published"
        );
        clear();
    }

    #[tokio::test]
    async fn the_installed_handoff_receives_the_listener_address() {
        let startup = {
            let _guard = registry_lock();
            clear();
            IntegratedStartup::install(Some(reserve()))
        };

        publish_bound(addr(8127));

        assert_eq!(
            startup.wait_bound(Duration::from_secs(5)).await,
            Some(addr(8127))
        );
        clear();
    }
}
