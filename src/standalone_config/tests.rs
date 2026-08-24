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
    assert_eq!(version, 9);

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
        user_id: None,
        client_key_id: None,
        client_key_label: None,
        request_user_agent: None,
        endpoint_key_label: None,
        mcp_server_name: None,
        mcp_protocol_method: None,
        mcp_operation_name: None,
        http_request_content_encoding: None,
        http_request_compressed: false,
        http_request_compressed_bytes: None,
        http_request_decompressed_bytes: None,
        http_request_compression_ratio: None,
        conversation_source: "none".to_string(),
        client_installation_id: None,
        provider_response_id: None,
        provider_conversation_key: None,
        request_storage_mode: "full".to_string(),
        error_message: None,
        request_has_previous_response_id: false,
        request_previous_response_id: None,
        request_previous_response_parent_found: None,
        request_conversation_key: None,
        request_conversation_parent_found: None,
        upstream_redaction_enabled: false,
        response_capture_truncated: false,
    }
}

#[tokio::test]
async fn fresh_migration_creates_empty_usage_ledger_at_schema_version_nine() {
    let (store, path) = open_store().await;
    let version = standalone_query!("src/sql/standalone/schema_version.sql")
        .fetch_one(store.pool())
        .await
        .expect("schema version")
        .try_get::<i64, _>("schema_version")
        .expect("version value");
    assert_eq!(version, 9);
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

// ---- Phase 1C-a standalone usage metadata tests ----

fn ai_metadata_record(request_id: Uuid) -> StandaloneUsageSummaryRecord {
    let mut record = sample_usage_record(request_id, "/v1/responses", Some("gpt-5"));
    record.user_id = Some(42);
    record.client_key_id = Some(7);
    record.client_key_label = Some("test client".to_string());
    record.request_user_agent = Some("prompt-ferry-cli/0.4".to_string());
    record.endpoint_key_label = Some("primary".to_string());
    record.http_request_content_encoding = Some("gzip".to_string());
    record.http_request_compressed = true;
    record.http_request_compressed_bytes = Some(2048);
    record.http_request_decompressed_bytes = Some(8192);
    record.http_request_compression_ratio = Some(0.25);
    record.conversation_source = "responses".to_string();
    record.client_installation_id = Some("install-abc".to_string());
    record.provider_response_id = Some("resp_001".to_string());
    record.provider_conversation_key = Some("conv-001".to_string());
    record.request_storage_mode = "summary".to_string();
    record.error_message = Some("upstream returned HTTP 502".to_string());
    record.request_has_previous_response_id = true;
    record.request_previous_response_id = Some("resp_000".to_string());
    record.request_previous_response_parent_found = Some(true);
    record.request_conversation_key = Some("conv-001".to_string());
    record.request_conversation_parent_found = Some(false);
    record.upstream_redaction_enabled = true;
    record.response_capture_truncated = true;
    record
}

fn mcp_metadata_record(request_id: Uuid) -> StandaloneUsageSummaryRecord {
    let mut record = sample_usage_record(request_id, "/mcp", None);
    record.category = "mcp".to_string();
    record.mcp_server_id = Some(Uuid::new_v4());
    record.mcp_server_name = Some("catalog".to_string());
    record.mcp_protocol_method = Some("tools/list".to_string());
    record.mcp_operation_name = Some("list_tools".to_string());
    record.request_storage_mode = "metadata_only".to_string();
    record.request_has_previous_response_id = false;
    record.upstream_redaction_enabled = false;
    record.response_capture_truncated = false;
    record
}

#[tokio::test]
async fn fresh_migration_creates_metadata_columns_at_schema_version_seven() {
    let (store, path) = open_store().await;
    let pool = store.pool().clone();
    // Inspect the columns added by migration 0007 are present and have the
    // expected SQLite types/defaults.
    for (column, declared_type) in [
        ("user_id", "INTEGER"),
        ("client_key_id", "INTEGER"),
        ("client_key_label", "TEXT"),
        ("request_user_agent", "TEXT"),
        ("endpoint_key_label", "TEXT"),
        ("mcp_server_name", "TEXT"),
        ("mcp_protocol_method", "TEXT"),
        ("mcp_operation_name", "TEXT"),
        ("http_request_content_encoding", "TEXT"),
        ("http_request_compressed", "INTEGER"),
        ("http_request_compressed_bytes", "INTEGER"),
        ("http_request_decompressed_bytes", "INTEGER"),
        ("http_request_compression_ratio", "REAL"),
        ("conversation_source", "TEXT"),
        ("client_installation_id", "TEXT"),
        ("provider_response_id", "TEXT"),
        ("provider_conversation_key", "TEXT"),
        ("request_storage_mode", "TEXT"),
        ("error_message", "TEXT"),
        ("request_has_previous_response_id", "INTEGER"),
        ("request_previous_response_id", "TEXT"),
        ("request_previous_response_parent_found", "INTEGER"),
        ("request_conversation_key", "TEXT"),
        ("request_conversation_parent_found", "INTEGER"),
        ("upstream_redaction_enabled", "INTEGER"),
        ("response_capture_truncated", "INTEGER"),
    ] {
        let row = sqlx::query(
            "SELECT type FROM pragma_table_info('standalone_usage_summaries') WHERE name = ?",
        )
        .bind(column)
        .fetch_optional(&pool)
        .await
        .expect("pragma")
        .unwrap_or_else(|| panic!("missing column {column}"));
        let actual_type: String = row.try_get("type").expect("type");
        assert_eq!(actual_type, declared_type, "column {column}");
    }
    cleanup(store, path).await;
}

#[tokio::test]
async fn upgrade_from_schema_six_adds_metadata_columns_and_keeps_existing_rows() {
    let path = database_path();
    let pool = crate::db::connect_sqlite(&path).await.expect("pool");
    // Apply the schema through migration 0006 (Phase 1A baseline).
    for (label, body) in [
        (
            "0001_initial.sql",
            include_str!("/workspace/tools/prompt-ferry/migrations/standalone/0001_initial.sql"),
        ),
        (
            "0002_storage_contract.sql",
            include_str!(
                "/workspace/tools/prompt-ferry/migrations/standalone/0002_storage_contract.sql"
            ),
        ),
        (
            "0003_user_auth_compatibility.sql",
            include_str!(
                "/workspace/tools/prompt-ferry/migrations/standalone/0003_user_auth_compatibility.sql"
            ),
        ),
        (
            "0004_coordinator_state.sql",
            include_str!(
                "/workspace/tools/prompt-ferry/migrations/standalone/0004_coordinator_state.sql"
            ),
        ),
        (
            "0005_mcp_configuration.sql",
            include_str!(
                "/workspace/tools/prompt-ferry/migrations/standalone/0005_mcp_configuration.sql"
            ),
        ),
        (
            "0006_request_ledger.sql",
            include_str!(
                "/workspace/tools/prompt-ferry/migrations/standalone/0006_request_ledger.sql"
            ),
        ),
    ] {
        sqlx::raw_sql(body).execute(&pool).await.expect(label);
    }

    // Insert a Phase 1A row whose Phase 1C-a metadata columns are NULL so
    // we can confirm the upgrade preserves it.
    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO standalone_usage_summaries(
             request_id, event_kind, category, state, path, recorded_at,
             redaction_applied, redaction_findings_count, redaction_replacements_count,
             redaction_types_json, redaction_fields_json, route_selection_reason
         ) VALUES (
             ?, 'request', 'ai', 'received', '/v1/legacy', '2026-08-24T00:00:00+00:00',
             0, 0, 0, '[]', '[]', 'default'
         )",
    )
    .bind(request_id.to_string())
    .execute(&pool)
    .await
    .expect("legacy insert");
    pool.close().await;

    let store = StandaloneConfigStore::open(&path).await.expect("upgrade");
    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list after upgrade");
    assert_eq!(stored.len(), 1, "Phase 1A row must survive migration");
    let legacy = &stored[0];
    assert_eq!(legacy.request_id, request_id);
    assert!(legacy.user_id.is_none());
    assert!(legacy.client_key_id.is_none());
    assert!(legacy.client_key_label.is_none());
    assert!(legacy.request_user_agent.is_none());
    assert!(legacy.endpoint_key_label.is_none());
    assert!(legacy.mcp_server_name.is_none());
    assert!(legacy.mcp_protocol_method.is_none());
    assert!(legacy.mcp_operation_name.is_none());
    assert!(legacy.http_request_content_encoding.is_none());
    assert!(!legacy.http_request_compressed);
    assert!(legacy.http_request_compressed_bytes.is_none());
    assert!(legacy.http_request_decompressed_bytes.is_none());
    assert!(legacy.http_request_compression_ratio.is_none());
    assert_eq!(legacy.conversation_source, "none");
    assert!(legacy.client_installation_id.is_none());
    assert!(legacy.provider_response_id.is_none());
    assert!(legacy.provider_conversation_key.is_none());
    assert_eq!(legacy.request_storage_mode, "full");
    assert!(legacy.error_message.is_none());
    assert!(!legacy.request_has_previous_response_id);
    assert!(legacy.request_previous_response_id.is_none());
    assert!(legacy.request_previous_response_parent_found.is_none());
    assert!(legacy.request_conversation_key.is_none());
    assert!(legacy.request_conversation_parent_found.is_none());
    assert!(!legacy.upstream_redaction_enabled);
    assert!(!legacy.response_capture_truncated);

    // Inserting a Phase 1C-a row alongside the legacy one must round-trip
    // the new metadata columns.
    let ai_request = Uuid::new_v4();
    store
        .insert_usage_summary(&ai_metadata_record(ai_request))
        .await
        .expect("ai metadata insert");
    let after = store.list_usage_summaries(64).await.expect("list");
    assert_eq!(after.len(), 2);
    let ai_row = after
        .iter()
        .find(|record| record.request_id == ai_request)
        .expect("ai row");
    assert_eq!(ai_row.user_id, Some(42));
    assert_eq!(ai_row.client_key_id, Some(7));
    assert_eq!(ai_row.client_key_label.as_deref(), Some("test client"));
    assert_eq!(
        ai_row.request_user_agent.as_deref(),
        Some("prompt-ferry-cli/0.4")
    );
    assert_eq!(ai_row.endpoint_key_label.as_deref(), Some("primary"));
    assert_eq!(
        ai_row.http_request_content_encoding.as_deref(),
        Some("gzip")
    );
    assert!(ai_row.http_request_compressed);
    assert_eq!(ai_row.http_request_compressed_bytes, Some(2048));
    assert_eq!(ai_row.http_request_decompressed_bytes, Some(8192));
    assert_eq!(ai_row.http_request_compression_ratio, Some(0.25));
    assert_eq!(ai_row.conversation_source, "responses");
    assert_eq!(
        ai_row.client_installation_id.as_deref(),
        Some("install-abc")
    );
    assert_eq!(ai_row.provider_response_id.as_deref(), Some("resp_001"));
    assert_eq!(
        ai_row.provider_conversation_key.as_deref(),
        Some("conv-001")
    );
    assert_eq!(ai_row.request_storage_mode, "summary");
    assert_eq!(
        ai_row.error_message.as_deref(),
        Some("upstream returned HTTP 502")
    );
    assert!(ai_row.request_has_previous_response_id);
    assert_eq!(
        ai_row.request_previous_response_id.as_deref(),
        Some("resp_000")
    );
    assert_eq!(ai_row.request_previous_response_parent_found, Some(true));
    assert_eq!(ai_row.request_conversation_key.as_deref(), Some("conv-001"));
    assert_eq!(ai_row.request_conversation_parent_found, Some(false));
    assert!(ai_row.upstream_redaction_enabled);
    assert!(ai_row.response_capture_truncated);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn metadata_round_trip_preserves_ai_and_mcp_values() {
    let (store, path) = open_store().await;
    let ai = ai_metadata_record(Uuid::new_v4());
    let mcp = mcp_metadata_record(Uuid::new_v4());
    let ai_request_id = ai.request_id;
    let mcp_request_id = mcp.request_id;

    store.insert_usage_summary(&ai).await.expect("ai insert");
    store.insert_usage_summary(&mcp).await.expect("mcp insert");

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list summaries");
    let ai_row = stored
        .iter()
        .find(|record| record.request_id == ai_request_id)
        .expect("ai row");
    assert_eq!(ai_row.user_id, Some(42));
    assert_eq!(ai_row.client_key_id, Some(7));
    assert_eq!(ai_row.endpoint_key_label.as_deref(), Some("primary"));
    assert!(ai_row.http_request_compressed);
    assert_eq!(ai_row.http_request_compressed_bytes, Some(2048));
    assert_eq!(ai_row.http_request_decompressed_bytes, Some(8192));
    assert_eq!(ai_row.http_request_compression_ratio, Some(0.25));
    assert_eq!(ai_row.conversation_source, "responses");
    assert!(ai_row.request_has_previous_response_id);
    assert!(ai_row.upstream_redaction_enabled);
    assert!(ai_row.response_capture_truncated);
    assert_eq!(ai_row.request_previous_response_parent_found, Some(true));

    let mcp_row = stored
        .iter()
        .find(|record| record.request_id == mcp_request_id)
        .expect("mcp row");
    assert_eq!(mcp_row.category, "mcp");
    assert_eq!(mcp_row.mcp_server_name.as_deref(), Some("catalog"));
    assert_eq!(mcp_row.mcp_protocol_method.as_deref(), Some("tools/list"));
    assert_eq!(mcp_row.mcp_operation_name.as_deref(), Some("list_tools"));
    assert_eq!(mcp_row.request_storage_mode, "metadata_only");
    assert!(!mcp_row.request_has_previous_response_id);
    assert!(!mcp_row.upstream_redaction_enabled);
    assert!(!mcp_row.response_capture_truncated);
    cleanup(store, path).await;
}

#[tokio::test]
async fn metadata_boolean_and_null_columns_round_trip_via_storage() {
    let (store, path) = open_store().await;
    let mut record = sample_usage_record(Uuid::new_v4(), "/v1/responses", Some("gpt-5"));
    record.http_request_compressed = true;
    record.request_has_previous_response_id = true;
    record.upstream_redaction_enabled = true;
    record.response_capture_truncated = true;
    record.request_previous_response_parent_found = Some(true);
    record.request_conversation_parent_found = Some(false);
    record.request_id = Uuid::new_v4();

    store.insert_usage_summary(&record).await.expect("insert");
    let stored = store.list_usage_summaries(64).await.expect("list");
    let row = stored
        .iter()
        .find(|r| r.request_id == record.request_id)
        .expect("row");
    assert!(row.http_request_compressed);
    assert!(row.request_has_previous_response_id);
    assert!(row.upstream_redaction_enabled);
    assert!(row.response_capture_truncated);
    assert_eq!(row.request_previous_response_parent_found, Some(true));
    assert_eq!(row.request_conversation_parent_found, Some(false));

    // Confirm the underlying integer storage is exactly 0/1, not arbitrary
    // values, so a downstream parse still recognizes the booleans.
    let pool = store.pool().clone();
    let raw = sqlx::query(
        "SELECT http_request_compressed,
                request_has_previous_response_id,
                upstream_redaction_enabled,
                response_capture_truncated,
                request_previous_response_parent_found,
                request_conversation_parent_found
         FROM standalone_usage_summaries
         WHERE request_id = ?",
    )
    .bind(record.request_id.to_string())
    .fetch_one(&pool)
    .await
    .expect("raw row");
    let http_compressed: i64 = raw.try_get("http_request_compressed").expect("int");
    let has_previous: i64 = raw
        .try_get("request_has_previous_response_id")
        .expect("int");
    let upstream_enabled: i64 = raw.try_get("upstream_redaction_enabled").expect("int");
    let capture_truncated: i64 = raw.try_get("response_capture_truncated").expect("int");
    let parent_found: Option<i64> = raw
        .try_get("request_previous_response_parent_found")
        .expect("opt int");
    let conv_parent_found: Option<i64> = raw
        .try_get("request_conversation_parent_found")
        .expect("opt int");
    assert_eq!(http_compressed, 1);
    assert_eq!(has_previous, 1);
    assert_eq!(upstream_enabled, 1);
    assert_eq!(capture_truncated, 1);
    assert_eq!(parent_found, Some(1));
    assert_eq!(conv_parent_found, Some(0));

    // Verify the opposite boolean state (false / None) is also stored
    // correctly. The CHECK constraints enforced at the schema layer mean a
    // non 0/1 integer cannot land in the table in the first place; the
    // parser-side validation is a defensive backup exercised by the
    // dedicated malformed-row tests in this module.
    let mut no_metadata = sample_usage_record(Uuid::new_v4(), "/v1/no", Some("gpt-5"));
    no_metadata.request_previous_response_parent_found = None;
    no_metadata.request_conversation_parent_found = None;
    store
        .insert_usage_summary(&no_metadata)
        .await
        .expect("insert no metadata");
    let no_stored = store
        .list_usage_summaries(64)
        .await
        .expect("list no metadata");
    let no_row = no_stored
        .iter()
        .find(|r| r.request_id == no_metadata.request_id)
        .expect("no row");
    assert!(!no_row.http_request_compressed);
    assert!(!no_row.request_has_previous_response_id);
    assert!(!no_row.upstream_redaction_enabled);
    assert!(!no_row.response_capture_truncated);
    assert_eq!(no_row.request_previous_response_parent_found, None);
    assert_eq!(no_row.request_conversation_parent_found, None);
    cleanup(store, path).await;
}

#[tokio::test]
async fn repeated_lifecycle_events_for_one_request_retain_phase_1c_a_metadata() {
    let (store, path) = open_store().await;
    let request_id = Uuid::new_v4();

    let mut initial = ai_metadata_record(request_id);
    initial.state = "received".to_string();

    let mut terminal = ai_metadata_record(request_id);
    terminal.state = "failed".to_string();
    terminal.error_code = Some("upstream_error".to_string());
    terminal.error_message = Some("upstream returned HTTP 502".to_string());
    terminal.status = Some(502);
    terminal.ok = Some(false);
    terminal.request_has_previous_response_id = false;

    let first_event_id = store
        .insert_usage_summary(&initial)
        .await
        .expect("initial insert");
    let second_event_id = store
        .insert_usage_summary(&terminal)
        .await
        .expect("terminal insert");
    assert!(second_event_id > first_event_id);

    let stored = store
        .list_usage_summaries(64)
        .await
        .expect("list summaries");
    let for_request: Vec<&StandaloneUsageSummaryRecord> = stored
        .iter()
        .filter(|record| record.request_id == request_id)
        .collect();
    assert_eq!(for_request.len(), 2);
    assert_eq!(for_request[0].state, "received");
    assert_eq!(for_request[1].state, "failed");
    // The metadata fields added in Phase 1C-a must persist on both rows.
    for row in &for_request {
        assert_eq!(row.user_id, Some(42));
        assert_eq!(row.client_key_id, Some(7));
        assert_eq!(row.client_key_label.as_deref(), Some("test client"));
        assert_eq!(
            row.request_user_agent.as_deref(),
            Some("prompt-ferry-cli/0.4")
        );
        assert_eq!(row.endpoint_key_label.as_deref(), Some("primary"));
        assert!(row.http_request_compressed);
        assert_eq!(row.http_request_compressed_bytes, Some(2048));
        assert_eq!(row.conversation_source, "responses");
        assert_eq!(row.client_installation_id.as_deref(), Some("install-abc"));
        assert_eq!(row.provider_response_id.as_deref(), Some("resp_001"));
        assert!(row.upstream_redaction_enabled);
        assert!(row.response_capture_truncated);
    }
    // The terminal event independently overrides request_has_previous_response_id
    // so the lifecycle distinction is preserved.
    assert!(for_request[0].request_has_previous_response_id);
    assert!(!for_request[1].request_has_previous_response_id);
    assert_eq!(
        for_request[1].error_message.as_deref(),
        Some("upstream returned HTTP 502")
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn metadata_round_trips_after_reopen_and_hydration() {
    let (store, path) = open_store().await;
    let request_id = Uuid::new_v4();
    store
        .insert_usage_summary(&ai_metadata_record(request_id))
        .await
        .expect("insert");
    store.close().await;

    let reopened = StandaloneConfigStore::open(&path).await.expect("reopen");
    let stored = reopened
        .list_usage_summaries(64)
        .await
        .expect("list after reopen");
    assert_eq!(stored.len(), 1);
    let row = &stored[0];
    assert_eq!(row.request_id, request_id);
    assert_eq!(row.user_id, Some(42));
    assert_eq!(row.client_key_id, Some(7));
    assert_eq!(row.client_key_label.as_deref(), Some("test client"));
    assert_eq!(
        row.request_user_agent.as_deref(),
        Some("prompt-ferry-cli/0.4")
    );
    assert_eq!(row.endpoint_key_label.as_deref(), Some("primary"));
    assert!(row.http_request_compressed);
    assert_eq!(row.http_request_compressed_bytes, Some(2048));
    assert_eq!(row.http_request_decompressed_bytes, Some(8192));
    assert_eq!(row.http_request_compression_ratio, Some(0.25));
    assert_eq!(row.conversation_source, "responses");
    assert_eq!(row.client_installation_id.as_deref(), Some("install-abc"));
    assert_eq!(row.provider_response_id.as_deref(), Some("resp_001"));
    assert_eq!(row.provider_conversation_key.as_deref(), Some("conv-001"));
    assert_eq!(row.request_storage_mode, "summary");
    assert_eq!(
        row.error_message.as_deref(),
        Some("upstream returned HTTP 502")
    );
    assert!(row.request_has_previous_response_id);
    assert_eq!(
        row.request_previous_response_id.as_deref(),
        Some("resp_000")
    );
    assert_eq!(row.request_previous_response_parent_found, Some(true));
    assert_eq!(row.request_conversation_key.as_deref(), Some("conv-001"));
    assert_eq!(row.request_conversation_parent_found, Some(false));
    assert!(row.upstream_redaction_enabled);
    assert!(row.response_capture_truncated);
    cleanup(reopened, path).await;
}

#[tokio::test]
async fn metadata_record_debug_output_omits_raw_body_and_secret_field_names() {
    let record = ai_metadata_record(Uuid::new_v4());
    let debug = format!("{record:?}");
    for forbidden in [
        "request_body",
        "response_body",
        "raw_body",
        "session_secret",
        "request_raw_json",
        "response_raw_body",
        "upstream_redacted_request_json",
    ] {
        assert!(
            !debug.contains(forbidden),
            "metadata record debug output must not contain {forbidden:?}; got: {debug}"
        );
    }
    // The Phase 1C-a metadata fields must be visible in the debug output so
    // operators can inspect them.
    for visible in [
        "user_id",
        "client_key_id",
        "client_key_label",
        "request_user_agent",
        "endpoint_key_label",
        "mcp_server_name",
        "mcp_protocol_method",
        "mcp_operation_name",
        "http_request_content_encoding",
        "http_request_compressed",
        "http_request_compressed_bytes",
        "http_request_decompressed_bytes",
        "http_request_compression_ratio",
        "conversation_source",
        "client_installation_id",
        "provider_response_id",
        "provider_conversation_key",
        "request_storage_mode",
        "error_message",
        "request_has_previous_response_id",
        "request_previous_response_id",
        "request_conversation_key",
        "upstream_redaction_enabled",
        "response_capture_truncated",
    ] {
        assert!(
            debug.contains(visible),
            "metadata record debug output must contain {visible:?}"
        );
    }
}

// ---- Phase 1C-b standalone replay snapshot tests ----

fn sample_prompt_refs_json(entries: &[(&str, &str)]) -> String {
    let array = entries
        .iter()
        .map(|(role, block_hash)| serde_json::json!({"role": role, "block_hash": block_hash}))
        .collect::<Vec<_>>();
    serde_json::to_string(&array).expect("serialize refs")
}

fn sample_snapshot(
    conversation_id: Uuid,
    conversation_seq: i32,
    base_event_id: i64,
    refs_json: &str,
) -> StandaloneReplaySnapshotRecord {
    let refs_value: serde_json::Value = serde_json::from_str(refs_json).expect("refs JSON");
    let ref_count = refs_value.as_array().map(|array| array.len()).unwrap_or(0) as i32;
    let byte_size = refs_json.len() as i32;
    StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id,
        conversation_seq,
        prompt_refs_json: refs_json.to_string(),
        ref_count,
        byte_size,
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn fresh_migration_creates_replay_snapshot_table_at_schema_version_nine() {
    let (store, path) = open_store().await;
    let version = standalone_query!("src/sql/standalone/schema_version.sql")
        .fetch_one(store.pool())
        .await
        .expect("schema version")
        .try_get::<i64, _>("schema_version")
        .expect("version value");
    assert_eq!(version, 9);
    let pool = store.pool().clone();
    for (column, declared_type) in [
        ("conversation_id", "TEXT"),
        ("base_event_id", "INTEGER"),
        ("conversation_seq", "INTEGER"),
        ("prompt_refs_json", "TEXT"),
        ("ref_count", "INTEGER"),
        ("byte_size", "INTEGER"),
        ("updated_at", "TEXT"),
    ] {
        let row = sqlx::query(
            "SELECT type FROM pragma_table_info('standalone_replay_snapshots') WHERE name = ?",
        )
        .bind(column)
        .fetch_optional(&pool)
        .await
        .expect("pragma")
        .unwrap_or_else(|| panic!("missing column {column}"));
        let actual_type: String = row.try_get("type").expect("type");
        assert_eq!(actual_type, declared_type, "column {column}");
    }
    assert!(
        store
            .get_replay_snapshot(Uuid::new_v4())
            .await
            .expect("get missing")
            .is_none(),
        "fresh database must not have any replay snapshots"
    );
    cleanup(store, path).await;
}

#[tokio::test]
async fn upgrade_from_schema_eight_creates_request_lease_table() {
    use sha2::{Digest, Sha384};
    let path = database_path();
    let pool = crate::db::connect_sqlite(&path).await.expect("pool");
    // Apply the baseline schema via raw SQL so the migrator applies
    // only migration 0009 when the store is opened. Migration 0007
    // contains `ALTER TABLE ... ADD COLUMN` statements which fail on
    // re-run, so we mark 0001..=0008 as already-applied in the SQLx
    // migrator's tracking table after the raw SQL executes. The
    // checksum matches the SHA384 digest of each migration's SQL text,
    // matching `sqlx_core::migrate::migration::checksum`.
    let applied: &[(&str, &str)] = &[
        (
            "0001_initial",
            include_str!("../../migrations/standalone/0001_initial.sql"),
        ),
        (
            "0002_storage_contract",
            include_str!("../../migrations/standalone/0002_storage_contract.sql"),
        ),
        (
            "0003_user_auth_compatibility",
            include_str!("../../migrations/standalone/0003_user_auth_compatibility.sql"),
        ),
        (
            "0004_coordinator_state",
            include_str!("../../migrations/standalone/0004_coordinator_state.sql"),
        ),
        (
            "0005_mcp_configuration",
            include_str!("../../migrations/standalone/0005_mcp_configuration.sql"),
        ),
        (
            "0006_request_ledger",
            include_str!("../../migrations/standalone/0006_request_ledger.sql"),
        ),
        (
            "0007_request_metadata",
            include_str!("../../migrations/standalone/0007_request_metadata.sql"),
        ),
        (
            "0008_replay_snapshots",
            include_str!("../../migrations/standalone/0008_replay_snapshots.sql"),
        ),
    ];
    for (version, body) in applied {
        sqlx::raw_sql(*body)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("{version}: {error}"));
    }
    // Create the SQLx migrations tracking table so we can record the
    // already-applied 0001..=0008 migrations. Mirroring the migrator's
    // own `ensure_migrations_table` keeps the test independent of the
    // SQLx version's internal CREATE TABLE statement.
    sqlx::raw_sql(
        r#"CREATE TABLE IF NOT EXISTS _sqlx_migrations (
            version BIGINT PRIMARY KEY,
            description TEXT NOT NULL,
            installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            success BOOLEAN NOT NULL,
            checksum BLOB NOT NULL,
            execution_time BIGINT NOT NULL
        )"#,
    )
    .execute(&pool)
    .await
    .expect("create sqlx migrations table");
    for (index, (version, body)) in applied.iter().enumerate() {
        let checksum = Sha384::digest(body.as_bytes()).to_vec();
        sqlx::query(
            "INSERT INTO _sqlx_migrations(version, description, success, checksum, execution_time) VALUES (?, ?, 1, ?, 0)",
        )
        .bind(i64::try_from(index + 1).expect("version fits i64"))
        .bind(*version)
        .bind(checksum)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("{version} record: {error}"));
    }
    pool.close().await;

    let store = StandaloneConfigStore::open(&path).await.expect("upgrade");
    let version = standalone_query!("src/sql/standalone/schema_version.sql")
        .fetch_one(store.pool())
        .await
        .expect("schema version")
        .try_get::<i64, _>("schema_version")
        .expect("version value");
    assert_eq!(version, 9);

    // Confirm migration 0008 took effect before the new lease table
    // arrived so the test really exercises the schema-8 -> schema-9
    // boundary.
    let pool = store.pool().clone();
    let replay_table_exists: i64 = sqlx::query(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='standalone_replay_snapshots')",
    )
    .fetch_one(&pool)
    .await
    .expect("replay pragma")
    .try_get(0)
    .expect("pragma value");
    assert_eq!(replay_table_exists, 1, "schema 8 replay table must exist");

    // The new table must be empty and writable without disturbing
    // pre-existing schema-8 rows.
    let leases = crate::standalone_config::StandaloneRequestLeaseStore::new(pool.clone());
    let request_id = Uuid::new_v4();
    let owner = Uuid::new_v4();
    assert_eq!(
        leases
            .acquire(request_id, owner, 60)
            .await
            .expect("acquire lease"),
        crate::standalone_config::RequestLeaseAcquireOutcome::Acquired
    );
    let active = leases.list_active().await.expect("list");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].request_id, request_id);
    assert_eq!(active[0].owner_worker_id, owner);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn replay_snapshot_first_insert_then_higher_sequence_replaces() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();
    let first_refs = sample_prompt_refs_json(&[("user", "hash-1")]);
    let first = sample_snapshot(conversation_id, 1, 10, &first_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&first)
            .await
            .expect("first upsert"),
        ReplaySnapshotUpsertOutcome::Inserted
    );

    let higher_refs = sample_prompt_refs_json(&[
        ("user", "hash-1"),
        ("assistant", "hash-2"),
        ("user", "hash-3"),
    ]);
    let higher = sample_snapshot(conversation_id, 2, 11, &higher_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&higher)
            .await
            .expect("higher upsert"),
        ReplaySnapshotUpsertOutcome::Updated
    );

    let loaded = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get")
        .expect("snapshot");
    assert_eq!(loaded.conversation_seq, 2);
    assert_eq!(loaded.base_event_id, 11);
    assert_eq!(loaded.ref_count, 3);
    assert_eq!(loaded.prompt_refs_json, higher_refs);
    cleanup(store, path).await;
}

