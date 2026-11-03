use std::{path::PathBuf, sync::Arc, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::{
    db::{McpServer, McpServerInput},
    mcp::{
        CatalogFreshness, CatalogFreshnessPolicy, McpCatalogCache, ServerCatalogSnapshot, builtin,
    },
    relay_secrets::RelaySecretManager,
    standalone_config::StandaloneConfigStore,
};

fn freshness_policy() -> CatalogFreshnessPolicy {
    CatalogFreshnessPolicy::new(Duration::from_secs(300), Duration::from_secs(1800))
}

fn refresh_policy() -> CatalogRefreshPolicy {
    CatalogRefreshPolicy {
        service_ceiling: Duration::from_secs(2),
        follower_poll_interval: Duration::from_millis(20),
        lease_margin: Duration::from_secs(1),
        failure_backoff: Duration::from_millis(50),
        concurrency: 4,
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

/// A service harness over a real SQLite configuration store.
///
/// The repository is the only piece that needs a database: it answers the
/// configuration-generation recheck before a snapshot is published.
struct Harness {
    path: PathBuf,
    store: Arc<StandaloneConfigStore>,
    manager: RelaySecretManager,
    repository: crate::db::ConfigRepository,
    cache: McpCatalogCache,
}

impl Harness {
    async fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-mcp-service-{}.sqlite",
            Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([11_u8; 32])).expect("manager");
        let repository = crate::db::ConfigRepository::sqlite(store.clone(), manager.clone());
        let cache = McpCatalogCache::with_freshness_policy(freshness_policy());
        Self {
            path,
            store,
            manager,
            repository,
            cache,
        }
    }

    fn service(&self) -> McpCatalogService {
        McpCatalogService::new_with_repository(self.repository.clone(), self.cache.clone())
            .with_refresh_policy(refresh_policy())
    }

    async fn persist(&self, input: McpServerInput) -> McpServer {
        self.store
            .save_mcp_server(&self.manager, Uuid::new_v4(), &input, None)
            .await
            .expect("persist mcp server")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
    }
}

fn input(name: &str, transport: &str) -> McpServerInput {
    McpServerInput {
        scope: "admin".to_string(),
        owner_user_id: None,
        source_endpoint_id: None,
        name: name.to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: transport.to_string(),
        provider_kind: None,
        url: Some("http://127.0.0.1:1/mcp".to_string()),
        command: None,
        args: json!([]),
        env_json: json!({}),
        bearer_tokens_json: json!([]),
        http_headers_json: json!({}),
        auth_mode: "none".to_string(),
        basic_username: None,
        basic_password: None,
        proxy_url: None,
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: json!([]),
        disabled_tools: json!([]),
        disabled_resources: json!([]),
        enabled: true,
        timeout_ms: 1_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}

fn server(name: &str) -> McpServer {
    McpServer {
        server_id: Uuid::new_v4(),
        source_endpoint_id: None,
        scope: "admin".to_string(),
        owner_user_id: None,
        name: name.to_string(),
        aggregate_naming_mode: "qualified_only".to_string(),
        transport: "http".to_string(),
        provider_kind: None,
        url: Some("http://127.0.0.1:1/mcp".to_string()),
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
        timeout_ms: 200,
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

/// Wait until the cache holds the catalog the upstream published.
async fn wait_for_published_catalog(
    cache: &McpCatalogCache,
    server: &McpServer,
) -> ServerCatalogSnapshot {
    for _ in 0..400 {
        if let Some(snapshot) = cache.raw_snapshot(server).await
            && snapshot == builtin::catalog()
        {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the scheduled refresh did not publish a catalog");
}

#[tokio::test]
async fn a_fresh_catalog_is_served_without_an_upstream_call() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    harness.cache.put(&server, snapshot("cached_tool")).await;

    let acquired = harness
        .service()
        .acquire(&server)
        .await
        .expect("fresh catalog");

    assert_eq!(acquired.freshness, CatalogFreshness::Fresh);
    assert!(!acquired.refreshed);
    assert_eq!(acquired.snapshot.tools[0]["name"], "cached_tool");
}

#[tokio::test]
async fn reviewer_stale_aggregate_respects_refresh_concurrency() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut servers = Vec::new();
    for index in 0..5 {
        let mut config = input(&format!("reviewer-{index}"), "http");
        config.url = Some(format!("http://{address}/mcp"));
        let server = harness.persist(config).await;
        harness
            .cache
            .put_at(
                &server,
                snapshot("cached"),
                Utc::now() - ChronoDuration::seconds(600),
            )
            .await;
        servers.push(server);
    }
    let service = harness.service().with_refresh_policy(CatalogRefreshPolicy {
        concurrency: 2,
        ..refresh_policy()
    });
    service.aggregate_tools(&servers).await.unwrap();
    let mut connections = Vec::new();
    while let Ok(Ok((socket, _))) =
        tokio::time::timeout(Duration::from_millis(200), listener.accept()).await
    {
        connections.push(socket);
        if connections.len() == servers.len() {
            break;
        }
    }
    let active = connections.len();
    drop(connections);
    drop(listener);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    harness.store.pool().close().await;
    assert!(
        active <= 2,
        "configured concurrency is 2, but {active} upstream connections started"
    );
}

#[tokio::test]
async fn reviewer_forced_follower_does_not_succeed_with_pre_refresh_snapshot() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = input("alpha", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    harness.cache.put(&server, snapshot("before_refresh")).await;
    let service = harness.service();
    let owner = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.refresh_server(&server).await })
    };
    let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let follower_result = service.refresh_server(&server).await;
    let owner_result = owner.await.unwrap();
    drop(socket);
    drop(listener);
    harness.store.pool().close().await;
    assert!(owner_result.is_err(), "the unresponsive owner must fail");
    assert!(
        follower_result.is_err(),
        "forced follower claimed success with the old snapshot after the owner failed: {follower_result:?}"
    );
}

#[tokio::test]
async fn reviewer_r3_disabled_manual_refresh_obeys_deadline() {
    let harness = Harness::new().await;
    let mut config = input("disabled-deadline", "builtin_minimax");
    config.enabled = false;
    let server = harness.persist(config).await;
    let cache_pool = crate::db::connect_sqlite(&harness.path).await.unwrap();
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(cache_pool.clone()),
    )
    .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(CatalogRefreshPolicy {
            service_ceiling: Duration::from_millis(40),
            ..refresh_policy()
        });
    let mut held = Vec::new();
    for _ in 0..cache_pool.options().get_max_connections() {
        held.push(cache_pool.acquire().await.unwrap());
    }
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        service.refresh_server_by_id(server.server_id),
    )
    .await;
    drop(held);
    service.drain_refreshes(Duration::from_secs(1)).await;
    cache_pool.close().await;
    harness.store.pool().close().await;
    assert!(
        matches!(result, Ok(Err(_))),
        "disabled refresh bypassed its 40ms deadline and remained pending after 200ms: {result:?}"
    );
}

