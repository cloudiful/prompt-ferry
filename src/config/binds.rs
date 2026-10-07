//! Listener address derivation for the integrated entrypoint.
//!
//! An integrated process owns three listeners: the relay public bind, the
//! loopback-only worker bridge between relay and worker, and the admin
//! listener that serves the UI and API. This module turns a configured bind
//! string into a concrete, already-reserved address before anything starts,
//! so an operator learns about a port collision immediately and every
//! consumer (the derived worker relay URL, the admin readiness signal) can
//! address the listener that will really exist.
//!
//! A configured port of `0` — or an empty bind string — means *automatic*:
//! a free loopback port is selected. Any other port is explicit, is bound
//! exactly as written, and fails loudly when it is already taken.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};

use anyhow::{Context, anyhow};

/// How a listener address was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindKind {
    /// Written out in configuration; bound verbatim.
    Explicit,
    /// No usable port was configured, so a free loopback port was selected.
    Automatic,
}

/// One reserved listener address.
///
/// The socket is held from reservation until the component that serves on it
/// takes it over, which closes the window where another process could claim
/// the port between selection and bind.
///
/// The socket is never released back to the OS: a listener whose owner binds
/// it much later, after slow work such as a database migration, would
/// otherwise lose a port that something else may have taken in between.
#[derive(Debug)]
pub struct ReservedBind {
    field: &'static str,
    addr: SocketAddr,
    kind: BindKind,
    listener: Option<TcpListener>,
}

impl ReservedBind {
    /// The address that will really be served on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn kind(&self) -> BindKind {
        self.kind
    }

    /// Hand the reserved socket to the component that serves on it.
    pub fn into_listener(self) -> anyhow::Result<TcpListener> {
        self.listener.ok_or_else(|| {
            anyhow!(
                "`{}` has no reserved socket because it was already handed over",
                self.field
            )
        })
    }
}

/// Resolve a configured bind into a reserved socket.
///
/// `field` names the configuration key in errors so an operator can find it.
/// `require_loopback` rejects a non-loopback address, and pins an automatic
/// selection to `127.0.0.1` so an automatic port is never published off-host.
pub fn reserve_bind(
    bind: &str,
    field: &'static str,
    require_loopback: bool,
) -> anyhow::Result<ReservedBind> {
    let trimmed = bind.trim();
    let requested = if trimmed.is_empty() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    } else {
        trimmed.parse().with_context(|| {
            format!(
                "invalid `{field}` bind address `{trimmed}`; expected `host:port`, for example \
                 `127.0.0.1:8787`, or port `0` to let prompt-ferry pick a free port"
            )
        })?
    };

    let automatic = requested.port() == 0;
    let ip = if automatic && !requested.ip().is_loopback() {
        // An automatic port on a wildcard host would publish an internal
        // listener on every interface; pin it to loopback instead.
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        requested.ip()
    };
    if require_loopback && !ip.is_loopback() {
        return Err(anyhow!(
            "`{field}` bind must use a loopback address, got `{requested}`"
        ));
    }

    let target = SocketAddr::new(ip, requested.port());
    let listener = TcpListener::bind(target).with_context(|| {
        format!(
            "failed to bind `{field}` to `{target}`; choose a free port or set port `0` to let \
             prompt-ferry pick one"
        )
    })?;
    let addr = listener
        .local_addr()
        .with_context(|| format!("failed to read the address reserved for `{field}`"))?;

    Ok(ReservedBind {
        field,
        addr,
        kind: if automatic {
            BindKind::Automatic
        } else {
            BindKind::Explicit
        },
        listener: Some(listener),
    })
}

/// Build the worker relay URL that points at the reserved bridge listener.
///
/// The bridge is bound by the relay in this same process, so the derived URL
/// addresses the listener that will really exist rather than a configured
/// guess.
pub fn worker_bridge_url(bridge: &ReservedBind) -> String {
    format!("ws://{}/ws/worker", url_authority(bridge.addr()))
}

