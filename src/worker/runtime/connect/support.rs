use anyhow::Error;
use tracing::info;

use crate::usage::truncate_chars;

pub(crate) fn is_expected_relay_disconnect(err: &Error) -> bool {
    err.chain().any(|cause| {
        let text = cause.to_string();
        text.contains("peer closed connection without sending TLS close_notify")
            || text.contains("unexpected EOF")
            || text.contains("Connection reset without closing handshake")
    })
}

pub(crate) fn format_error_chain(err: &Error) -> String {
    let parts = err
        .chain()
        .map(ToString::to_string)
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    truncate_chars(&parts.join(": "), 512)
}

/// Reports a host that currently has no relay to connect to.
///
/// Having no connection is not a startup failure and never changes the role: the
/// worker keeps serving and connects as soon as a relay target is configured.
/// The report is per transition, so a restart with no target logs it once.
#[derive(Debug, Default)]
pub(crate) struct MissingRelayTarget {
    reported: bool,
}

impl MissingRelayTarget {
    /// Record the connection state of this host, reporting a newly missing
    /// connection once.
    pub(crate) fn observe(&mut self, has_targets: bool, source: &'static str) {
        if has_targets {
            self.reported = false;
            return;
        }
        if std::mem::replace(&mut self.reported, true) {
            return;
        }
        info!(
            source,
            "no relay target is configured; the worker stays up and connects once \
             worker.relay_urls names one"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{MissingRelayTarget, format_error_chain};
    use anyhow::anyhow;

    #[test]
    fn formats_full_error_chain() {
        let err = anyhow!("unexpected EOF").context("websocket read failed");
        assert_eq!(
            format_error_chain(&err),
            "websocket read failed: unexpected EOF"
        );
    }

    #[test]
    fn a_missing_connection_is_reported_once_per_transition() {
        let mut target = MissingRelayTarget::default();

        // The report is a state machine over the connection state, so it is
        // checked through the flag it keeps: the first empty observation
        // reports, a repeated one does not, and a connection in between arms
        // the next report.
        target.observe(false, "local relay targets");
        assert!(target.reported, "the first empty reconcile must report");
        target.observe(false, "local relay targets");
        assert!(target.reported, "a repeated empty reconcile stays reported");
        target.observe(true, "local relay targets");
        assert!(!target.reported, "a connected host has nothing to report");
        target.observe(false, "local relay targets");
        assert!(
            target.reported,
            "losing the connection again reports once more"
        );
    }
}
