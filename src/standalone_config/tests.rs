use std::{
    collections::HashSet,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sqlx::Row;
use tokio::sync::Barrier;
use uuid::Uuid;

use super::*;
use crate::{
    config::{BridgeEncryptionMode, NativeApi, NativeApiSource, TlsMode},
    relay_secrets::RelaySecretManager,
};

fn manager(byte: u8) -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([byte; 32])).expect("test manager")
}

fn database_path() -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!("prompt-ferry-standalone-{suffix}.sqlite"))
}

fn sample_config() -> StandaloneConfig {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    StandaloneConfig {
        relays: vec![ManagedRelayConfig {
            relay_id: Uuid::new_v4(),
            name: "local relay".to_string(),
            relay_url: "ws://127.0.0.1:8788/ws/worker".to_string(),
            enabled: true,
            tls_mode: TlsMode::Off,
            bridge_encryption_mode: BridgeEncryptionMode::Required,
            relay_ca_pem: Some("CA PEM".to_string()),
            client_cert_pem: Some("CERT PEM".to_string()),
            client_key_pem: Some("CLIENT KEY".to_string()),
            bridge_encryption_key: Some("BRIDGE KEY".to_string()),
        }],
        endpoints: vec![ProviderEndpointConfig {
            endpoint_id,
            name: "upstream".to_string(),
            provider: EndpointProvider::Generic,
            provider_region: Some(EndpointRegion::Global),
            base_url: "https://api.example.test".to_string(),
            native_api: NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            key_lb_enabled: true,
            enabled: true,
            mcp_enabled: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            api_key: "endpoint-secret".to_string(),
            api_keys: vec![EndpointApiKeyConfig {
                key_id,
                endpoint_id,
                key_label: "primary".to_string(),
                api_key: "endpoint-key-secret".to_string(),
                position: 0,
                enabled: true,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
        }],
        routes: vec![ModelRouteConfig {
            rule_id: Uuid::new_v4(),
            scope: RouteScope::Admin,
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: RoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: Some(100),
            monthly_max_requests: Some(1_000),
            enabled: true,
            targets: vec![ModelRouteTargetConfig {
                target_id: Uuid::new_v4(),
                endpoint_id,
                position: 0,
                enabled: true,
                upstream_model: Some("provider-model".to_string()),
                responses_continuation_policy: ContinuationPolicy::ForcePassthrough,
            }],
        }],
        client_keys: vec![ClientKeyConfig {
            key_id: Uuid::new_v4(),
            user_id: 7,
            key_prefix: "pfy_test".to_string(),
            label: "test client".to_string(),
            secret: "client-secret".to_string(),
            enabled: true,
        }],
        settings: vec![SettingConfig {
            key: "redaction_config".to_string(),
            version: 1,
            value: serde_json::json!({"enabled": true}),
        }],
    }
}

fn concurrent_endpoint(index: usize) -> ProviderEndpointConfig {
    let mut endpoint = sample_config().endpoints.remove(0);
    endpoint.endpoint_id = Uuid::new_v4();
    endpoint.name = format!("concurrent-endpoint-{index}");
    endpoint.api_key = format!("concurrent-endpoint-secret-{index}");
    endpoint.api_keys[0].key_id = Uuid::new_v4();
    endpoint.api_keys[0].api_key = format!("concurrent-endpoint-key-secret-{index}");
    endpoint
}

async fn open_store() -> (StandaloneConfigStore, PathBuf) {
    let path = database_path();
    let store = StandaloneConfigStore::open(&path)
        .await
        .expect("open store");
    (store, path)
}

async fn cleanup(store: StandaloneConfigStore, path: PathBuf) {
    store.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn schema_creation_has_typed_tables_without_plaintext_secret_columns() {
    let (store, path) = open_store().await;
    // Inspect through a separate pool after migration has completed.
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("pool");
    let table_names = sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/schema_tables.sql"
    )))
    .fetch_all(&pool)
    .await
    .expect("tables")
    .into_iter()
    .map(|row| row.try_get::<String, _>("name").expect("table name"))
    .collect::<HashSet<_>>();
    assert!(table_names.contains("standalone_schema_meta"));
    assert!(table_names.contains("standalone_provider_endpoints"));
    assert!(table_names.contains("standalone_endpoint_keys"));
    assert!(table_names.contains("standalone_model_routes"));
    assert!(table_names.contains("standalone_client_keys"));
    assert!(table_names.contains("standalone_users"));
    assert!(table_names.contains("standalone_mcp_servers"));
    let secret_columns = sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/schema_columns.sql"
    )))
    .fetch_all(&pool)
    .await
    .expect("columns")
    .into_iter()
    .map(|row| {
        row.try_get::<String, _>("column_name")
            .expect("column name")
    })
    .filter(|column| matches!(column.as_str(), "api_key" | "secret" | "password"))
    .collect::<Vec<_>>();
    assert!(
        secret_columns.is_empty(),
        "plaintext secret columns: {secret_columns:?}"
    );
    pool.close().await;
    cleanup(store, path).await;
}

