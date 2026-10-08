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

/// Forget the recorded role.
///
/// Tests use this to reach the subcommand behavior a component sees when no
/// role-driven entrypoint installed one.
#[cfg(test)]
pub fn clear() {
    registry().take();
}

fn registry() -> MutexGuard<'static, Option<HostRole>> {
    static REGISTRY: OnceLock<Mutex<Option<HostRole>>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::{HostRole, activate, active, clear};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The record is process-wide, so the tests that reach it run one at a time
    /// and leave it empty again.
    fn record_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn a_subcommand_startup_has_no_recorded_role() {
        let _guard = record_lock();
        clear();

        assert_eq!(active(), None);
    }

    #[test]
    fn the_recorded_role_is_the_one_the_entrypoint_resolved() {
        let _guard = record_lock();
        clear();

        activate(HostRole::Worker);

        assert_eq!(active(), Some(HostRole::Worker));
        clear();
    }

    #[test]
    fn recording_again_never_exposes_a_stale_role() {
        let _guard = record_lock();
        clear();

        activate(HostRole::Relay);
        activate(HostRole::Integrated);

        assert_eq!(
            active(),
            Some(HostRole::Integrated),
            "a restarted entrypoint must read its own role"
        );
        clear();
    }
}
