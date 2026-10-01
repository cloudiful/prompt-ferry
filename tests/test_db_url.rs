//! Pure guards for the test-scoped PostgreSQL URL helpers.
//!
//! Every helper behind these assertions is pure: it builds a URL or a pool
//! handle pinned by an explicit `search_path` to a fresh isolated `pfy_test_*`
//! schema and never connects. These cases pin that isolation contract, so a
//! stray query fails on the missing schema instead of reaching a shared one.

#[allow(dead_code)]
#[path = "support/test_db_url.rs"]
mod test_db_url;

#[test]
fn generated_test_schema_names_stay_inside_the_test_namespace() {
    let name = test_db_url::new_test_schema_name();
    test_db_url::assert_test_schema(&name).expect("generated names must be valid test schemas");
    assert!(name.starts_with("pfy_test_"));
    assert!(test_db_url::assert_test_schema("public").is_err());
    assert!(test_db_url::assert_test_schema("prompt_ferry").is_err());
    assert!(test_db_url::assert_test_schema("pfy_test_short").is_err());
}

#[test]
fn scoped_and_lazy_test_urls_carry_an_explicit_isolated_search_path() {
    let schema = test_db_url::new_test_schema_name();
    let scoped =
        test_db_url::scoped_database_url("postgres://user:pass@db.example:5432/worker", &schema)
            .expect("scoped url");
    assert!(
        scoped.contains("search_path"),
        "scoped url must pin search_path: {scoped}"
    );
    assert!(
        scoped.contains(&schema),
        "scoped url must target the isolated schema: {scoped}"
    );
    assert!(test_db_url::scoped_database_url("postgres://user@host/db", "public").is_err());
    assert!(test_db_url::scoped_database_url("postgres://user@host/db", "prompt_ferry").is_err());

    let lazy = test_db_url::lazy_test_database_url();
    assert!(
        lazy.starts_with("postgres://"),
        "lazy url keeps a postgres scheme: {lazy}"
    );
    assert!(
        lazy.contains("search_path"),
        "lazy url must pin search_path: {lazy}"
    );
    assert!(
        !lazy.contains("public"),
        "lazy url must never reference the public schema: {lazy}"
    );
}
