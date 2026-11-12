//! The host role this process actually started as.
//!
//! The no-subcommand startup resolves the role once, before any component
//! runs, and records it here. Components that start inside the same process
//! read the record instead of taking the role as a parameter: the worker
//! runtime entrypoint is shared with the standalone `worker` command, which
//! brings its own connection targets and installs no role.
//!
//! No record means a subcommand startup, which keeps its existing behaviour.

use std::sync::{Mutex, MutexGuard, OnceLock};

use super::HostRole;

/// Record the role this process runs as.
///
/// Recording again replaces the previous role, so a restart inside one process
/// never observes a stale one.
pub fn activate(role: HostRole) {
    *registry() = Some(role);
}

/// The role this process runs as, or `None` for a subcommand startup.
pub fn active() -> Option<HostRole> {
    *registry()
}

fn registry() -> MutexGuard<'static, Option<HostRole>> {
    static REGISTRY: OnceLock<Mutex<Option<HostRole>>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
