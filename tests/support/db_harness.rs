// Shared test plumbing is pulled in by absolute path so this module works the
// same from an integration-test crate and from an in-crate `#[cfg(test)]`
// module. Every helper it re-exports is pure: it builds URLs and pool handles
// without connecting to a running server, or it validates the dedicated test
// target before anything connects at all.
#[allow(dead_code)]
pub mod test_db_url {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_db_url.rs"
    ));
}

#[allow(dead_code)]
pub mod test_pg_target {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/test_pg_target.rs"
    ));
}