#[tokio::test]
async fn restart_persists_configuration_and_wrong_key_is_rejected() {
    let (store, path) = open_store().await;
    let expected = sample_config();
    store
        .replace_snapshot(&manager(5), &expected)
        .await
        .expect("save");
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("inspect pool");
    let stored_secret = sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/inspect_endpoint_ciphertext.sql"
    )))
    .fetch_one(&pool)
    .await
    .expect("stored endpoint")
    .try_get::<Vec<u8>, _>("api_key_ciphertext")
    .expect("ciphertext");
    assert!(!String::from_utf8_lossy(&stored_secret).contains("endpoint-secret"));
    pool.close().await;
    store.close().await;

    let reopened = StandaloneConfigStore::open(&path).await.expect("reopen");
    let restored = reopened.load_snapshot(&manager(5)).await.expect("load");
    assert_eq!(restored, expected);
    assert!(reopened.load_snapshot(&manager(9)).await.is_err());
    cleanup(reopened, path).await;
}

#[test]
fn public_debug_output_redacts_all_standalone_secrets() {
    let config = sample_config();
    let output = format!("{config:?}");
    for secret in [
        "CA PEM",
        "CERT PEM",
        "CLIENT KEY",
        "BRIDGE KEY",
        "endpoint-secret",
        "endpoint-key-secret",
        "client-secret",
    ] {
        assert!(!output.contains(secret), "debug output leaked {secret:?}");
    }
    assert!(output.contains("local relay"));
    assert!(output.contains("[REDACTED;"));

    let seed = BootstrapSeed {
        relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
        tls_mode: TlsMode::Off,
        relay_ca_pem: Some("seed CA".to_string()),
        client_cert_pem: Some("seed cert".to_string()),
        client_key_pem: Some("seed key".to_string()),
        bridge_encryption_mode: BridgeEncryptionMode::Required,
        bridge_encryption_key: Some("seed bridge".to_string()),
        upstream_base_url: "https://api.example.test".to_string(),
        upstream_api_key: "seed endpoint secret".to_string(),
        upstream_native_api: NativeApi::Responses,
    };
    let seed_output = format!("{seed:?}");
    for secret in [
        "seed CA",
        "seed cert",
        "seed key",
        "seed bridge",
        "seed endpoint secret",
    ] {
        assert!(
            !seed_output.contains(secret),
            "debug output leaked {secret:?}"
        );
    }
}

