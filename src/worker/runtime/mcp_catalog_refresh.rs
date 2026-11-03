//! The managed periodic MCP catalog refresh.
//!
//! One task owns the catalog refresh lifecycle of the worker: it performs the
//! startup bulk load and then refreshes every catalog that aged past the soft
//! freshness window. The task is cancellable, so a shutdown cancels an
//! in-flight upstream fetch instead of waiting for it.

use std::time::Duration;

use rand::RngExt;
use tokio::task::JoinHandle;
use tracing::info;

use super::lifecycle::RuntimeControl;
use crate::{
    config::{McpWarmupMode, WorkerConfig},
    mcp::{CATALOG_REFRESH_DRAIN_TIMEOUT, McpCatalogService},
};

/// The base interval between two periodic catalog refresh passes.
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// The share of the interval that is added as a random jitter, so several
/// workers do not fetch the same catalogs at the same moment.
const REFRESH_JITTER_PERCENT: u64 = 10;

/// Start the managed catalog refresh task.
pub(super) fn spawn(
    config: &WorkerConfig,
    service: McpCatalogService,
    control: RuntimeControl,
) -> JoinHandle<()> {
    spawn_task(config, service, control, REFRESH_INTERVAL)
}

fn spawn_task(
    config: &WorkerConfig,
    service: McpCatalogService,
    control: RuntimeControl,
    interval: Duration,
) -> JoinHandle<()> {
    // `all` loads every enabled catalog at startup. `lazy` and `off` skip the
    // bulk cold start; their catalogs are loaded by the read that needs them
    // and refreshed by the periodic pass once they exist.
    let bulk_load = matches!(config.mcp_warmup, McpWarmupMode::All);
    tokio::spawn(async move {
        if bulk_load && !run_pass(&control, &service, service.warm_enabled_servers()).await {
            stop_and_drain(&service).await;
            return;
        }
        loop {
            tokio::select! {
                _ = control.wait_for_shutdown() => break,
                _ = tokio::time::sleep(next_interval(interval)) => {}
            }
            if !run_pass(&control, &service, service.refresh_aging_entries()).await {
                break;
            }
        }
        stop_and_drain(&service).await;
        info!(
            category = "mcp_catalog_refresh",
            "MCP catalog refresh task stopped"
        );
    })
}

/// Run one pass, stopping the work it owns as soon as shutdown starts.
///
/// Returns `false` when the shutdown interrupted the pass. The coordinator is
/// cancelled first, so every refresh it owns — including the ones the pass only
/// scheduled — releases its lease explicitly instead of waiting for the lease
/// expiry.
async fn run_pass<F>(control: &RuntimeControl, service: &McpCatalogService, pass: F) -> bool
where
    F: std::future::Future<Output = ()>,
{
    tokio::pin!(pass);
    tokio::select! {
        _ = pass.as_mut() => true,
        _ = control.wait_for_shutdown() => {
            service.cancel_refreshes();
            let _ = tokio::time::timeout(CATALOG_REFRESH_DRAIN_TIMEOUT, pass.as_mut()).await;
            false
        }
    }
}

/// Stop scheduling refresh work and wait for the owned refreshes to stop.
async fn stop_and_drain(service: &McpCatalogService) {
    service.cancel_refreshes();
    service.drain_refreshes(CATALOG_REFRESH_DRAIN_TIMEOUT).await;
}