#[tokio::test]
async fn reviewer_r3_aborted_owner_cleanup_is_included_in_drain() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = input("aborted-owner", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    let cache_pool = crate::db::connect_sqlite(&harness.path).await.unwrap();
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(cache_pool.clone()),
    )
    .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(refresh_policy());
    let owner = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut held = Vec::new();
    for _ in 0..cache_pool.options().get_max_connections() {
        held.push(cache_pool.acquire().await.unwrap());
    }
    owner.abort();
    let _ = owner.await;
    service.drain_refreshes(Duration::from_millis(100)).await;
    let tracked = service.cache().refresh_coordinator().tracked_refreshes();
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
    )
    .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
    .fetch_one(harness.store.pool())
    .await
    .unwrap();
    drop(held);
    for _ in 0..100 {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
        )
        .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
        if count == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(socket);
    drop(listener);
    cache_pool.close().await;
    harness.store.pool().close().await;
    assert!(
        remaining == 0 || tracked > 0,
        "drain reported zero activities while an aborted owner still had an untracked lease cleanup"
    );
}

#[tokio::test]
async fn a_cold_catalog_is_loaded_on_demand() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;

    let acquired = harness
        .service()
        .acquire(&server)
        .await
        .expect("recovered catalog");

    assert!(acquired.refreshed);
    assert_eq!(acquired.freshness, CatalogFreshness::Fresh);
    assert_eq!(acquired.snapshot, builtin::catalog());
}

#[tokio::test]
async fn a_stale_catalog_is_served_while_one_refresh_runs() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached_tool"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;

    let acquired = harness
        .service()
        .acquire(&server)
        .await
        .expect("stale catalog");

    assert_eq!(acquired.freshness, CatalogFreshness::Stale);
    assert!(!acquired.refreshed);
    assert_eq!(acquired.snapshot.tools[0]["name"], "cached_tool");

    let published = wait_for_published_catalog(&harness.cache, &server).await;
    assert_eq!(published, builtin::catalog());
}

#[tokio::test]
async fn an_expired_catalog_is_refreshed_before_it_is_served() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached_tool"),
            Utc::now() - ChronoDuration::seconds(1801),
        )
        .await;

    let acquired = harness
        .service()
        .acquire(&server)
        .await
        .expect("recovered catalog");

    assert!(acquired.refreshed);
    assert_eq!(acquired.freshness, CatalogFreshness::Fresh);
    assert_eq!(acquired.snapshot, builtin::catalog());
}

#[tokio::test]
async fn a_failed_refresh_never_serves_an_expired_catalog() {
    let harness = Harness::new().await;
    // A stdio server without a command cannot be reached.
    let server = harness.persist(input("alpha", "stdio")).await;
    let fetched_at = Utc::now() - ChronoDuration::seconds(1801);
    harness
        .cache
        .put_at(&server, snapshot("cached_tool"), fetched_at)
        .await;

    let error = harness
        .service()
        .acquire(&server)
        .await
        .expect_err("an expired catalog must not be served");
    assert!(
        error.to_string().contains("catalog is not ready"),
        "unexpected error: {error}"
    );

    // The old snapshot is retained for diagnostics and keeps its age.
    let lookup = harness.cache.lookup(&server).await;
    assert_eq!(lookup.freshness, CatalogFreshness::Expired);
    assert_eq!(lookup.fetched_at, Some(fetched_at));
    assert_eq!(
        lookup.snapshot.expect("retained snapshot").tools[0]["name"],
        "cached_tool"
    );
}