#[tokio::test]
async fn future_schema_version_is_rejected() {
    let (store, path) = open_store().await;
    store.close().await;
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("pool");
    sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/set_future_schema_version.sql"
    )))
    .execute(&pool)
    .await
    .expect("set future version");
    pool.close().await;
    let error = StandaloneConfigStore::open(&path)
        .await
        .expect_err("future version must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::UnsupportedSchemaVersion { found: 99, .. }
    ));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn incomplete_schema_is_reported_as_corrupt_database_on_open() {
    let (store, path) = open_store().await;
    store.close().await;

    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .expect("pool");
    sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/drop_schema_meta.sql"
    )))
    .execute(&pool)
    .await
    .expect("tamper schema");
    pool.close().await;

    let error = StandaloneConfigStore::open(&path)
        .await
        .expect_err("incomplete schema must fail to open");
    assert!(matches!(
        error,
        StandaloneConfigError::CorruptDatabase(message)
            if message.contains("schema metadata query failed")
    ));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_is_idempotent_and_does_not_overwrite_existing_configuration() {
    let (store, path) = open_store().await;
    let seed = BootstrapSeed {
        relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
        tls_mode: TlsMode::Off,
        relay_ca_pem: None,
        client_cert_pem: None,
        client_key_pem: None,
        bridge_encryption_mode: BridgeEncryptionMode::Off,
        bridge_encryption_key: None,
        upstream_base_url: "https://api.example.test".to_string(),
        upstream_api_key: "bootstrap-secret".to_string(),
        upstream_native_api: NativeApi::Responses,
    };
    assert!(
        store
            .bootstrap_if_empty(&manager(5), seed.clone())
            .await
            .expect("seed")
            .seeded
    );
    let first = store
        .load_snapshot(&manager(5))
        .await
        .expect("first snapshot");
    assert!(
        !store
            .bootstrap_if_empty(
                &manager(5),
                BootstrapSeed {
                    upstream_api_key: "replacement-secret".to_string(),
                    ..seed
                }
            )
            .await
            .expect("second seed")
            .seeded
    );
    assert_eq!(
        store
            .load_snapshot(&manager(5))
            .await
            .expect("second snapshot"),
        first
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn failed_multi_table_replace_is_atomic() {
    let (store, path) = open_store().await;
    let initial = sample_config();
    store
        .replace_snapshot(&manager(5), &initial)
        .await
        .expect("initial save");
    let mut invalid = initial.clone();
    let endpoint_id = invalid.endpoints[0].endpoint_id;
    invalid.endpoints[0].api_keys.push(EndpointApiKeyConfig {
        key_id: Uuid::new_v4(),
        endpoint_id,
        key_label: "primary".to_string(),
        api_key: "duplicate-label-secret".to_string(),
        position: 1,
        enabled: true,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    });
    assert!(store.replace_snapshot(&manager(5), &invalid).await.is_err());
    assert_eq!(
        store
            .load_snapshot(&manager(5))
            .await
            .expect("after rollback"),
        initial
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn concurrent_endpoint_writes_and_reads_use_sqlite_busy_timeout() {
    let (store, path) = open_store().await;
    let store = Arc::new(store);
    let barrier = Arc::new(Barrier::new(8));
    let mut tasks = Vec::new();

    for index in 0..4 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            let endpoint = concurrent_endpoint(index);
            barrier.wait().await;
            store
                .save_endpoint(&manager(5), &endpoint)
                .await
                .expect("concurrent endpoint write");
            let snapshot = store
                .load_snapshot(&manager(5))
                .await
                .expect("read after endpoint write");
            assert!(
                snapshot
                    .endpoints
                    .iter()
                    .any(|saved| saved.endpoint_id == endpoint.endpoint_id)
            );
        }));
    }

    for _ in 0..4 {
        let store = Arc::clone(&store);
        let barrier = Arc::clone(&barrier);
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .load_snapshot(&manager(5))
                .await
                .expect("concurrent endpoint read");
        }));
    }

    for task in tasks {
        task.await.expect("concurrent task");
    }

    let snapshot = store
        .load_snapshot(&manager(5))
        .await
        .expect("final snapshot");
    assert_eq!(snapshot.endpoints.len(), 4);
    for index in 0..4 {
        assert!(
            snapshot
                .endpoints
                .iter()
                .any(|endpoint| endpoint.name == format!("concurrent-endpoint-{index}"))
        );
    }

    let store = Arc::try_unwrap(store).ok().expect("only store owner");
    cleanup(store, path).await;
}

