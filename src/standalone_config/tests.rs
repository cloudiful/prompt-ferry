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
            api_key: "endpoint-secret".to_string(),
            api_keys: vec![EndpointApiKeyConfig {
                key_id,
                key_label: "primary".to_string(),
                api_key: "endpoint-key-secret".to_string(),
                position: 0,
                enabled: true,
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
    invalid.endpoints[0].api_keys.push(EndpointApiKeyConfig {
        key_id: Uuid::new_v4(),
        key_label: "primary".to_string(),
        api_key: "duplicate-label-secret".to_string(),
        position: 1,
        enabled: true,
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
    assert_eq!(version, 3);

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