#[tokio::test]
async fn a_read_waits_for_the_refresh_another_worker_owns() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    let service = harness.service();
    let lease = harness
        .cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("another worker owns the refresh");

    let publisher = {
        let cache = harness.cache.clone();
        let server = server.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            cache.put(&server, snapshot("published")).await;
        })
    };

    let acquired = service.acquire(&server).await.expect("followed refresh");

    assert_eq!(acquired.snapshot.tools[0]["name"], "published");
    publisher.await.expect("publisher");
    lease.release().await;
}

#[tokio::test]
async fn a_read_fails_when_the_owned_refresh_never_publishes() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    let service = harness.service().with_refresh_policy(CatalogRefreshPolicy {
        service_ceiling: Duration::from_millis(200),
        ..refresh_policy()
    });
    let lease = harness
        .cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .expect("another worker owns the refresh");

    let error = service
        .acquire(&server)
        .await
        .expect_err("a missing catalog must not be invented");
    assert!(
        error.to_string().contains("catalog is not ready"),
        "unexpected error: {error}"
    );

    lease.release().await;
}

#[tokio::test]
async fn a_snapshot_of_a_superseded_configuration_is_not_published() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    let mut superseded = server.clone();
    superseded.updated_at -= ChronoDuration::seconds(1);

    let error = harness
        .service()
        .acquire(&superseded)
        .await
        .expect_err("a superseded configuration must not be published");
    assert!(
        error.to_string().contains("configuration changed"),
        "unexpected error: {error}"
    );
    assert!(harness.cache.raw_snapshot(&server).await.is_none());
}

#[tokio::test]
async fn the_periodic_pass_skips_fresh_catalogs_and_recovers_stale_ones() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;
    let service = harness.service();

    harness.cache.put(&server, snapshot("cached_tool")).await;
    service.refresh_aging_entries().await;
    assert_eq!(
        harness
            .cache
            .raw_snapshot(&server)
            .await
            .expect("fresh snapshot")
            .tools[0]["name"],
        "cached_tool"
    );

    harness
        .cache
        .put_at(
            &server,
            snapshot("cached_tool"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;
    service.refresh_aging_entries().await;
    assert_eq!(
        harness
            .cache
            .raw_snapshot(&server)
            .await
            .expect("refreshed snapshot"),
        builtin::catalog()
    );
}

#[tokio::test]
async fn a_failed_periodic_refresh_backs_off_until_the_window_elapses() {
    let harness = Harness::new().await;
    // A stdio server without a command cannot be reached.
    let server = harness.persist(input("alpha", "stdio")).await;
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached_tool"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;
    let service = harness.service();

    service.refresh_aging_entries().await;
    assert!(
        harness
            .cache
            .refresh_backoff_active(server.server_id, server.updated_at)
            .await,
        "a failed background refresh must back off"
    );

    // A configuration change clears the backoff with the invalidated snapshot.
    harness.cache.invalidate(server.server_id).await;
    assert!(
        !harness
            .cache
            .refresh_backoff_active(server.server_id, server.updated_at)
            .await
    );
}

#[tokio::test]
async fn the_periodic_pass_never_cold_loads_a_missing_catalog() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;

    harness.service().refresh_aging_entries().await;

    assert!(harness.cache.raw_snapshot(&server).await.is_none());
}

#[tokio::test]
async fn the_warmup_loads_every_enabled_catalog() {
    let harness = Harness::new().await;
    let server = harness.persist(input("alpha", "builtin_minimax")).await;

    harness.service().warm_enabled_servers().await;

    assert_eq!(
        harness
            .cache
            .raw_snapshot(&server)
            .await
            .expect("warmed snapshot"),
        builtin::catalog()
    );
}

#[tokio::test]
async fn aggregate_reads_cached_servers_only() {
    let harness = Harness::new().await;
    let cached = server("alpha");
    let missing = server("beta");
    harness.cache.put(&cached, snapshot("cached_tool")).await;

    let tools = harness
        .service()
        .aggregate_tools(&[cached, missing])
        .await
        .unwrap();

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "alpha__cached_tool");
}

