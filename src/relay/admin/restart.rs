//! A restart this host asked for through its own management page.
//!
//! A role change and a management bind change both name listeners and running
//! components, so nothing about them can be applied to a process that is already
//! serving. The control plane records the change and then asks for a restart
//! here; the startup that owns the components watches this signal and returns,
//! which is the same path a supervisor already uses to bring the process back.

use std::sync::OnceLock;

use tokio::sync::watch;

fn channel() -> &'static watch::Sender<bool> {
    static CHANNEL: OnceLock<watch::Sender<bool>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::channel(false).0)
}

/// Watch for a restart request.
///
/// A receiver taken before the request still observes it, because the signal is
/// a level rather than an edge.
pub fn subscribe() -> watch::Receiver<bool> {
    channel().subscribe()
}

/// Ask the running host to stop so it can come back with the saved settings.
pub fn request() {
    let _ = channel().send(true);
}

/// Wait until a restart is requested.
pub async fn wait(mut requested: watch::Receiver<bool>) {
    if *requested.borrow() {
        return;
    }
    let _ = requested.changed().await;
}

/// Whether a restart has been requested in this process.
pub fn requested() -> bool {
    *channel().borrow()
}