#[tokio::test]
async fn replay_snapshot_lower_or_equal_sequence_does_not_regress() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();
    let initial_refs = sample_prompt_refs_json(&[("user", "hash-1"), ("assistant", "hash-2")]);
    let initial = sample_snapshot(conversation_id, 5, 100, &initial_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&initial)
            .await
            .expect("initial"),
        ReplaySnapshotUpsertOutcome::Inserted
    );

    // Lower sequence must be rejected as a non-regression.
    let lower_refs = sample_prompt_refs_json(&[("user", "hash-stale")]);
    let lower = sample_snapshot(conversation_id, 4, 200, &lower_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&lower)
            .await
            .expect("lower upsert"),
        ReplaySnapshotUpsertOutcome::Skipped
    );
    let loaded = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get")
        .expect("snapshot");
    assert_eq!(
        loaded.conversation_seq, 5,
        "stored sequence must not regress"
    );
    assert_eq!(
        loaded.base_event_id, 100,
        "stored base_event_id must not regress"
    );
    assert_eq!(
        loaded.prompt_refs_json, initial_refs,
        "stored refs must not be overwritten by an older snapshot"
    );

    // Equal sequence with a lower base_event_id must also be rejected.
    let same_seq_older = sample_snapshot(conversation_id, 5, 99, &lower_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&same_seq_older)
            .await
            .expect("same seq older"),
        ReplaySnapshotUpsertOutcome::Skipped
    );
    let loaded = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get")
        .expect("snapshot");
    assert_eq!(loaded.base_event_id, 100);
    assert_eq!(loaded.prompt_refs_json, initial_refs);
    cleanup(store, path).await;
}