#[tokio::test]
async fn aggregate_normalizes_union_output_schema_without_touching_cache() {
    let harness = Harness::new().await;
    let server_cfg = server("alpha");
    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({
                "name": "db_query",
                "description": "query",
                "inputSchema": {"type": "object"},
                "outputSchema": {"oneOf": [{"type": "object"}, {"type": "object"}]},
            }),
            json!({"name": "plain_tool"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    harness.cache.put(&server_cfg, snapshot).await;

    let tools = harness
        .service()
        .aggregate_tools(std::slice::from_ref(&server_cfg))
        .await
        .unwrap();

    let query = tools
        .iter()
        .find(|tool| tool["name"] == "alpha__db_query")
        .expect("db_query tool");
    assert_eq!(query["outputSchema"]["type"], "object");
    assert_eq!(query["outputSchema"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(query["description"], "query");
    let plain = tools
        .iter()
        .find(|tool| tool["name"] == "alpha__plain_tool")
        .expect("plain tool");
    assert!(plain.get("outputSchema").is_none());

    // McpCatalogCache keeps the raw upstream payload for admin catalog use.
    let cached = harness.cache.raw_snapshot(&server_cfg).await.unwrap();
    assert!(cached.tools[0]["outputSchema"].get("type").is_none());
}

#[tokio::test]
async fn server_catalog_requires_a_recovered_catalog() {
    let harness = Harness::new().await;
    let server = server("alpha");

    let error = harness
        .service()
        .catalog_for_server(&[server], "alpha")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("catalog is not ready"));
}

#[tokio::test]
async fn server_catalog_keeps_aggregate_name_conflicts() {
    let harness = Harness::new().await;
    let mut alpha = server("alpha");
    alpha.aggregate_naming_mode = "passthrough_preferred".to_string();
    let mut beta = server("beta");
    beta.aggregate_naming_mode = "passthrough_preferred".to_string();
    let snapshot = ServerCatalogSnapshot {
        tools: vec![json!({"name": "shared_tool"})],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    harness.cache.put(&alpha, snapshot.clone()).await;
    harness.cache.put(&beta, snapshot).await;

    let catalog = harness
        .service()
        .catalog_for_server(&[alpha, beta], "alpha")
        .await
        .unwrap();

    assert_eq!(catalog.tools[0].aggregate_names, vec!["alpha__shared_tool"]);
}

#[tokio::test]
async fn catalog_exposes_disabled_tool_while_aggregate_filters_it() {
    let harness = Harness::new().await;
    let mut server_cfg = server("alpha");
    server_cfg.disabled_tools = json!(["secret_tool"]);
    server_cfg.disabled_resources = json!(["file:///secret"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "public_tool", "description": "pub"}),
            json!({"name": "secret_tool", "description": "sec"}),
        ],
        resources: vec![
            json!({"uri": "file:///public", "name": "pub"}),
            json!({"uri": "file:///secret", "name": "sec"}),
        ],
        resource_templates: vec![
            json!({"uriTemplate": "git://{owner}/{repo}/issues"}),
            json!({"uriTemplate": "git://{owner}/{repo}/secret"}),
        ],
        prompts: vec![json!({"name": "prompt_a"})],
    };
    server_cfg.disabled_resources = json!(["file:///secret", "git://{owner}/{repo}/secret"]);
    harness.cache.put(&server_cfg, snapshot.clone()).await;

    // Cache retains unfiltered snapshot
    let cached = harness.cache.raw_snapshot(&server_cfg).await.unwrap();
    assert_eq!(cached.tools.len(), 2);
    assert_eq!(cached.resources.len(), 2);
    assert_eq!(cached.resource_templates.len(), 2);

    // Admin catalog must expose disabled items
    let catalog = harness
        .service()
        .catalog_for_server(&[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.tools.len(), 2);
    assert!(catalog.tools.iter().any(|t| t.name == "secret_tool"));
    assert_eq!(catalog.resources.len(), 2);
    assert!(catalog.resources.iter().any(|r| r.name == "file:///secret"));

    // Runtime aggregate must filter disabled items per server
    let agg_tools = harness
        .service()
        .aggregate_tools(&[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_tools.len(), 1);
    assert_eq!(agg_tools[0]["name"], "alpha__public_tool");

    let agg_resources = harness
        .service()
        .aggregate_resources(&[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_resources.len(), 1);
    assert_eq!(
        agg_resources[0]["uri"],
        "mcp://alpha/file%3A%2F%2F%2Fpublic"
    );

    let agg_templates = harness
        .service()
        .aggregate_resource_templates(&[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg_templates.len(), 1);
    assert!(
        agg_templates[0]["uriTemplate"]
            .as_str()
            .unwrap()
            .contains("issues")
    );
}

#[tokio::test]
async fn whitelist_catalog_shows_all_while_aggregate_shows_allowed_only() {
    let harness = Harness::new().await;
    let mut server_cfg = server("alpha");
    server_cfg.tool_filter_mode = "whitelist".to_string();
    server_cfg.allowed_tools = json!(["allowed_tool"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "allowed_tool"}),
            json!({"name": "other_tool"}),
            json!({"name": "third_tool"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    harness.cache.put(&server_cfg, snapshot.clone()).await;

    // Admin catalog shows all upstream tools
    let catalog = harness
        .service()
        .catalog_for_server(&[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.tools.len(), 3);
    assert!(catalog.tools.iter().any(|t| t.name == "other_tool"));

    // Aggregate shows only whitelisted
    let agg = harness
        .service()
        .aggregate_tools(&[server_cfg.clone()])
        .await
        .unwrap();
    assert_eq!(agg.len(), 1);
    assert_eq!(agg[0]["name"], "alpha__allowed_tool");

    // Underlying cache still retains all
    let cached = harness.cache.raw_snapshot(&server_cfg).await.unwrap();
    assert_eq!(cached.tools.len(), 3);
}

#[tokio::test]
async fn aggregate_filters_per_server_independently() {
    let harness = Harness::new().await;
    let mut alpha = server("alpha");
    alpha.disabled_tools = json!(["shared_tool"]);
    let mut beta = server("beta");
    beta.disabled_tools = json!([]);

    let snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "shared_tool"}),
            json!({"name": "unique_alpha"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    let beta_snapshot = ServerCatalogSnapshot {
        tools: vec![
            json!({"name": "shared_tool"}),
            json!({"name": "unique_beta"}),
        ],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: Vec::new(),
    };
    harness.cache.put(&alpha, snapshot).await;
    harness.cache.put(&beta, beta_snapshot).await;

    let agg = harness
        .service()
        .aggregate_tools(&[alpha.clone(), beta.clone()])
        .await
        .unwrap();
    assert_eq!(agg.len(), 3);
    let names: Vec<_> = agg
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.contains(&"alpha__shared_tool".to_string()));
    assert!(names.contains(&"beta__shared_tool".to_string()));
    assert!(names.contains(&"alpha__unique_alpha".to_string()));
    assert!(names.contains(&"beta__unique_beta".to_string()));

    // Catalog for alpha still shows its disabled tool
    let catalog_alpha = harness
        .service()
        .catalog_for_server(&[alpha, beta], "alpha")
        .await
        .unwrap();
    assert!(catalog_alpha.tools.iter().any(|t| t.name == "shared_tool"));
}

#[tokio::test]
async fn prompts_remain_unfiltered_in_both_catalog_and_aggregate() {
    let harness = Harness::new().await;
    let mut server_cfg = server("alpha");
    server_cfg.disabled_tools = json!(["any"]);
    let snapshot = ServerCatalogSnapshot {
        tools: vec![json!({"name": "any"})],
        resources: Vec::new(),
        resource_templates: Vec::new(),
        prompts: vec![json!({"name": "prompt_one"}), json!({"name": "prompt_two"})],
    };
    harness.cache.put(&server_cfg, snapshot).await;

    let catalog = harness
        .service()
        .catalog_for_server(&[server_cfg.clone()], "alpha")
        .await
        .unwrap();
    assert_eq!(catalog.prompts.len(), 2);

    let agg = harness
        .service()
        .aggregate_prompts(&[server_cfg])
        .await
        .unwrap();
    assert_eq!(agg.len(), 2);
}

/// An upstream that holds `list_tools` until the test releases it, so a fetch
/// can be paused while its configuration row or cache entry changes.
struct DelayedCatalogServer {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl rmcp::ServerHandler for DelayedCatalogServer {
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        self.started.notify_waiters();
        self.release.notified().await;
        Ok(rmcp::model::ListToolsResult::with_all_items(vec![
            rmcp::model::Tool::new(
                "delayed_tool",
                "delayed tool",
                json!({ "type": "object", "properties": {} })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        ]))
    }

    fn get_info(&self) -> rmcp::model::ServerConfig {
        rmcp::model::InitializeResult::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(rmcp::model::Implementation::new("delayed-upstream", "1.0"))
    }
}

async fn spawn_delayed_upstream() -> (String, Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let service = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        {
            let started = started.clone();
            let release = release.clone();
            move || {
                Ok(DelayedCatalogServer {
                    started: started.clone(),
                    release: release.clone(),
                })
            }
        },
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .disable_allowed_hosts(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{address}/mcp"), started, release)
}

#[tokio::test]
async fn bounded_stale_refresh_concurrency_is_shared_across_service_wrappers() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut servers = Vec::new();
    for index in 0..4 {
        let mut config = input(&format!("shared-{index}"), "http");
        config.url = Some(format!("http://{address}/mcp"));
        let server = harness.persist(config).await;
        harness
            .cache
            .put_at(
                &server,
                snapshot("cached"),
                Utc::now() - ChronoDuration::seconds(600),
            )
            .await;
        servers.push(server);
    }
    // Two distinct wrappers share the cache's coordinator, so the configured
    // bound applies to their combined refresh work, not to each wrapper.
    let first = harness.service().with_refresh_policy(CatalogRefreshPolicy {
        concurrency: 2,
        ..refresh_policy()
    });
    let second = harness.service().with_refresh_policy(CatalogRefreshPolicy {
        concurrency: 2,
        ..refresh_policy()
    });
    let left = {
        let service = first.clone();
        let servers = servers[..2].to_vec();
        tokio::spawn(async move { service.aggregate_tools(&servers).await })
    };
    let right = {
        let service = second.clone();
        let servers = servers[2..].to_vec();
        tokio::spawn(async move { service.aggregate_tools(&servers).await })
    };
    left.await.unwrap().unwrap();
    right.await.unwrap().unwrap();

    let mut connections = Vec::new();
    while let Ok(Ok((socket, _))) =
        tokio::time::timeout(Duration::from_millis(200), listener.accept()).await
    {
        connections.push(socket);
        if connections.len() == servers.len() {
            break;
        }
    }
    let active = connections.len();
    first.cancel_refreshes();
    first.drain_refreshes(Duration::from_secs(2)).await;
    drop(connections);
    drop(listener);
    harness.store.pool().close().await;
    assert!(
        active <= 2,
        "two wrappers sharing one cache opened {active} upstream connections for a bound of 2"
    );
}

#[tokio::test]
async fn repeated_stale_reads_do_not_queue_unbounded_refreshes() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = input("alpha", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;
    let service = harness.service();
    let coordinator = harness.cache.refresh_coordinator().clone();

    for _ in 0..50 {
        let acquired = service.acquire(&server).await.expect("stale catalog");
        assert_eq!(acquired.freshness, CatalogFreshness::Stale);
    }

    let mut connections = Vec::new();
    while let Ok(Ok((socket, _))) =
        tokio::time::timeout(Duration::from_millis(200), listener.accept()).await
    {
        connections.push(socket);
    }
    let active = connections.len();
    let tracked = coordinator.tracked_refreshes();
    let queued = coordinator.queued_refreshes();
    service.cancel_refreshes();
    service.drain_refreshes(Duration::from_secs(2)).await;
    drop(connections);
    drop(listener);
    harness.store.pool().close().await;
    assert_eq!(
        active, 1,
        "fifty stale reads must coalesce into one upstream refresh"
    );
    assert!(
        tracked <= 1 && queued <= 1,
        "stale reads queued {queued} pending and {tracked} tracked refreshes"
    );
}

#[tokio::test]
async fn a_read_triggered_refresh_is_cancelled_and_releases_its_lease() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = input("alpha", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(600),
        )
        .await;
    let service = harness.service();

    let acquired = service.acquire(&server).await.expect("stale catalog");
    assert_eq!(acquired.freshness, CatalogFreshness::Stale);
    let _socket = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("the read-triggered refresh reached the upstream")
        .unwrap()
        .0;

    service.cancel_refreshes();
    service.drain_refreshes(Duration::from_secs(2)).await;

    let next = harness
        .cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await;
    let released = next.is_some();
    if let Some(lease) = next {
        lease.release().await;
    }
    drop(listener);
    harness.store.pool().close().await;
    assert!(
        released,
        "a cancelled read-triggered refresh kept its shared lease"
    );
}

#[tokio::test]
async fn a_config_change_during_an_in_flight_fetch_prevents_publication() {
    let harness = Harness::new().await;
    let (url, started, release) = spawn_delayed_upstream().await;
    let mut config = input("alpha", "http");
    config.url = Some(url);
    let server = harness.persist(config.clone()).await;
    let service = harness.service();

    let listening = started.notified();
    tokio::pin!(listening);
    let fetch = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    listening.await;

    let newer = harness
        .store
        .save_mcp_server(&harness.manager, server.server_id, &config, Some(&server))
        .await
        .expect("update the configuration row");
    assert!(newer.updated_at > server.updated_at);
    release.notify_waiters();

    let error = fetch
        .await
        .expect("the fetch task joins")
        .expect_err("a superseded configuration must not be published");
    assert!(
        error.to_string().contains("configuration changed"),
        "unexpected error: {error}"
    );
    assert!(
        harness.cache.raw_snapshot(&newer).await.is_none(),
        "the superseded fetch published a snapshot"
    );
}

#[tokio::test]
async fn an_invalidate_during_an_in_flight_fetch_prevents_resurrection() {
    let harness = Harness::new().await;
    let (url, started, release) = spawn_delayed_upstream().await;
    let mut config = input("alpha", "http");
    config.url = Some(url);
    let server = harness.persist(config).await;
    // Expired, so the read waits for the in-flight refresh instead of serving
    // the old snapshot while it is scheduled.
    harness
        .cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(4_000),
        )
        .await;
    let service = harness.service();

    let listening = started.notified();
    tokio::pin!(listening);
    let fetch = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    listening.await;
    service.invalidate(server.server_id).await;
    release.notify_waiters();

    let error = fetch
        .await
        .expect("the fetch task joins")
        .expect_err("an invalidated catalog must not be resurrected");
    assert!(
        error.to_string().contains("invalidated"),
        "unexpected error: {error}"
    );
    assert!(
        harness.cache.raw_snapshot(&server).await.is_none(),
        "the in-flight fetch resurrected an invalidated snapshot"
    );
}

#[tokio::test]
async fn reviewer_r2_shared_peer_invalidation_fences_in_flight_publication() {
    let harness = Harness::new().await;
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let upstream = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        {
            let started = started.clone();
            let release = release.clone();
            move || {
                Ok(DelayedCatalogServer {
                    started: started.clone(),
                    release: release.clone(),
                })
            }
        },
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default()
            .into(),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default()
            .disable_allowed_hosts(),
    );
    let router = axum::Router::new().nest_service("/mcp", upstream);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, router).await });
    let mut config = input("peer-invalidation", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    let worker = crate::config::WorkerConfig::default();
    let cache =
        McpCatalogCache::from_config_with_sqlite(&worker, Some(harness.store.pool().clone())).await;
    let peer =
        McpCatalogCache::from_config_with_sqlite(&worker, Some(harness.store.pool().clone())).await;
    cache
        .put_at(
            &server,
            snapshot("old"),
            Utc::now() - ChronoDuration::seconds(4000),
        )
        .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache.clone())
        .with_refresh_policy(refresh_policy());
    let listening = started.notified();
    tokio::pin!(listening);
    let fetch = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    tokio::time::timeout(Duration::from_secs(2), listening)
        .await
        .unwrap();
    peer.invalidate(server.server_id).await;
    release.notify_waiters();
    let result = fetch.await.unwrap();
    let resurrected = peer.lookup(&server).await.snapshot.is_some();
    cache.invalidate(server.server_id).await;
    service.drain_refreshes(Duration::from_secs(1)).await;
    upstream_task.abort();
    let _ = upstream_task.await;
    harness.store.pool().close().await;
    assert!(
        result.is_err() && !resurrected,
        "peer invalidation was lost: fetch_ok={}, resurrected={resurrected}",
        result.is_ok()
    );
}

#[tokio::test]
async fn reviewer_r2_initial_cache_wait_obeys_whole_acquisition_ceiling() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("cache-wait", "builtin_minimax"))
        .await;
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(harness.store.pool().clone()),
    )
    .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(CatalogRefreshPolicy {
            service_ceiling: Duration::from_millis(40),
            ..refresh_policy()
        });
    let mut held = Vec::new();
    for _ in 0..harness.store.pool().options().get_max_connections() {
        held.push(harness.store.pool().acquire().await.unwrap());
    }
    let result = tokio::time::timeout(Duration::from_millis(200), service.acquire(&server)).await;
    drop(held);
    service.drain_refreshes(Duration::from_secs(1)).await;
    harness.store.pool().close().await;
    assert!(
        matches!(result, Ok(Err(_))),
        "a 40ms ceiling did not terminate the initial cache wait within 200ms: {result:?}"
    );
}

