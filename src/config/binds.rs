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
    Explicit,
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
    /// A reserved address without a socket, for pure derivation tests.
    #[cfg(test)]
    pub(crate) fn for_test(addr: SocketAddr) -> Self {
        Self {
            field: "test",
            addr,
            kind: BindKind::Explicit,
            listener: None,
        }
    }

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
    use super::{local_ui_url, loopback_reachable};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    /// Free loopback port, for tests that need a concrete address to bind.
    ///
    /// Every case here binds a real socket, so a failure of this probe is an
    /// exhausted local resource rather than a bind-derivation defect.

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
