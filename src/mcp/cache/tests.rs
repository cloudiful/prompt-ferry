use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;

use super::*;

fn server() -> McpServer {
    McpServer {
        server_id: Uuid::new_v4(),
        source_endpoint_id: None,
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "alpha".to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        provider_kind: None,
        url: Some("http://127.0.0.1:3000/mcp".to_string()),
        command: None,
        args: json!([]),
        env_json: json!({}),
        bearer_tokens_json: json!([]),
        http_headers_json: json!({}),
        auth_mode: "none".to_string(),
        basic_username: None,
        basic_password: None,
        proxy_url: None,
        has_proxy_url: false,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: json!([]),
        disabled_tools: json!([]),
        disabled_resources: json!([]),
        enabled: true,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
        lifecycle_learned_mode: None,
        lifecycle_learned_protocol_version: None,
        lifecycle_learned_for_updated_at: None,
        lifecycle_learned_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn snapshot(name: &str) -> ServerCatalogSnapshot {
    ServerCatalogSnapshot {
        tools: vec![json!({ "name": name })],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    }
}

fn policy() -> CatalogFreshnessPolicy {
    CatalogFreshnessPolicy::new(Duration::from_secs(300), Duration::from_secs(1800))
}

#[tokio::test]
async fn local_cache_reuses_snapshot_without_expiry() {
    let cache = McpCatalogCache::new();
    let server = server();
    let expected = snapshot("cached");

    cache.put(&server, expected.clone()).await;

    assert_eq!(cache.raw_snapshot(&server).await, Some(expected));
}

#[tokio::test]
async fn config_change_prevents_old_snapshot_reuse() {
    let cache = McpCatalogCache::new();
    let server = server();
    cache.put(&server, snapshot("cached")).await;

    let mut changed = server.clone();
    changed.updated_at += ChronoDuration::seconds(1);

    assert_eq!(cache.raw_snapshot(&changed).await, None);
    assert_eq!(
        cache.lookup(&changed).await.freshness,
        CatalogFreshness::Missing
    );
}

#[tokio::test]
async fn a_stored_snapshot_is_fresh_right_after_it_was_fetched() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    cache.put(&server, snapshot("cached")).await;

    let lookup = cache.lookup(&server).await;

    assert_eq!(lookup.freshness, CatalogFreshness::Fresh);
    assert!(lookup.snapshot.is_some());
}

#[tokio::test]
async fn a_snapshot_older_than_the_soft_window_is_stale() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;

    let lookup = cache.lookup(&server).await;

    assert_eq!(lookup.freshness, CatalogFreshness::Stale);
    assert!(lookup.snapshot.is_some());
}

#[tokio::test]
async fn a_snapshot_older_than_the_maximum_age_is_expired() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(1801),
        )
        .await;

    let lookup = cache.lookup(&server).await;

    assert_eq!(lookup.freshness, CatalogFreshness::Expired);
    assert!(lookup.snapshot.is_some());
}

#[tokio::test]
async fn a_legacy_payload_without_an_acquisition_timestamp_is_expired() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    let legacy = serde_json::json!({
        "updated_at": server.updated_at,
        "snapshot": {
            "tools": [{ "name": "legacy" }],
            "resources": [],
            "prompts": [],
        },
    })
    .to_string();

    let stored: StoredCatalog = serde_json::from_str(&legacy).expect("legacy payload");
    assert!(stored.fetched_at.is_none());
    assert!(cache.parse_stored(&legacy, &server).is_some());
}

#[tokio::test]
async fn reading_a_snapshot_never_marks_it_fresh() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    let fetched_at = Utc::now() - ChronoDuration::seconds(1200);
    cache.put_at(&server, snapshot("cached"), fetched_at).await;

    let first = cache.lookup(&server).await;
    let second = cache.lookup(&server).await;

    assert_eq!(first.freshness, CatalogFreshness::Stale);
    assert_eq!(second.freshness, CatalogFreshness::Stale);
    assert_eq!(first.fetched_at, second.fetched_at);
    assert_eq!(second.fetched_at, Some(fetched_at));
}

#[tokio::test]
async fn the_content_fingerprint_survives_a_metadata_only_refresh() {
    let cache = McpCatalogCache::with_freshness_policy(policy());
    let server = server();
    cache.put(&server, snapshot("cached")).await;
    let first = cache.lookup(&server).await;

    cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(1),
        )
        .await;
    let second = cache.lookup(&server).await;

    assert_eq!(first.content_hash, second.content_hash);
    assert_ne!(first.fetched_at, second.fetched_at);
}
