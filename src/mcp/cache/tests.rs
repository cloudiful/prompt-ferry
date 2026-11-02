use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;

use super::lease::REFRESH_KEY_PREFIX;
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

#[tokio::test]
async fn refresh_inflight_is_deduplicated() {
    let cache = McpCatalogCache::new();
    let server = server();
    let ttl = Duration::from_secs(60);

    let first = cache
        .try_acquire_refresh_lease(server.server_id, ttl)
        .await
        .expect("first lease");
    assert!(
        cache
            .try_acquire_refresh_lease(server.server_id, ttl)
            .await
            .is_none(),
        "a second lease must not be handed out while the first one is held"
    );

    first.release().await;

    assert!(
        cache
            .try_acquire_refresh_lease(server.server_id, ttl)
            .await
            .is_some()
    );
}

#[tokio::test]
async fn an_expired_local_lease_is_recovered() {
    let cache = McpCatalogCache::new();
    let server = server();

    assert!(
        cache
            .try_acquire_refresh_lease(server.server_id, Duration::from_millis(1))
            .await
            .is_some()
    );
    assert!(
        cache
            .try_acquire_refresh_lease(server.server_id, Duration::from_millis(1))
            .await
            .is_none()
    );

    // The held lease expires on its own, so a cancelled refresh cannot wedge
    // the server forever.
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        cache
            .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
            .await
            .is_some()
    );
}

#[tokio::test]
async fn reviewer_expired_local_owner_cannot_release_successor() {
    let cache = McpCatalogCache::new();
    let server = server();
    let expired_owner = cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_millis(10))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let successor = cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .unwrap();
    expired_owner.release().await;
    let unexpected_owner = cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await;
    let successor_was_preserved = unexpected_owner.is_none();
    if let Some(lease) = unexpected_owner {
        lease.release().await;
    }
    successor.release().await;
    assert!(
        successor_was_preserved,
        "releasing an expired local owner erased the live successor lease"
    );
}

#[tokio::test]
async fn sqlite_catalog_cache_is_shared_between_cache_instances() {
    let path =
        std::env::temp_dir().join(format!("prompt-ferry-mcp-cache-{}.sqlite", Uuid::new_v4()));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let config = WorkerConfig::default();
    let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let second = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let server = server();
    let expected = snapshot("sqlite-cached");

    first.put(&server, expected.clone()).await;
    assert_eq!(second.raw_snapshot(&server).await, Some(expected));
    second.invalidate(server.server_id).await;
    assert_eq!(first.raw_snapshot(&server).await, None);
    pool.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn sqlite_refresh_leases_are_shared_between_cache_instances() {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-mcp-cache-lease-{}.sqlite",
        Uuid::new_v4()
    ));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let config = WorkerConfig::default();
    let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let second = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let server = server();

    let lease = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("first lease");
    assert!(
        second
            .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
            .await
            .is_none(),
        "another cache instance must not take the same refresh lease"
    );

    lease.release().await;
    assert!(
        second
            .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
            .await
            .is_some()
    );

    pool.close().await;
    let _ = std::fs::remove_file(path);
}

/// The dedicated Valkey test target, or `None` when it is not configured.
fn valkey_test_url() -> Option<String> {
    let Ok(url) = std::env::var("PROMPT_FERRY_TEST_VALKEY_URL") else {
        eprintln!("skipping Valkey MCP catalog test: PROMPT_FERRY_TEST_VALKEY_URL is not set");
        return None;
    };
    Some(url)
}

#[tokio::test]
async fn valkey_catalog_cache_is_shared_between_cache_instances() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 60,
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config(&config).await;
    let second = McpCatalogCache::from_config(&config).await;
    let server = server();
    let expected = snapshot("valkey-cached");

    first.put(&server, expected.clone()).await;
    assert_eq!(second.raw_snapshot(&server).await, Some(expected));

    second.invalidate(server.server_id).await;
    assert_eq!(first.raw_snapshot(&server).await, None);
}