#[tokio::test]
async fn reviewer_r2_direct_follower_is_cancelled_and_drained() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("direct-follower", "builtin_minimax"))
        .await;
    let service = harness.service();
    let lease = harness
        .cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .unwrap();
    let mut follower = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    service.cancel_refreshes();
    service.drain_refreshes(Duration::from_millis(100)).await;
    let stopped = tokio::time::timeout(Duration::from_millis(100), &mut follower)
        .await
        .is_ok();
    if !stopped {
        follower.abort();
        let _ = follower.await;
    }
    lease.release().await;
    harness.store.pool().close().await;
    assert!(
        stopped,
        "drain returned while a direct follower was still polling despite cancellation"
    );
}

fn valkey_test_url() -> Option<String> {
    match std::env::var("PROMPT_FERRY_TEST_VALKEY_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            eprintln!("skipping Valkey MCP catalog test: PROMPT_FERRY_TEST_VALKEY_URL is not set");
            None
        }
    }
}

#[tokio::test]
async fn valkey_shared_peer_invalidation_fences_in_flight_publication() {
    let Some(url) = valkey_test_url() else {
        return;
    };
    let harness = Harness::new().await;
    let (upstream_url, started, release) = spawn_delayed_upstream().await;
    let mut config = input("valkey-peer-invalidation", "http");
    config.url = Some(upstream_url);
    let server = harness.persist(config).await;
    let worker = crate::config::WorkerConfig {
        valkey_url: url.clone(),
        valkey_ttl_seconds: 60,
        ..crate::config::WorkerConfig::default()
    };
    let cache = McpCatalogCache::from_config(&worker).await;
    let peer = McpCatalogCache::from_config(&worker).await;
    cache
        .put_at(
            &server,
            snapshot("old"),
            Utc::now() - ChronoDuration::seconds(4_000),
        )
        .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache.clone())
        .with_refresh_policy(refresh_policy());

    let listening = started.notified();
    tokio::pin!(listening);
    let fetch = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    tokio::time::timeout(Duration::from_secs(2), listening)
        .await
        .expect("the fetch reached the upstream");
    peer.invalidate(server.server_id).await;
    release.notify_waiters();
    let result = fetch.await.expect("the fetch task joins");
    let resurrected = peer.lookup(&server).await.snapshot.is_some();
    cache.invalidate(server.server_id).await;
    service.drain_refreshes(Duration::from_secs(1)).await;
    harness.store.pool().close().await;

    let client = redis::Client::open(url).unwrap();
    let mut manager = client.get_connection_manager().await.unwrap();
    let _: usize = redis::AsyncCommands::del(
        &mut manager,
        format!(
            "{}{}",
            crate::mcp::MCP_CATALOG_VALKEY_KEY_PREFIX,
            server.server_id
        ),
    )
    .await
    .unwrap();
    assert!(
        result.is_err() && !resurrected,
        "peer invalidation was lost: fetch_ok={}, resurrected={resurrected}",
        result.is_ok()
    );
}

