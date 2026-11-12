//! Test-only helpers that never touch a running service.
//!
//! The URL below is a literal fixture. A test that needs a database URL to
//! validate configuration string handling uses it without dialling anything.

/// A database URL for configuration that is never dialled.
pub(crate) fn fixture_database_url() -> String {
    "postgres://prompt-ferry-test@127.0.0.1:1/prompt-ferry-test".to_string()
}
