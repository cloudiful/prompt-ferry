use anyhow::Error;
use tokio_tungstenite::tungstenite::Error as WsError;

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
    use super::format_error_chain;
    use anyhow::anyhow;

    #[test]
    fn formats_full_error_chain() {
        let err = anyhow!("unexpected EOF").context("websocket read failed");
        assert_eq!(
            format_error_chain(&err),
            "websocket read failed: unexpected EOF"
        );
    }
}
