use anyhow::Error;
use tokio_tungstenite::tungstenite::Error as WsError;
use tracing::info;

use crate::usage::truncate_chars;

pub(crate) fn ws_connect_error_detail(err: &WsError) -> String {
    if let WsError::Http(response) = err {
        let status = response.status();
        let body = response
            .body()
            .as_deref()
            .map(|body| String::from_utf8_lossy(body).trim().to_string())
            .filter(|body| !body.is_empty());

        if let Some(body) = body {
            return format!("HTTP error: {status}: {}", truncate_chars(&body, 256));
        }
    }

    err.to_string()
}

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
/// worker keeps serving and connects as soon as an enabled relay appears. The
/// report is per transition, because the reconcilers run on a timer.
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
            "no enabled relay connection is available; the worker stays up and connects once one is \
             configured"
        );
    }
}

pub(crate) async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
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
        target.observe(false, "remote relay list");
        assert!(target.reported, "the first empty reconcile must report");
        target.observe(false, "remote relay list");
        assert!(target.reported, "a repeated empty reconcile stays reported");
        target.observe(true, "remote relay list");
        assert!(!target.reported, "a connected host has nothing to report");
        target.observe(false, "remote relay list");
        assert!(
            target.reported,
            "losing the connection again reports once more"
        );
    }
}