#[tokio::test]
async fn valkey_catalog_entries_expire_with_the_retention_window() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 60,
        ..WorkerConfig::default()
    };
    let cache = McpCatalogCache::from_config(&config).await;
    let server = server();

    cache.put(&server, snapshot("valkey-cached")).await;

    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let ttl: i64 = manager
        .ttl(format!(
            "{MCP_CATALOG_VALKEY_KEY_PREFIX}{}",
            server.server_id
        ))
        .await
        .unwrap();
    assert!(
        ttl > 0 && ttl <= 60,
        "the stored catalog must expire with the retention window, got {ttl}s"
    );

    let _: usize = manager
        .del(format!(
            "{MCP_CATALOG_VALKEY_KEY_PREFIX}{}",
            server.server_id
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn valkey_refresh_leases_are_shared_and_released_token_safe() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config(&config).await;
    let second = McpCatalogCache::from_config(&config).await;
    let server = server();
    let key = format!("{REFRESH_KEY_PREFIX}{}", server.server_id);

    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = manager.del(&key).await.unwrap();

    let lease = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("first lease");
    assert!(
        second
            .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
            .await
            .is_none(),
        "another worker must not take the same refresh lease"
    );

    lease.release().await;
    assert!(
        second
            .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
            .await
            .is_some(),
        "the released lease must be available again"
    );

    let _: usize = manager.del(&key).await.unwrap();
}

#[tokio::test]
async fn sqlite_expired_owner_cannot_release_successor() {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-mcp-cache-cas-{}.sqlite",
        Uuid::new_v4()
    ));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let config = WorkerConfig::default();
    let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let second = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let server = server();

    let expired_owner = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(1))
        .await
        .expect("expired owner lease");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let successor = second
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("the successor takes over the expired lease");
    expired_owner.release().await;

    let unexpected = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await;
    let preserved = unexpected.is_none();
    if let Some(lease) = unexpected {
        lease.release().await;
    }
    successor.release().await;

    pool.close().await;
    let _ = std::fs::remove_file(path);
    assert!(
        preserved,
        "releasing an expired SQLite owner erased the live successor lease"
    );
}

#[tokio::test]
async fn valkey_expired_owner_cannot_release_successor() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config(&config).await;
    let second = McpCatalogCache::from_config(&config).await;
    let server = server();
    let key = format!("{REFRESH_KEY_PREFIX}{}", server.server_id);
    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = manager.del(&key).await.unwrap();

    let expired_owner = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(1))
        .await
        .expect("expired owner lease");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let successor = second
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("the successor takes over the expired lease");
    expired_owner.release().await;

    let unexpected = first
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await;
    let preserved = unexpected.is_none();
    if let Some(lease) = unexpected {
        lease.release().await;
    }
    successor.release().await;
    let _: usize = manager.del(&key).await.unwrap();
    assert!(
        preserved,
        "releasing an expired Valkey owner erased the live successor lease"
    );
}

#[tokio::test]
async fn reviewer_r4_cleanup_budget_retires_activity_without_claiming_release() {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-mcp-cleanup-budget-{}.sqlite",
        Uuid::new_v4()
    ));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let observer = crate::db::connect_sqlite(&path).await.unwrap();
    let cache =
        McpCatalogCache::from_config_with_sqlite(&WorkerConfig::default(), Some(pool.clone()))
            .await;
    let server = server();
    let coordinator = cache.refresh_coordinator().clone();
    let activity = coordinator.admit().unwrap();
    let lease = cache
        .try_acquire_tracked_refresh_lease(
            server.server_id,
            Duration::from_secs(60),
            Duration::from_millis(30),
            activity,
        )
        .await
        .unwrap();
    let mut held = Vec::new();
    for _ in 0..pool.options().get_max_connections() {
        held.push(pool.acquire().await.unwrap());
    }
    drop(lease);
    let tracked_before = coordinator.tracked_refreshes();
    coordinator
        .cancel_and_drain(Duration::from_millis(300))
        .await;
    let tracked_after = coordinator.tracked_refreshes();
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
    )
    .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
    .fetch_one(&observer)
    .await
    .unwrap();
    drop(held);
    pool.close().await;
    observer.close().await;
    let _ = std::fs::remove_file(&path);
    assert_eq!(tracked_before, 1);
    assert_eq!(tracked_after, 0, "cleanup timeout leaked its activity");
    assert_eq!(
        remaining, 1,
        "the blocked release should require expiry recovery"
    );
}