#[tokio::test]
async fn sqlite_coordinator_allows_only_one_live_lease_owner() {
    let (store, path) = open_store().await;
    let first = StandaloneCoordinatorStore::new(store.pool().clone());
    let second = StandaloneCoordinatorStore::new(store.pool().clone());
    let barrier = Arc::new(Barrier::new(2));
    let first_barrier = Arc::clone(&barrier);
    let second_barrier = Arc::clone(&barrier);
    let first_task = tokio::spawn(async move {
        first_barrier.wait().await;
        first
            .acquire_lease("maintenance", "worker-a", 30)
            .await
            .expect("first lease attempt")
    });
    let second_task = tokio::spawn(async move {
        second_barrier.wait().await;
        second
            .acquire_lease("maintenance", "worker-b", 30)
            .await
            .expect("second lease attempt")
    });
    let first_acquired = first_task.await.expect("first task");
    let second_acquired = second_task.await.expect("second task");
    assert_ne!(first_acquired, second_acquired);

    let owner = if first_acquired {
        "worker-a"
    } else {
        "worker-b"
    };
    let other = if first_acquired {
        "worker-b"
    } else {
        "worker-a"
    };
    let coordinator = StandaloneCoordinatorStore::new(store.pool().clone());
    assert!(
        coordinator
            .refresh_lease("maintenance", owner, 30)
            .await
            .unwrap()
    );
    assert!(
        !coordinator
            .acquire_lease("maintenance", other, 30)
            .await
            .unwrap()
    );
    coordinator
        .release_lease("maintenance", owner)
        .await
        .expect("release lease");
    assert!(
        coordinator
            .acquire_lease("maintenance", other, 30)
            .await
            .unwrap()
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn sqlite_coordinator_values_refresh_without_process_local_state() {
    let (store, path) = open_store().await;
    let first = StandaloneCoordinatorStore::new(store.pool().clone());
    let second = StandaloneCoordinatorStore::new(store.pool().clone());
    let value = first
        .get_or_insert("session", "same-session", "first", 30)
        .await
        .expect("insert session value");
    assert_eq!(value, "first");
    assert_eq!(
        second.get("session", "same-session").await.unwrap(),
        Some("first".to_string())
    );
    assert_eq!(
        second
            .get_or_insert("session", "same-session", "second", 30)
            .await
            .unwrap(),
        "first"
    );
    assert!(second.delete("session", "same-session").await.unwrap());
    assert_eq!(first.get("session", "same-session").await.unwrap(), None);
    cleanup(store, path).await;
}

#[tokio::test]
async fn legacy_schema_migrates_users_and_keeps_encrypted_client_keys() {
    let path = database_path();
    let manager = manager(5);
    let legacy_key_id = Uuid::new_v4();
    let pool = crate::db::connect_sqlite(&path).await.expect("pool");
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/standalone/0001_initial.sql"
    )))
    .execute(&pool)
    .await
    .expect("legacy schema");
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations/standalone/0002_storage_contract.sql"
    )))
    .execute(&pool)
    .await
    .expect("phase 1 schema");

    let envelope = manager
        .encrypt("legacy-client-secret")
        .expect("encrypt key");
    sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/save_client_key.sql"
    )))
    .bind(legacy_key_id.to_string())
    .bind(77_i64)
    .bind("legacy")
    .bind("Legacy client key")
    .bind(1_i64)
    .bind(envelope.ciphertext)
    .bind(envelope.nonce)
    .bind(i64::from(envelope.key_version))
    .execute(&pool)
    .await
    .expect("legacy client key");
    pool.close().await;

    let store = StandaloneConfigStore::open(&path)
        .await
        .expect("migrate legacy");
    let version = sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/sql/standalone/schema_version.sql"
    )))
    .fetch_one(store.pool())
    .await
    .expect("schema version")
    .try_get::<i64, _>("schema_version")
    .expect("version value");
    assert_eq!(version, 6);

    let snapshot = store
        .load_snapshot(&manager)
        .await
        .expect("load legacy snapshot");
    assert_eq!(snapshot.client_keys[0].secret, "legacy-client-secret");
    let legacy_user = sqlx::query(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/standalone_config/sql/users/inspect_user_enabled.sql"
    )))
    .bind(77_i64)
    .fetch_one(store.pool())
    .await
    .expect("legacy user placeholder")
    .try_get::<i64, _>("enabled")
    .expect("legacy user status");
    assert_eq!(legacy_user, 0);
    cleanup(store, path).await;
}

