//! Guard for the canonical PostgreSQL configuration entry point.
//!
//! `DATABASE_URL` is the single user-facing PostgreSQL variable, so no
//! allowlisted source may reintroduce a legacy `PROMPT_FERRY_*_DATABASE_URL`
//! key or hardcode a localhost test URL. The test-scoped URL and schema
//! namespace assertions live in `tests/test_db_url.rs`.

/// Guard: no allowlisted source may reintroduce a legacy DB URL variable or a
/// hardcoded localhost test URL now that `DATABASE_URL` is canonical.
#[test]
fn sources_never_hardcode_a_postgres_test_url_or_a_legacy_env_key() {
    const FILES: &[&str] = &[
        "crates/prompt-ferry-runtime-env/src/runtime_env.rs",
        "tools/db-init/src/main.rs",
        "src/config/mod.rs",
        "src/mcp/entry/tests.rs",
        "src/worker_admin/types.rs",
        "src/worker_admin/types/mcp.rs",
        "src/worker_admin/handlers/server.rs",
        "src/worker_admin/handlers/config_audit/tests/harness.rs",
        "src/worker_admin/handlers/config_export/tests/harness.rs",
        "src/worker_admin/handlers/config_import/tests/harness.rs",
        "src/worker/runtime/tests.rs",
        "src/db/config_repository/http_tests.rs",
        "tests/oauth_login.rs",
        "tests/chatgpt_routing.rs",
        "tests/support/endpoint_create_fixture.rs",
        "tests/support/db_harness.rs",
        "tests/support/test_db_url.rs",
    ];
    // Built at runtime so this guard file does not match its own needles.
    let needles = [
        ["PROMPT_FERRY_DEV", "DATABASE_URL"].concat(),
        ["PROMPT_FERRY_TEST", "DATABASE_URL"].concat(),
        ["PROMPT_FERRY_WORKER__", "DATABASE_URL"].concat(),
        ["postgres://postgres:postgres", "@localhost"].concat(),
        ["postgres://postgres:postgres", "@127.0.0.1"].concat(),
    ];
    for path in FILES {
        let source =
            std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"));
        for needle in &needles {
            assert!(
                !source.contains(needle.as_str()),
                "{path} must not reference {needle}"
            );
        }
    }
}