#[tokio::test]
async fn a_blocked_repository_stage_obeys_the_operation_ceiling() {
    let harness = Harness::new().await;
    let server = harness.persist(input("repo-wait", "builtin_minimax")).await;
    let cache = McpCatalogCache::new();
    cache
        .put_at(
            &server,
            snapshot("cached"),
            Utc::now() - ChronoDuration::seconds(4_000),
        )
        .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(CatalogRefreshPolicy {
            service_ceiling: Duration::from_millis(40),
            ..refresh_policy()
        });
    let mut held = Vec::new();
    for _ in 0..harness.store.pool().options().get_max_connections() {
        held.push(harness.store.pool().acquire().await.unwrap());
    }
    let result =
        tokio::time::timeout(Duration::from_millis(200), service.refresh_server(&server)).await;
    drop(held);
    service.drain_refreshes(Duration::from_secs(1)).await;
    harness.store.pool().close().await;
    assert!(
        matches!(result, Ok(Err(_))),
        "a blocked repository stage did not terminate within the ceiling: {result:?}"
    );
}

#[tokio::test]
async fn a_forced_follower_is_cancelled_and_drained() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("forced-follower", "builtin_minimax"))
        .await;
    let service = harness.service();
    let lease = harness
        .cache
        .try_acquire_refresh_lease(server.server_id, Duration::from_secs(60))
        .await
        .unwrap();
    let mut follower = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.refresh_server(&server).await })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    service.cancel_refreshes();
    service.drain_refreshes(Duration::from_millis(100)).await;
    let stopped = tokio::time::timeout(Duration::from_millis(100), &mut follower).await;
    if stopped.is_err() {
        follower.abort();
        let _ = follower.await;
    }
    lease.release().await;
    harness.store.pool().close().await;
    assert!(
        matches!(stopped, Ok(Ok(Err(_)))),
        "a forced direct follower was not cancelled and drained: {stopped:?}"
    );
}