/// Build the local UI URL for a reserved admin address.
///
/// A wildcard admin bind is reached through loopback locally, which is what a
/// browser on the same machine must open.
pub fn local_ui_url(admin: SocketAddr) -> String {
    format!("http://{}/", url_authority(loopback_reachable(admin)))
}

/// Map a wildcard bind to the loopback address that reaches it locally.
pub fn loopback_reachable(addr: SocketAddr) -> SocketAddr {
    if addr.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), addr.port())
    } else {
        addr
    }
}

/// Render a `host:port` authority, bracketing an IPv6 literal.
fn url_authority(addr: SocketAddr) -> String {
    if addr.is_ipv6() {
        format!("[{}]:{}", addr.ip(), addr.port())
    } else {
        addr.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{BindKind, local_ui_url, loopback_reachable, reserve_bind, worker_bridge_url};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    /// Free loopback port, for tests that need a concrete address to bind.
    ///
    /// Every case here binds a real socket, so a failure of this probe is an
    /// exhausted local resource rather than a bind-derivation defect.
    fn free_loopback_port() -> String {
        let reserved =
            reserve_bind("127.0.0.1:0", "test", false).expect("probe a free loopback port");
        let addr = reserved.addr();
        drop(reserved.into_listener().expect("release the probed socket"));
        addr.to_string()
    }

    #[test]
    fn explicit_bind_is_reserved_on_the_configured_address() {
        let port = free_loopback_port();
        let reserved = reserve_bind(&port, "relay.bind", false)
            .expect("reserve the configured explicit address");

        assert_eq!(reserved.kind(), BindKind::Explicit);
        assert_eq!(reserved.addr().to_string(), port);
        assert!(
            reserved.into_listener().is_ok(),
            "an explicit bind holds its socket"
        );
    }

    #[test]
    fn automatic_bind_selects_a_free_loopback_port() {
        let reserved = reserve_bind("127.0.0.1:0", "serve.internal_worker_bind", true)
            .expect("reserve an automatic loopback port");
        assert_eq!(reserved.kind(), BindKind::Automatic);
        assert_eq!(reserved.addr().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_ne!(reserved.addr().port(), 0, "the port must be resolved");
    }

    #[test]
    fn empty_bind_is_automatic_and_loopback() {
        let reserved = reserve_bind("   ", "serve.internal_worker_bind", true)
            .expect("reserve an automatic loopback port for an empty bind");
        assert_eq!(reserved.kind(), BindKind::Automatic);
        assert_eq!(reserved.addr().ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn automatic_bind_on_a_wildcard_host_stays_on_loopback() {
        let reserved = reserve_bind("0.0.0.0:0", "relay.bind", true)
            .expect("reserve an automatic loopback port for a wildcard host");
        assert_eq!(reserved.kind(), BindKind::Automatic);
        assert_eq!(
            reserved.addr().ip(),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            "an automatic port must never be published off-host"
        );
    }

    #[test]
    fn automatic_binds_do_not_collide_with_each_other() {
        let first = reserve_bind("127.0.0.1:0", "relay.bind", false).expect("first");
        let second = reserve_bind("127.0.0.1:0", "worker.admin_bind", false).expect("second");
        assert_ne!(first.addr().port(), second.addr().port());
    }

    #[test]
    fn loopback_only_bind_rejects_a_non_loopback_address() {
        let error = reserve_bind("0.0.0.0:8788", "serve.internal_worker_bind", true)
            .expect_err("the worker bridge must stay loopback-only");
        assert!(error.to_string().contains("loopback"));
        assert!(error.to_string().contains("serve.internal_worker_bind"));
    }

    #[test]
    fn public_bind_allows_a_non_loopback_address() {
        let reserved = reserve_bind("0.0.0.0:0", "relay.bind", false).expect("public bind");
        assert!(reserved.addr().port() > 0);
    }

    #[test]
    fn malformed_bind_names_the_field_and_the_expected_shape() {
        let error = reserve_bind("not-an-address", "relay.bind", false).expect_err("malformed");
        let message = error.to_string();
        assert!(message.contains("relay.bind"), "message was: {message}");
        assert!(message.contains("host:port"), "message was: {message}");
    }

    #[test]
    fn occupied_explicit_bind_fails_and_points_at_port_zero() {
        let held = reserve_bind("127.0.0.1:0", "relay.bind", false).expect("hold a port");
        let occupied = held.addr().to_string();

        let error = reserve_bind(&occupied, "relay.bind", false).expect_err("port is taken");
        let message = format!("{error:#}");
        assert!(message.contains("relay.bind"), "message was: {message}");
        assert!(
            message.contains("port `0`"),
            "a collision must point at automatic selection: {message}"
        );
    }

    #[test]
    fn a_reservation_keeps_its_port_until_the_socket_is_handed_over() {
        let reserved = reserve_bind("127.0.0.1:0", "worker.admin_bind", false).expect("reserve");
        let addr = reserved.addr();

        // The socket is still held, so nothing else can take the port while the
        // owner is still starting up.
        let collision = reserve_bind(&addr.to_string(), "worker.admin_bind", false)
            .expect_err("the reservation must hold its port");
        assert!(format!("{collision:#}").contains("worker.admin_bind"));

        // Handing the socket over is what frees the port again.
        drop(reserved.into_listener().expect("hand over the socket"));
        reserve_bind(&addr.to_string(), "worker.admin_bind", false)
            .expect("the port is free once the socket is dropped");
    }

    #[test]
    fn an_automatic_admin_reservation_knows_its_own_url_up_front() {
        // Holding the socket means the resolved address is the real one, so a
        // client can be told the URL before the owner finishes starting.
        let reserved = reserve_bind("127.0.0.1:0", "worker.admin_bind", false).expect("reserve");

        assert_eq!(reserved.kind(), BindKind::Automatic);
        assert_eq!(
            local_ui_url(reserved.addr()),
            format!("http://{}/", reserved.addr())
        );
    }

    #[test]
    fn an_empty_or_wildcard_automatic_admin_bind_resolves_to_a_concrete_address() {
        for configured in ["", "   ", "0.0.0.0:0", "127.0.0.1:0"] {
            let reserved =
                reserve_bind(configured, "worker.admin_bind", false).expect("resolve automatic");

            assert_eq!(
                reserved.kind(),
                BindKind::Automatic,
                "configured `{configured}` should be automatic"
            );
            assert_eq!(
                reserved.addr().ip(),
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                "configured `{configured}` must resolve to loopback"
            );
            assert_ne!(reserved.addr().port(), 0, "the port must be resolved");
        }
    }

    #[test]
    fn an_explicit_admin_bind_is_propagated_unchanged() {
        let port = free_loopback_port();
        let reserved = reserve_bind(&port, "worker.admin_bind", false).expect("reserve");

        assert_eq!(reserved.kind(), BindKind::Explicit);
        assert_eq!(reserved.addr().to_string(), port);
    }

    #[test]
    fn derived_worker_url_points_at_the_reserved_bridge_listener() {
        let bridge = reserve_bind("127.0.0.1:0", "serve.internal_worker_bind", true)
            .expect("reserve bridge");

        assert_eq!(
            worker_bridge_url(&bridge),
            format!("ws://{}/ws/worker", bridge.addr())
        );
    }

    #[test]
    fn local_ui_url_targets_loopback_for_a_wildcard_admin_bind() {
        assert_eq!(
            local_ui_url(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8789)),
            "http://127.0.0.1:8789/"
        );
        assert_eq!(
            local_ui_url(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8789)),
            "http://127.0.0.1:8789/"
        );
    }

    #[test]
    fn loopback_reachable_leaves_a_concrete_address_alone() {
        let concrete = SocketAddr::new("10.0.0.5".parse().unwrap(), 8789);
        assert_eq!(loopback_reachable(concrete), concrete);
    }
}