#[tokio::test]
async fn reviewer_r4_admin_reader_does_not_restart_the_acquisition_deadline() {
    #[allow(dead_code)]
    mod test_db_url {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/test_db_url.rs"
        ));
    }
    let cache = McpCatalogCache::new();
    let mut named = server();
    named.name = "named".to_string();
    let mut sibling = server();
    sibling.name = "sibling".to_string();
    cache.put(&named, snapshot("named_tool")).await;
    cache.put(&sibling, snapshot("sibling_tool")).await;
    let pool = test_db_url::lazy_test_pool();
    let service = crate::mcp::McpCatalogService::new(pool.clone(), cache.clone())
        .with_refresh_policy(crate::mcp::CatalogRefreshPolicy {
            service_ceiling: Duration::from_millis(500),
            ..crate::mcp::CatalogRefreshPolicy::production()
        });
    let first_blocker = cache.local.write().await;
    let outer_deadline = tokio::time::Instant::now() + Duration::from_millis(650);
    let mut read = {
        let service = service.clone();
        tokio::spawn(async move { service.catalog_for_server(&[named, sibling], "named").await })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let second_blocker = {
        let slots = cache.local.clone();
        tokio::spawn(async move {
            let _guard = slots.write().await;
            let _ = locked_tx.send(());
            let _ = release_rx.await;
        })
    };
    tokio::time::sleep(Duration::from_millis(280)).await;
    drop(first_blocker);
    tokio::time::timeout(Duration::from_millis(100), locked_rx)
        .await
        .unwrap()
        .unwrap();
    let result = tokio::time::timeout_at(outer_deadline, &mut read).await;
    let stopped_within_ceiling = result.is_ok();
    if !stopped_within_ceiling {
        read.abort();
        let _ = read.await;
    }
    let _ = release_tx.send(());
    second_blocker.await.unwrap();
    service.drain_refreshes(Duration::from_secs(1)).await;
    pool.close().await;
    assert!(
        stopped_within_ceiling,
        "a 500ms public catalog read was still pending after 650ms because sibling work restarted the deadline"
    );
}

#[tokio::test]
async fn reviewer_r3_peer_invalidation_is_not_undone_by_failure_fallback() {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-mcp-fallback-{}.sqlite",
        Uuid::new_v4()
    ));
    let peer_pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&peer_pool).await.unwrap();
    let reader_pool = crate::db::connect_sqlite(&path).await.unwrap();
    let config = WorkerConfig::default();
    let reader = McpCatalogCache::from_config_with_sqlite(&config, Some(reader_pool.clone())).await;
    let peer = McpCatalogCache::from_config_with_sqlite(&config, Some(peer_pool.clone())).await;
    let server = server();
    reader.put(&server, snapshot("invalidated")).await;
    peer.invalidate(server.server_id).await;
    reader_pool.close().await;
    let lookup = reader.lookup(&server).await;
    peer_pool.close().await;
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    assert!(
        !lookup.freshness.is_servable(),
        "a peer-invalidated snapshot became servable through backend-failure fallback: {:?}",
        lookup.freshness
    );
}

#[tokio::test]
async fn reviewer_r3_sqlite_expired_baseline_cannot_overwrite_restart() {
    let path =
        std::env::temp_dir().join(format!("prompt-ferry-mcp-expiry-{}.sqlite", Uuid::new_v4()));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let config = WorkerConfig {
        valkey_ttl_seconds: 1,
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let server = server();
    let old = first.baseline(&server).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let restarted = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let current = restarted.baseline(&server).await.unwrap();
    let old_won = first
        .publish_if_current(&server, &old.token, snapshot("old"), Utc::now())
        .await
        .unwrap();
    let new_won = restarted
        .publish_if_current(&server, &current.token, snapshot("new"), Utc::now())
        .await
        .unwrap();
    pool.close().await;
    let _ = std::fs::remove_file(&path);
    assert!(
        !old_won && new_won,
        "expired baseline overwrote restart or new baseline could not publish"
    );
}

#[tokio::test]
async fn reviewer_r3_valkey_expired_baseline_cannot_overwrite_restart() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 1,
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config(&config).await;
    let server = server();
    let old = first.baseline(&server).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let restarted = McpCatalogCache::from_config(&config).await;
    let current = restarted.baseline(&server).await.unwrap();
    let old_won = first
        .publish_if_current(&server, &old.token, snapshot("old"), Utc::now())
        .await
        .unwrap();
    let new_won = restarted
        .publish_if_current(&server, &current.token, snapshot("new"), Utc::now())
        .await
        .unwrap();
    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = manager.del(catalog_key(server.server_id)).await.unwrap();
    assert!(
        !old_won && new_won,
        "expired baseline overwrote restart or new baseline could not publish"
    );
}