// ---- Phase 1A standalone usage ledger tests ----

fn sample_usage_record(
    request_id: Uuid,
    path: &str,
    model: Option<&str>,
) -> StandaloneUsageSummaryRecord {
    StandaloneUsageSummaryRecord {
        request_id,
        event_kind: "request".to_string(),
        category: "ai".to_string(),
        state: "completed".to_string(),
        path: path.to_string(),
        recorded_at: chrono::Utc::now(),
        status: Some(200),
        ok: Some(true),
        duration_ms: Some(123),
        ttft_ms: Some(45),
        model: model.map(str::to_string),
        requested_model: model.map(str::to_string),
        upstream_model: model.map(str::to_string),
        endpoint_id: Some(Uuid::new_v4()),
        endpoint_key_id: Some(Uuid::new_v4()),
        model_route_rule_id: Some(Uuid::new_v4()),
        mcp_server_id: None,
        input_tokens: Some(11),
        output_tokens: Some(22),
        total_tokens: Some(33),
        cached_tokens: Some(0),
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        error_code: None,
        failure_family: None,
        redaction_applied: true,
        redaction_findings_count: 1,
        redaction_replacements_count: 1,
        redaction_types: vec!["email".to_string()],
        redaction_fields: vec!["messages.email".to_string()],
        route_selection_reason: "default".to_string(),
    }
}