#[tokio::test]
async fn replay_snapshot_same_sequence_higher_event_id_replaces() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();
    let first_refs = sample_prompt_refs_json(&[("user", "hash-1")]);
    let first = sample_snapshot(conversation_id, 7, 50, &first_refs);
    assert_eq!(
        store.upsert_replay_snapshot(&first).await.expect("first"),
        ReplaySnapshotUpsertOutcome::Inserted
    );

    let newer_refs = sample_prompt_refs_json(&[("user", "hash-2"), ("assistant", "hash-3")]);
    let newer = sample_snapshot(conversation_id, 7, 75, &newer_refs);
    assert_eq!(
        store
            .upsert_replay_snapshot(&newer)
            .await
            .expect("same seq newer"),
        ReplaySnapshotUpsertOutcome::Updated
    );
    let loaded = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get")
        .expect("snapshot");
    assert_eq!(loaded.conversation_seq, 7);
    assert_eq!(loaded.base_event_id, 75);
    assert_eq!(loaded.ref_count, 2);
    assert_eq!(loaded.prompt_refs_json, newer_refs);
    cleanup(store, path).await;
}

#[tokio::test]
async fn replay_snapshot_rejects_invalid_input_payloads() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();

    // Empty prompt refs JSON must be rejected so callers cannot insert
    // a row that could not be reconstructed into typed refs. We build
    // the DTO directly here because `sample_snapshot` requires valid
    // JSON to compute its counters.
    let empty = StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id: 1,
        conversation_seq: 1,
        prompt_refs_json: "[]".to_string(),
        ref_count: 0,
        byte_size: 2,
        updated_at: chrono::Utc::now(),
    };
    let error = store
        .upsert_replay_snapshot(&empty)
        .await
        .expect_err("empty refs must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            ..
        }
    ));

    // Non-array payload must be rejected.
    let object_payload = StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id: 1,
        conversation_seq: 1,
        prompt_refs_json: "{}".to_string(),
        ref_count: 0,
        byte_size: 2,
        updated_at: chrono::Utc::now(),
    };
    let error = store
        .upsert_replay_snapshot(&object_payload)
        .await
        .expect_err("non-array refs must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            ..
        }
    ));

    // Malformed JSON must surface as an invalid input error so callers
    // can warn and continue without poisoning the durable row.
    let malformed = StandaloneReplaySnapshotRecord {
        conversation_id,
        base_event_id: 1,
        conversation_seq: 1,
        prompt_refs_json: "not json".to_string(),
        ref_count: 0,
        byte_size: 8,
        updated_at: chrono::Utc::now(),
    };
    let error = store
        .upsert_replay_snapshot(&malformed)
        .await
        .expect_err("malformed refs must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "prompt_refs_json",
            ..
        }
    ));

    // Negative counters must be rejected at the storage boundary.
    let valid_refs = sample_prompt_refs_json(&[("user", "hash-1")]);
    let mut negative_refs = sample_snapshot(conversation_id, 1, 1, &valid_refs);
    negative_refs.ref_count = -1;
    let error = store
        .upsert_replay_snapshot(&negative_refs)
        .await
        .expect_err("negative ref_count must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "ref_count",
            ..
        }
    ));

    let mut negative_bytes = sample_snapshot(conversation_id, 1, 1, &valid_refs);
    negative_bytes.byte_size = -1;
    let error = store
        .upsert_replay_snapshot(&negative_bytes)
        .await
        .expect_err("negative byte_size must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "byte_size",
            ..
        }
    ));

    // Non-positive conversation_seq must be rejected.
    let mut zero_seq = sample_snapshot(conversation_id, 0, 1, &valid_refs);
    zero_seq.byte_size = valid_refs.len() as i32;
    let error = store
        .upsert_replay_snapshot(&zero_seq)
        .await
        .expect_err("zero seq must fail");
    assert!(matches!(
        error,
        StandaloneConfigError::InvalidInput {
            field: "conversation_seq",
            ..
        }
    ));

    // Confirm no row was written by any of the rejected attempts.
    let loaded = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get");
    assert!(loaded.is_none());
    cleanup(store, path).await;
}