#[tokio::test]
async fn sqlite_peer_invalidation_clears_and_fences_the_local_view() {
    let path = std::env::temp_dir().join(format!(
        "prompt-ferry-mcp-cache-fence-{}.sqlite",
        Uuid::new_v4()
    ));
    let pool = crate::db::connect_sqlite(&path).await.unwrap();
    crate::db::migrate_standalone(&pool).await.unwrap();
    let config = WorkerConfig::default();
    let first = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let second = McpCatalogCache::from_config_with_sqlite(&config, Some(pool.clone())).await;
    let server = server();
    first.put(&server, snapshot("old")).await;
    assert!(first.lookup(&server).await.snapshot.is_some());

    // A peer invalidates after this cache captured its baseline but before it
    // published.
    let baseline = first.baseline(&server).await.expect("baseline");
    second.invalidate(server.server_id).await;
    assert!(
        second.lookup(&server).await.snapshot.is_none(),
        "the tombstone must read as missing"
    );
    assert!(
        first.lookup(&server).await.snapshot.is_none(),
        "the peer tombstone must clear the stale local view"
    );
    let published = first
        .publish_if_current(&server, &baseline.token, snapshot("resurrect"), Utc::now())
        .await
        .expect("publish attempt");
    assert!(
        !published,
        "a stale baseline must fail the publication fence"
    );
    assert!(
        second.lookup(&server).await.snapshot.is_none(),
        "the fenced publication must not resurrect the catalog"
    );

    pool.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn valkey_peer_invalidation_clears_and_fences_the_local_view() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let config = WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 60,
        ..WorkerConfig::default()
    };
    let first = McpCatalogCache::from_config(&config).await;
    let second = McpCatalogCache::from_config(&config).await;
    let server = server();
    first.put(&server, snapshot("old")).await;
    assert!(first.lookup(&server).await.snapshot.is_some());

    let baseline = first.baseline(&server).await.expect("baseline");
    second.invalidate(server.server_id).await;
    assert!(
        second.lookup(&server).await.snapshot.is_none(),
        "the tombstone must read as missing"
    );
    assert!(
        first.lookup(&server).await.snapshot.is_none(),
        "the peer tombstone must clear the stale local view"
    );
    let published = first
        .publish_if_current(&server, &baseline.token, snapshot("resurrect"), Utc::now())
        .await
        .expect("publish attempt");
    assert!(
        !published,
        "a stale baseline must fail the publication fence"
    );
    assert!(
        second.lookup(&server).await.snapshot.is_none(),
        "the fenced publication must not resurrect the catalog"
    );

    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = manager
        .del(format!(
            "{MCP_CATALOG_VALKEY_KEY_PREFIX}{}",
            server.server_id
        ))
        .await
        .unwrap();
}

/// A TCP proxy in front of Valkey whose connections can be cut on demand.
async fn spawn_outage_proxy(
    target: std::net::SocketAddr,
) -> (std::net::SocketAddr, tokio::sync::watch::Sender<bool>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_tx, maybe_stop_rx) = tokio::sync::watch::channel(false);
    let mut stop_rx = maybe_stop_rx.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stop_rx.changed() => return,
                accepted = listener.accept() => {
                    let Ok((mut client, _)) = accepted else { return };
                    let Ok(mut upstream) = tokio::net::TcpStream::connect(target).await else {
                        return;
                    };
                    let mut connection_stop = maybe_stop_rx.clone();
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = connection_stop.changed() => {}
                            _ = tokio::io::copy_bidirectional(&mut client, &mut upstream) => {}
                        }
                    });
                }
            }
        }
    });
    (address, stop_tx)
}

fn valkey_socket_address(url: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let authority = url.split("://").nth(1)?.split('/').next()?;
    let host_port = authority.rsplit('@').next()?;
    host_port.to_socket_addrs().ok()?.next()
}

#[tokio::test]
async fn valkey_peer_invalidation_is_not_undone_by_a_store_outage() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let Some(target) = valkey_socket_address(&url) else {
        eprintln!("skipping Valkey outage test: the configured URL has no socket address");
        return;
    };
    let (proxy, stop) = spawn_outage_proxy(target).await;
    let through_proxy = WorkerConfig {
        valkey_url: format!("redis://{proxy}"),
        valkey_ttl_seconds: 60,
        ..WorkerConfig::default()
    };
    let direct = WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 60,
        ..WorkerConfig::default()
    };
    let reader = McpCatalogCache::from_config(&through_proxy).await;
    let peer = McpCatalogCache::from_config(&direct).await;
    let server = server();

    reader.put(&server, snapshot("invalidated")).await;
    peer.invalidate(server.server_id).await;
    let _ = stop.send(true);
    tokio::time::sleep(Duration::from_millis(50)).await;

    let lookup = tokio::time::timeout(Duration::from_secs(5), reader.lookup(&server))
        .await
        .expect("the unavailable backend read resolves");
    assert!(
        !lookup.freshness.is_servable(),
        "a peer-invalidated snapshot became servable after a Valkey outage: {:?}",
        lookup.freshness
    );

    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = manager.del(catalog_key(server.server_id)).await.unwrap();
}