/// The interval until the next pass, with a random jitter on top.
fn next_interval(base: Duration) -> Duration {
    let jitter_max = base.as_millis() as u64 * REFRESH_JITTER_PERCENT / 100;
    let jitter = rand::rng().random_range(0..=jitter_max);
    base + Duration::from_millis(jitter)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use chrono::{Duration as ChronoDuration, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{
        config::WorkerConfig,
        db::{McpServer, McpServerInput},
        mcp::{McpCatalogCache, ServerCatalogSnapshot},
        relay_secrets::RelaySecretManager,
        standalone_config::StandaloneConfigStore,
    };

    /// A worker harness over a real SQLite configuration store.
    struct Harness {
        path: PathBuf,
        store: Arc<StandaloneConfigStore>,
        manager: RelaySecretManager,
        repository: crate::db::ConfigRepository,
        cache: McpCatalogCache,
        config: WorkerConfig,
    }

    impl Harness {
        async fn new(mode: McpWarmupMode) -> Self {
            let path = std::env::temp_dir().join(format!(
                "prompt-ferry-mcp-refresh-{}.sqlite",
                Uuid::new_v4()
            ));
            let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
            let manager =
                RelaySecretManager::from_base64(&STANDARD.encode([13_u8; 32])).expect("manager");
            let repository = crate::db::ConfigRepository::sqlite(store.clone(), manager.clone());
            let config = WorkerConfig {
                mcp_warmup: mode,
                ..WorkerConfig::default()
            };
            let cache =
                McpCatalogCache::from_config_with_sqlite(&config, Some(store.pool().clone())).await;
            Self {
                path,
                store,
                manager,
                repository,
                cache,
                config,
            }
        }

        fn service(&self) -> McpCatalogService {
            McpCatalogService::new_with_repository(self.repository.clone(), self.cache.clone())
        }

        async fn persist(&self, transport: &str, url: Option<String>) -> McpServer {
            let input = McpServerInput {
                scope: "admin".to_string(),
                owner_user_id: None,
                source_endpoint_id: None,
                name: "alpha".to_string(),
                aggregate_naming_mode: "qualified_only".to_string(),
                transport: transport.to_string(),
                provider_kind: None,
                url,
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
                timeout_ms: 30_000,
                lifecycle_policy: "auto".to_string(),
                lifecycle_manual_protocol_version: None,
            };
            self.store
                .save_mcp_server(&self.manager, Uuid::new_v4(), &input, None)
                .await
                .expect("persist mcp server")
        }

        async fn refresh_leases(&self) -> i64 {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM standalone_coordinator_leases \
                 WHERE lease_key LIKE 'pfy:mcp-catalog-refresh:%'",
            )
            .fetch_one(self.store.pool())
            .await
            .unwrap_or_default()
        }
    }

    impl Drop for Harness {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(format!("{}-wal", self.path.display()));
            let _ = std::fs::remove_file(format!("{}-shm", self.path.display()));
        }
    }

    fn stale_snapshot() -> ServerCatalogSnapshot {
        ServerCatalogSnapshot {
            tools: vec![json!({ "name": "cached_tool" })],
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
        }
    }

    async fn wait_until_loaded(cache: &McpCatalogCache, server: &McpServer) {
        for _ in 0..600 {
            if cache.raw_snapshot(server).await.is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the catalog was not loaded");
    }

    async fn wait_until_published(cache: &McpCatalogCache, server: &McpServer) {
        for _ in 0..600 {
            let refreshed = cache.raw_snapshot(server).await.is_some_and(|snapshot| {
                snapshot.tools.first().is_some_and(|tool| {
                    tool.get("name").and_then(|name| name.as_str()) != Some("cached_tool")
                })
            });
            if refreshed {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the catalog was not refreshed");
    }

    #[test]
    fn the_periodic_interval_stays_inside_the_documented_window() {
        for _ in 0..64 {
            let interval = next_interval(REFRESH_INTERVAL);
            assert!(interval >= REFRESH_INTERVAL);
            assert!(
                interval
                    <= REFRESH_INTERVAL
                        + Duration::from_millis(
                            REFRESH_INTERVAL.as_millis() as u64 * REFRESH_JITTER_PERCENT / 100,
                        )
            );
        }
        assert_eq!(REFRESH_INTERVAL, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn the_all_mode_loads_every_catalog_at_startup() {
        let harness = Harness::new(McpWarmupMode::All).await;
        let server = harness.persist("builtin_minimax", None).await;
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            harness.service(),
            control.clone(),
            Duration::from_secs(3600),
        );

        wait_until_loaded(&harness.cache, &server).await;

        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the refresh task stops on shutdown")
            .expect("the refresh task joins");
    }

    #[tokio::test]
    async fn the_lazy_mode_does_not_cold_load_at_startup() {
        let harness = Harness::new(McpWarmupMode::Lazy).await;
        let server = harness.persist("builtin_minimax", None).await;
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            harness.service(),
            control.clone(),
            Duration::from_secs(3600),
        );

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            harness.cache.raw_snapshot(&server).await.is_none(),
            "the lazy mode must not bulk load catalogs at startup"
        );

        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the refresh task stops on shutdown")
            .expect("the refresh task joins");
    }

    #[tokio::test]
    async fn the_off_mode_does_not_cold_load_at_startup() {
        let harness = Harness::new(McpWarmupMode::Off).await;
        let server = harness.persist("builtin_minimax", None).await;
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            harness.service(),
            control.clone(),
            Duration::from_secs(3600),
        );

        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            harness.cache.raw_snapshot(&server).await.is_none(),
            "the off mode must not bulk load catalogs at startup"
        );

        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the refresh task stops on shutdown")
            .expect("the refresh task joins");
    }

    #[tokio::test]
    async fn the_periodic_pass_refreshes_an_aged_catalog_in_lazy_mode() {
        let harness = Harness::new(McpWarmupMode::Lazy).await;
        let server = harness.persist("builtin_minimax", None).await;
        harness
            .cache
            .put_at(
                &server,
                stale_snapshot(),
                Utc::now() - ChronoDuration::seconds(600),
            )
            .await;
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            harness.service(),
            control.clone(),
            Duration::from_millis(50),
        );

        wait_until_published(&harness.cache, &server).await;

        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the refresh task stops on shutdown")
            .expect("the refresh task joins");
    }

    #[tokio::test]
    async fn reviewer_shutdown_cancels_stale_startup_refresh() {
        let harness = Harness::new(McpWarmupMode::All).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = harness
            .persist("http", Some(format!("http://{address}/mcp")))
            .await;
        harness
            .cache
            .put_at(
                &server,
                stale_snapshot(),
                Utc::now() - ChronoDuration::seconds(600),
            )
            .await;
        let service = harness
            .service()
            .with_refresh_policy(crate::mcp::CatalogRefreshPolicy {
                service_ceiling: Duration::from_secs(1),
                ..crate::mcp::CatalogRefreshPolicy::production()
            });
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            service,
            control.clone(),
            Duration::from_secs(3600),
        );
        let (socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let remaining_leases = harness.refresh_leases().await;
        drop(socket);
        drop(listener);
        tokio::time::sleep(Duration::from_millis(1200)).await;
        harness.store.pool().close().await;
        assert_eq!(
            remaining_leases, 0,
            "the joined managed task left its stale startup fetch running"
        );
    }

    #[tokio::test]
    async fn a_shutdown_cancels_an_in_flight_fetch() {
        let harness = Harness::new(McpWarmupMode::Lazy).await;
        // A listener that accepts the connection but never answers keeps the
        // upstream fetch in flight until the shutdown cancels it.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("address");
        let server = harness
            .persist("http", Some(format!("http://{address}/mcp")))
            .await;
        // The periodic pass only refreshes an entry that already exists.
        harness
            .cache
            .put_at(
                &server,
                stale_snapshot(),
                Utc::now() - ChronoDuration::seconds(600),
            )
            .await;
        let _ = &server;
        let control = RuntimeControl::new();
        let task = spawn_task(
            &harness.config,
            harness.service(),
            control.clone(),
            Duration::from_millis(50),
        );

        let mut leased = false;
        for _ in 0..600 {
            if harness.refresh_leases().await > 0 {
                leased = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(leased, "the periodic refresh did not start a fetch");

        control.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the in-flight fetch is cancelled on shutdown")
            .expect("the refresh task joins");
        drop(listener);
    }
}