#[tokio::test]
async fn replay_snapshot_get_corrupt_row_reports_corrupt_database() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();
    let refs_json = sample_prompt_refs_json(&[("user", "hash-1")]);
    let snapshot = sample_snapshot(conversation_id, 1, 1, &refs_json);
    store
        .upsert_replay_snapshot(&snapshot)
        .await
        .expect("upsert");

    // Tamper with the row directly so the next read sees an unparseable
    // prompt refs JSON; the read path must surface it as a corruption
    // error rather than silently dropping the row.
    let pool = store.pool().clone();
    sqlx::query(
        "UPDATE standalone_replay_snapshots SET prompt_refs_json = ? WHERE conversation_id = ?",
    )
    .bind("not-json")
    .bind(conversation_id.to_string())
    .execute(&pool)
    .await
    .expect("tamper refs");

    let error = store
        .get_replay_snapshot(conversation_id)
        .await
        .expect_err("corrupt row must error");
    assert!(matches!(error, StandaloneConfigError::CorruptDatabase(_)));
    cleanup(store, path).await;
}

#[tokio::test]
async fn replay_snapshot_persists_across_reopen_and_raw_body_is_excluded() {
    let (store, path) = open_store().await;
    let conversation_id = Uuid::new_v4();
    let refs_json = sample_prompt_refs_json(&[
        ("system", "system-hash"),
        ("user", "user-hash"),
        ("assistant", "assistant-hash"),
    ]);
    let snapshot = sample_snapshot(conversation_id, 9, 42, &refs_json);
    assert_eq!(
        store
            .upsert_replay_snapshot(&snapshot)
            .await
            .expect("first"),
        ReplaySnapshotUpsertOutcome::Inserted
    );
    store.close().await;

    let reopened = StandaloneConfigStore::open(&path).await.expect("reopen");
    let loaded = reopened
        .get_replay_snapshot(conversation_id)
        .await
        .expect("get after reopen")
        .expect("snapshot after reopen");
    assert_eq!(loaded.conversation_id, conversation_id);
    assert_eq!(loaded.conversation_seq, 9);
    assert_eq!(loaded.base_event_id, 42);
    assert_eq!(loaded.ref_count, 3);
    assert_eq!(loaded.byte_size, refs_json.len() as i32);
    assert_eq!(loaded.prompt_refs_json, refs_json);

    // The DTO debug output must not mention raw body, secret, or
    // session fields; the only string data on this DTO is the
    // serialized prompt refs JSON.
    let debug = format!("{loaded:?}");
    for forbidden in [
        "request_body",
        "response_body",
        "raw_body",
        "session_secret",
        "request_raw_json",
        "response_raw_body",
        "upstream_redacted_request_json",
        "upstream_restore_session",
    ] {
        assert!(
            !debug.contains(forbidden),
            "snapshot debug must not contain {forbidden:?}; got: {debug}"
        );
    }
    // Required fields must be visible in the debug output.
    for visible in [
        "conversation_id",
        "base_event_id",
        "conversation_seq",
        "prompt_refs_json",
        "ref_count",
        "byte_size",
        "updated_at",
    ] {
        assert!(
            debug.contains(visible),
            "snapshot debug must contain {visible:?}"
        );
    }
    cleanup(reopened, path).await;
}