#[tokio::test]
async fn a_blocked_owner_slot_obeys_the_operation_ceiling() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("permit-wait", "builtin_minimax"))
        .await;
    let service = harness.service().with_refresh_policy(CatalogRefreshPolicy {
        service_ceiling: Duration::from_millis(40),
        concurrency: 1,
        ..refresh_policy()
    });
    let held = harness
        .cache
        .refresh_coordinator()
        .acquire_permit()
        .await
        .expect("the single owner slot");
    let result = tokio::time::timeout(Duration::from_millis(200), service.acquire(&server)).await;
    drop(held);
    service.drain_refreshes(Duration::from_secs(1)).await;
    harness.store.pool().close().await;
    assert!(
        matches!(result, Ok(Err(_))),
        "a blocked owner slot did not terminate within the ceiling: {result:?}"
    );
}

#[tokio::test]
async fn an_aborted_owner_releases_its_accounting_after_cleanup() {
    let harness = Harness::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut config = input("aborted-accounting", "http");
    config.url = Some(format!("http://{address}/mcp"));
    let server = harness.persist(config).await;
    let cache_pool = crate::db::connect_sqlite(&harness.path).await.unwrap();
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(cache_pool.clone()),
    )
    .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(refresh_policy());
    let owner = {
        let service = service.clone();
        let server = server.clone();
        tokio::spawn(async move { service.acquire(&server).await })
    };
    let (socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut held = Vec::new();
    for _ in 0..cache_pool.options().get_max_connections() {
        held.push(cache_pool.acquire().await.unwrap());
    }
    owner.abort();
    let _ = owner.await;
    assert!(
        service.cache().refresh_coordinator().tracked_refreshes() > 0,
        "the aborted owner dropped out of drain accounting before its lease cleanup"
    );

    drop(held);
    let mut tracked = usize::MAX;
    let mut leases = i64::MAX;
    for _ in 0..300 {
        tracked = service.cache().refresh_coordinator().tracked_refreshes();
        leases = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
        )
        .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
        if tracked == 0 && leases == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(socket);
    drop(listener);
    cache_pool.close().await;
    harness.store.pool().close().await;
    assert_eq!(tracked, 0, "the cleanup activity was never released");
    assert_eq!(leases, 0, "the bounded cleanup did not release the lease");
}