#[tokio::test]
async fn fresh_migration_creates_empty_usage_ledger_at_schema_version_six() {
    let (store, path) = open_store().await;
    let version = standalone_query!("src/sql/standalone/schema_version.sql")
        .fetch_one(store.pool())
        .await
        .expect("schema version")
        .try_get::<i64, _>("schema_version")
        .expect("version value");
    assert_eq!(version, 6);
    assert!(
        store
            .list_usage_summaries(64)
            .await
            .expect("list")
            .is_empty()
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn insert_and_list_usage_summaries_preserve_insertion_order() {
    let (store, path) = open_store().await;
    let first = sample_usage_record(Uuid::new_v4(), "/v1/responses", Some("gpt-5"));
    let second = sample_usage_record(Uuid::new_v4(), "/v1/chat", Some("claude"));
    let third = sample_usage_record(Uuid::new_v4(), "/v1/embeddings", None);
    let inserts = [&first, &second, &third];
    let mut event_ids = Vec::new();
    for record in inserts {
        let event_id = store
            .insert_usage_summary(record)
            .await
            .expect("insert summary");
        assert!(event_id > 0, "event_id must be positive");
        event_ids.push(event_id);
        // Re-inserting the same record creates a new event row so retries,
        // replays, and repeated lifecycle events all persist.
        let duplicate_event_id = store
            .insert_usage_summary(record)
            .await
            .expect("re-insert summary");
        assert_ne!(
            duplicate_event_id, event_id,
            "duplicate insert must allocate a new event_id"
        );
    }
    assert_eq!(event_ids.len(), 3);
    assert!(
        event_ids.windows(2).all(|window| window[0] < window[1]),
        "event_ids must increase monotonically across inserts"
    );

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list summaries");
    assert_eq!(stored.len(), 6, "duplicate inserts must produce six rows");
    let ids: Vec<Uuid> = stored.iter().map(|record| record.request_id).collect();
    assert_eq!(
        ids,
        vec![
            first.request_id,
            first.request_id,
            second.request_id,
            second.request_id,
            third.request_id,
            third.request_id,
        ]
    );
    assert_eq!(stored.first().expect("first").path, "/v1/responses");
    assert_eq!(
        stored.last().expect("last").redaction_types,
        vec!["email".to_string()]
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn repeated_lifecycle_events_for_one_request_retain_terminal_state() {
    let (store, path) = open_store().await;
    let request_id = Uuid::new_v4();
    let mut initial = sample_usage_record(request_id, "/v1/responses", Some("gpt-5"));
    initial.state = "received".to_string();
    let mut terminal = sample_usage_record(request_id, "/v1/responses", Some("gpt-5"));
    terminal.state = "failed".to_string();
    terminal.error_code = Some("upstream_error".to_string());
    terminal.failure_family = Some("upstream_5xx".to_string());
    terminal.status = Some(502);
    terminal.ok = Some(false);

    let first_event_id = store
        .insert_usage_summary(&initial)
        .await
        .expect("initial insert");
    let second_event_id = store
        .insert_usage_summary(&terminal)
        .await
        .expect("terminal insert");
    assert!(
        second_event_id > first_event_id,
        "terminal event must allocate a strictly greater event_id"
    );

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list summaries");
    let for_request: Vec<&StandaloneUsageSummaryRecord> = stored
        .iter()
        .filter(|record| record.request_id == request_id)
        .collect();
    assert_eq!(for_request.len(), 2, "both lifecycle events must persist");
    let states: Vec<&str> = for_request
        .iter()
        .map(|record| record.state.as_str())
        .collect();
    assert_eq!(
        states,
        vec!["received", "failed"],
        "insertion order must keep the original Received event and the terminal Failed event"
    );
    assert_eq!(for_request[1].error_code.as_deref(), Some("upstream_error"));
    cleanup(store, path).await;
}

#[tokio::test]
async fn prune_usage_summaries_trims_old_rows_to_max_rows() {
    let (store, path) = open_store().await;
    let records: Vec<StandaloneUsageSummaryRecord> = (0..5)
        .map(|index| {
            sample_usage_record(Uuid::new_v4(), &format!("/v1/test/{index}"), Some("gpt-5"))
        })
        .collect();
    for record in &records {
        store.insert_usage_summary(record).await.expect("insert");
    }
    let removed = store.prune_usage_summaries(2).await.expect("prune");
    assert_eq!(removed, 3, "three oldest rows must be removed");

    let kept = store.list_usage_summaries(16).await.expect("list kept");
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].request_id, records[3].request_id);
    assert_eq!(kept[1].request_id, records[4].request_id);

    // Pruning to a count equal to or larger than the current size is a
    // no-op and must not error.
    let noop = store.prune_usage_summaries(2).await.expect("noop prune");
    assert_eq!(noop, 0);

    // Pruning to a count equal to or larger than the current size is a
    // no-op and must not error.
    let noop = store.prune_usage_summaries(2).await.expect("noop prune");
    assert_eq!(noop, 0);
    cleanup(store, path).await;
}

#[tokio::test]
async fn reopen_restores_recent_usage_summaries_from_durable_ledger() {
    let (store, path) = open_store().await;
    let persisted = [
        sample_usage_record(Uuid::new_v4(), "/v1/a", Some("a")),
        sample_usage_record(Uuid::new_v4(), "/v1/b", Some("b")),
        sample_usage_record(Uuid::new_v4(), "/v1/c", Some("c")),
    ];
    for record in &persisted {
        store.insert_usage_summary(record).await.expect("insert");
    }
    store.close().await;

    let reopened = StandaloneConfigStore::open(&path).await.expect("reopen");
    let restored = reopened
        .list_usage_summaries(64)
        .await
        .expect("list after reopen");
    let ids = restored
        .iter()
        .map(|record| record.request_id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        persisted
            .iter()
            .map(|record| record.request_id)
            .collect::<Vec<_>>()
    );
    cleanup(reopened, path).await;
}

#[tokio::test]
async fn usage_summary_record_omits_raw_bodies_and_secrets() {
    let (store, path) = open_store().await;
    let mut record = sample_usage_record(Uuid::new_v4(), "/v1/redacted-path", Some("model"));
    // Even if a caller accidentally tries to push raw body fields through
    // the storage API, the DTO has no fields for raw bodies or secrets.
    record.error_code = Some("provider_error".to_string());
    record.failure_family = Some("upstream_5xx".to_string());
    store.insert_usage_summary(&record).await.expect("insert");
    let stored = store
        .list_usage_summaries(1)
        .await
        .expect("list")
        .into_iter()
        .next()
        .expect("stored summary");
    let debug = format!("{stored:?}");
    // The path field is a request route, not a body, and must round-trip.
    assert!(debug.contains("redacted-path"));
    // The DTO has no fields for raw request bodies, raw response bodies,
    // or session bodies. Field names like `cache_read_tokens` are token
    // counters, not token values, so they are not subject to this check.
    for forbidden in [
        "request_body",
        "response_body",
        "raw_body",
        "session_secret",
    ] {
        assert!(
            !debug.contains(forbidden),
            "Debug output must not contain {forbidden:?}; found: {debug}"
        );
    }
    cleanup(store, path).await;
}

#[tokio::test]
async fn list_usage_summaries_skips_malformed_rows_and_keeps_valid_ones() {
    let (store, path) = open_store().await;
    let keep = sample_usage_record(Uuid::new_v4(), "/v1/responses", Some("gpt-5"));
    let corrupt = sample_usage_record(Uuid::new_v4(), "/v1/chat", Some("claude"));
    let keep_request_id = keep.request_id;
    let corrupt_request_id = corrupt.request_id;
    store
        .insert_usage_summary(&keep)
        .await
        .expect("insert keep");
    store
        .insert_usage_summary(&corrupt)
        .await
        .expect("insert corrupt");

    // Tamper with only the second row's recorded_at so it carries an
    // unparseable timestamp that the domain parser rejects.
    let pool = store.pool().clone();
    sqlx::query("UPDATE standalone_usage_summaries SET recorded_at = ? WHERE request_id = ?")
        .bind("not-a-timestamp")
        .bind(corrupt_request_id.to_string())
        .execute(&pool)
        .await
        .expect("tamper recorded_at");

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list tolerates corrupt row");
    let loaded_ids: Vec<Uuid> = stored.iter().map(|record| record.request_id).collect();
    assert_eq!(
        loaded_ids,
        vec![keep_request_id],
        "the corrupt row must be skipped and the valid row must still load"
    );

    // Drop the tampered row so subsequent test runs do not see it.
    sqlx::query("DELETE FROM standalone_usage_summaries WHERE recorded_at = ?")
        .bind("not-a-timestamp")
        .execute(&pool)
        .await
        .expect("clean tampered row");
    cleanup(store, path).await;
}

#[tokio::test]
async fn list_usage_summaries_skips_malformed_uuid_and_keeps_valid_rows() {
    let (store, path) = open_store().await;
    let first = sample_usage_record(Uuid::new_v4(), "/v1/a", Some("a"));
    let second = sample_usage_record(Uuid::new_v4(), "/v1/b", Some("b"));
    let first_request_id = first.request_id;
    let second_request_id = second.request_id;
    store
        .insert_usage_summary(&first)
        .await
        .expect("insert first row");
    store
        .insert_usage_summary(&second)
        .await
        .expect("insert second row");

    let pool = store.pool().clone();
    sqlx::query("UPDATE standalone_usage_summaries SET endpoint_id = ? WHERE request_id = ?")
        .bind("not-a-uuid")
        .bind(second_request_id.to_string())
        .execute(&pool)
        .await
        .expect("tamper endpoint_id");

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list tolerates corrupt UUID row");
    let loaded_ids: Vec<Uuid> = stored.iter().map(|record| record.request_id).collect();
    assert!(
        loaded_ids.contains(&first_request_id),
        "first valid row must survive the malformed second row"
    );
    assert!(
        !loaded_ids.contains(&second_request_id),
        "second row with malformed endpoint_id must be skipped"
    );

    // Drop the tampered row so the cleanup path can succeed.
    sqlx::query("DELETE FROM standalone_usage_summaries WHERE endpoint_id = ?")
        .bind("not-a-uuid")
        .execute(&pool)
        .await
        .expect("clean tampered row");
    cleanup(store, path).await;
}