#[tokio::test]
async fn a_cancelled_explicit_release_keeps_its_token_for_cleanup() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("release-cancel", "builtin_minimax"))
        .await;
    let cache_pool = crate::db::connect_sqlite(&harness.path).await.unwrap();
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(cache_pool.clone()),
    )
    .await;
    let activity = cache
        .refresh_coordinator()
        .admit()
        .expect("tracked activity");
    let lease = cache
        .try_acquire_tracked_refresh_lease(
            server.server_id,
            Duration::from_secs(60),
            Duration::from_millis(500),
            activity,
        )
        .await
        .expect("tracked lease");

    let mut held = Vec::new();
    for _ in 0..cache_pool.options().get_max_connections() {
        held.push(cache_pool.acquire().await.unwrap());
    }
    let release = tokio::spawn(async move { lease.release().await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    release.abort();
    let _ = release.await;

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
    )
    .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
    .fetch_one(harness.store.pool())
    .await
    .unwrap();
    let tracked = cache.refresh_coordinator().tracked_refreshes();
    drop(held);
    let mut cleaned = remaining == 0;
    for _ in 0..300 {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM standalone_coordinator_leases WHERE lease_key = ?",
        )
        .bind(format!("pfy:mcp-catalog-refresh:{}", server.server_id))
        .fetch_one(harness.store.pool())
        .await
        .unwrap();
        if count == 0 && cache.refresh_coordinator().tracked_refreshes() == 0 {
            cleaned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cache_pool.close().await;
    harness.store.pool().close().await;
    assert_eq!(
        remaining, 1,
        "a cancelled explicit release consumed the ownership token"
    );
    assert!(
        tracked > 0,
        "a cancelled explicit release dropped out of accounting"
    );
    assert!(cleaned, "the bounded cleanup never released the lease");
}

#[tokio::test]
async fn a_blocked_reader_helper_obeys_the_operation_ceiling() {
    let harness = Harness::new().await;
    let server = harness
        .persist(input("helper-wait", "builtin_minimax"))
        .await;
    let cache_pool = crate::db::connect_sqlite(&harness.path).await.unwrap();
    let cache = McpCatalogCache::from_config_with_sqlite(
        &crate::config::WorkerConfig::default(),
        Some(cache_pool.clone()),
    )
    .await;
    let service = McpCatalogService::new_with_repository(harness.repository.clone(), cache)
        .with_refresh_policy(CatalogRefreshPolicy {
            service_ceiling: Duration::from_millis(40),
            ..refresh_policy()
        });
    let mut held = Vec::new();
    for _ in 0..cache_pool.options().get_max_connections() {
        held.push(cache_pool.acquire().await.unwrap());
    }
    let servers = vec![server];
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        service.catalog_for_server(&servers, "helper-wait"),
    )
    .await;
    drop(held);
    service.drain_refreshes(Duration::from_secs(1)).await;
    cache_pool.close().await;
    harness.store.pool().close().await;
    assert!(
        matches!(result, Ok(Err(_))),
        "a blocked reader helper did not terminate within the ceiling: {result:?}"
    );
}
