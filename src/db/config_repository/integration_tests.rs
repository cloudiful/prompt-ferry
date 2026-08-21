use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    config::{NativeApi, NativeApiSource as DbNativeApiSource},
    db::config_repository::ConfigRepository,
    db::{
        EndpointCreate, EndpointProvider, ModelEndpointRuleCreate, ModelRouteRoutingStrategy,
        ModelRouteTargetCreate, ResponsesContinuationPolicy,
    },
    keys::hash_client_key,
    protocol::RelayIpPolicy,
    relay_secrets::RelaySecretManager,
    standalone_config::StandaloneConfigStore,
};

fn master_key() -> RelaySecretManager {
    RelaySecretManager::from_base64(&STANDARD.encode([3_u8; 32])).expect("master key")
}

async fn open_repository() -> (
    Arc<StandaloneConfigStore>,
    RelaySecretManager,
    std::path::PathBuf,
) {
    let path = std::env::temp_dir().join(format!("prompt-ferry-repo-{}.sqlite", Uuid::new_v4()));
    let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
    let manager = master_key();
    crate::db::migrate_standalone(store.pool())
        .await
        .expect("migrations");
    (store, manager, path)
}

async fn close_repository(store: Arc<StandaloneConfigStore>, path: std::path::PathBuf) {
    let pool = store.pool().clone();
    drop(store);
    pool.close().await;
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn endpoint_crud_round_trips_with_encrypted_secret() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());

    let input = EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "primary upstream".to_string(),
        provider: EndpointProvider::Generic,
        provider_region: None,
        base_url: "https://upstream.example".to_string(),
        native_api: NativeApi::Chat,
        native_api_source: DbNativeApiSource::Manual,
        daily_max_requests: Some(100),
        monthly_max_requests: Some(1_000),
        api_key: "primary-secret".to_string(),
        api_keys: vec![crate::db::EndpointApiKeyCreate {
            key_label: "primary".to_string(),
            api_key: "primary-secret".to_string(),
            position: 0,
            enabled: true,
            key_id: None,
        }],
        key_lb_enabled: true,
        enabled: true,
    };
    let endpoint_id = Uuid::new_v4();
    let created = repo
        .create_endpoint(endpoint_id, input, false)
        .await
        .expect("create endpoint");
    assert_eq!(created.api_keys.len(), 1);
    assert_eq!(created.api_keys[0].key_label, "primary");

    // API key secret must not leak into the SQLite store as plaintext.
    let pool = store.pool();
    let row = sqlx::query(
        "SELECT api_key_ciphertext FROM standalone_provider_endpoints WHERE endpoint_id = ?",
    )
    .bind(endpoint_id.to_string())
    .fetch_one(pool)
    .await
    .expect("row");
    let ciphertext: Vec<u8> = row.try_get("api_key_ciphertext").expect("ct");
    assert!(!String::from_utf8_lossy(&ciphertext).contains("primary-secret"));

    let secret = repo
        .first_endpoint_api_key(endpoint_id)
        .await
        .expect("first key");
    assert_eq!(secret.as_deref(), Some("primary-secret"));

    let page = repo
        .list_endpoints_page(0, 10)
        .await
        .expect("list endpoints");
    assert_eq!(page.endpoints.len(), 1);
    assert_eq!(page.endpoints[0].endpoint_id, endpoint_id);

    let updated_input = EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "renamed".to_string(),
        provider: EndpointProvider::Generic,
        provider_region: None,
        base_url: "https://upstream.example".to_string(),
        native_api: NativeApi::Chat,
        native_api_source: DbNativeApiSource::Manual,
        daily_max_requests: Some(100),
        monthly_max_requests: Some(1_000),
        api_key: "secondary-secret".to_string(),
        api_keys: vec![crate::db::EndpointApiKeyCreate {
            key_label: "secondary".to_string(),
            api_key: "secondary-secret".to_string(),
            position: 0,
            enabled: true,
            key_id: None,
        }],
        key_lb_enabled: true,
        enabled: true,
    };
    let updated = repo
        .update_endpoint(endpoint_id, updated_input)
        .await
        .expect("update endpoint")
        .expect("endpoint present");
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.api_keys[0].key_label, "secondary");

    let secret = repo
        .first_endpoint_api_key(endpoint_id)
        .await
        .expect("first key after update");
    assert_eq!(secret.as_deref(), Some("secondary-secret"));

    repo.delete_endpoint(endpoint_id)
        .await
        .expect("delete endpoint");
    let page = repo
        .list_endpoints_page(0, 10)
        .await
        .expect("list endpoints after delete");
    assert_eq!(page.endpoints.len(), 0);

    close_repository(store, path).await;
}

fn mcp_input() -> crate::db::McpServerInput {
    crate::db::McpServerInput {
        scope: "admin".to_string(),
        owner_user_id: None,
        source_endpoint_id: None,
        name: "local tools".to_string(),
        aggregate_naming_mode: "passthrough_preferred".to_string(),
        transport: "stdio".to_string(),
        url: None,
        command: Some("mcpd".to_string()),
        args: serde_json::json!(["--stdio"]),
        env_json: serde_json::json!({"MCP_SECRET": "stdio-secret"}),
        bearer_tokens_json: serde_json::json!([
            {"token": "bearer-secret", "enabled": true}
        ]),
        http_headers_json: serde_json::json!({"x-region": "local"}),
        tool_filter_mode: "blacklist".to_string(),
        allowed_tools: serde_json::json!([]),
        disabled_tools: serde_json::json!([]),
        disabled_resources: serde_json::json!([]),
        daily_max_requests: None,
        monthly_max_requests: None,
        enabled: true,
        timeout_ms: 30_000,
        lifecycle_policy: "auto".to_string(),
        lifecycle_manual_protocol_version: None,
    }
}

#[tokio::test]
async fn mcp_crud_projection_and_credentials_use_encrypted_sqlite_storage() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager);
    let server_id = Uuid::new_v4();

    let created = repo
        .create_mcp_server(server_id, mcp_input())
        .await
        .expect("create MCP server");
    assert_eq!(created.server_id, server_id);
    assert_eq!(created.bearer_tokens()[0].token, "bearer-secret");
    assert_eq!(created.env_json["MCP_SECRET"], "stdio-secret");

    let row = sqlx::query(
        "SELECT env_ciphertext, bearer_tokens_ciphertext FROM standalone_mcp_servers WHERE server_id = ?",
    )
    .bind(server_id.to_string())
    .fetch_one(store.pool())
    .await
    .expect("MCP row");
    for column in ["env_ciphertext", "bearer_tokens_ciphertext"] {
        let ciphertext: Vec<u8> = row.try_get(column).expect("ciphertext");
        let text = String::from_utf8_lossy(&ciphertext);
        assert!(!text.contains("stdio-secret"));
        assert!(!text.contains("bearer-secret"));
    }

    let snapshot = store
        .load_snapshot(repo.standalone_secret_manager().expect("SQLite manager"))
        .await
        .expect("load core snapshot");
    store
        .replace_snapshot(
            repo.standalone_secret_manager().expect("SQLite manager"),
            &snapshot,
        )
        .await
        .expect("replace core snapshot");
    assert!(
        repo.get_mcp_server(server_id)
            .await
            .expect("reload MCP server after snapshot replacement")
            .is_some()
    );

    let page = repo
        .list_mcp_servers_page(Some(1), true, 0, 10)
        .await
        .expect("list MCP servers");
    assert_eq!(page.0, 1);
    assert_eq!(page.1[0].name, "local tools");

    let credentials = repo
        .list_mcp_credentials(server_id)
        .await
        .expect("list MCP credentials");
    assert_eq!(credentials.len(), 1);
    let view = crate::db::McpCredentialView::from(credentials.into_iter().next().unwrap());
    assert!(!view.secret_preview.contains("bearer-secret"));

    let mut updated_input = mcp_input();
    updated_input.name = "renamed tools".to_string();
    let updated = repo
        .update_mcp_server(server_id, updated_input)
        .await
        .expect("update MCP server")
        .expect("MCP server present");
    assert_eq!(updated.name, "renamed tools");
    assert_eq!(updated.bearer_tokens()[0].token, "bearer-secret");

    assert!(
        repo.delete_mcp_server(server_id)
            .await
            .expect("delete MCP server")
    );
    assert!(
        repo.get_mcp_server(server_id)
            .await
            .expect("get deleted MCP server")
            .is_none()
    );
    close_repository(store, path).await;
}

#[tokio::test]
async fn sqlite_minimax_endpoint_creates_managed_mcp_projection() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager);
    let endpoint = repo
        .create_endpoint(
            Uuid::new_v4(),
            EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "MiniMax local".to_string(),
                provider: EndpointProvider::Minimax,
                provider_region: None,
                base_url: "https://api.minimax.example".to_string(),
                native_api: NativeApi::Chat,
                native_api_source: DbNativeApiSource::Manual,
                daily_max_requests: None,
                monthly_max_requests: None,
                api_key: "minimax-secret".to_string(),
                api_keys: vec![],
                key_lb_enabled: false,
                enabled: true,
            },
            true,
        )
        .await
        .expect("create MiniMax endpoint");
    let pg_endpoint = endpoint.clone().into_pg();
    repo.sync_minimax_mcp_server(&pg_endpoint, true)
        .await
        .expect("sync managed MCP projection");
    let managed = repo
        .get_mcp_server_by_source_endpoint(endpoint.endpoint_id)
        .await
        .expect("get managed MCP projection")
        .expect("managed MCP projection");
    assert_eq!(managed.transport, "builtin_minimax");
    assert!(managed.enabled);

    repo.set_endpoint_mcp_enabled(endpoint.endpoint_id, false)
        .await
        .expect("toggle endpoint MCP flag");
    assert!(
        repo.get_mcp_server_by_source_endpoint(endpoint.endpoint_id)
            .await
            .expect("reload MCP projection after endpoint toggle")
            .is_some()
    );

    repo.sync_minimax_mcp_server(&pg_endpoint, false)
        .await
        .expect("disable managed MCP projection");
    assert!(
        !repo
            .get_mcp_server_by_source_endpoint(endpoint.endpoint_id)
            .await
            .expect("reload managed MCP projection")
            .expect("managed MCP projection")
            .enabled
    );
    close_repository(store, path).await;
}

mod keep_tests;

#[tokio::test]
async fn model_route_crud_round_trips_with_target_persistence() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());

    let endpoint_id = Uuid::new_v4();
    let endpoint = EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: "route upstream".to_string(),
        provider: EndpointProvider::Generic,
        provider_region: None,
        base_url: "https://upstream.example".to_string(),
        native_api: NativeApi::Chat,
        native_api_source: DbNativeApiSource::Manual,
        daily_max_requests: None,
        monthly_max_requests: None,
        api_key: "secret".to_string(),
        api_keys: vec![],
        key_lb_enabled: false,
        enabled: true,
    };
    repo.create_endpoint(endpoint_id, endpoint, false)
        .await
        .expect("endpoint seed");

    let rule_id = Uuid::new_v4();
    let input = ModelEndpointRuleCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        model_pattern: "gpt-*".to_string(),
        routing_strategy: ModelRouteRoutingStrategy::ClientKeyRendezvous,
        daily_max_requests: None,
        monthly_max_requests: None,
        enabled: true,
        targets: vec![ModelRouteTargetCreate {
            endpoint_id,
            enabled: true,
            upstream_model: Some("gpt-4o-mini".to_string()),
            responses_continuation_policy: ResponsesContinuationPolicy::ForceReplay,
        }],
    };
    let rule = repo
        .create_model_route(rule_id, input)
        .await
        .expect("create route");
    assert_eq!(rule.targets.len(), 1);
    assert_eq!(rule.targets[0].endpoint_id, endpoint_id);
    assert_eq!(
        rule.targets[0].upstream_model.as_deref(),
        Some("gpt-4o-mini")
    );

    let page = repo
        .list_model_routes_page(0, 10)
        .await
        .expect("list routes");
    assert_eq!(page.routes.len(), 1);

    let fetched = repo
        .get_model_route(rule_id)
        .await
        .expect("get route")
        .expect("route present");
    assert_eq!(fetched.targets.len(), 1);

    repo.delete_model_route(rule_id)
        .await
        .expect("delete route");
    let page = repo
        .list_model_routes_page(0, 10)
        .await
        .expect("list routes after delete");
    assert_eq!(page.routes.len(), 0);

    close_repository(store, path).await;
}

#[tokio::test]
async fn managed_relay_crud_round_trips_with_encrypted_secrets() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());

    let relay_input = crate::db::ManagedRelayInput {
        name: "primary relay".to_string(),
        relay_url: "wss://127.0.0.1:8788/ws/worker".to_string(),
        enabled: true,
        tls_mode: crate::config::TlsMode::Mtls,
        bridge_encryption_mode: crate::config::BridgeEncryptionMode::Required,
        relay_ca: Some(manager.encrypt("CA PEM").expect("encrypt ca")),
        client_cert: Some(manager.encrypt("CLIENT CERT").expect("encrypt cert")),
        client_key: Some(manager.encrypt("CLIENT KEY").expect("encrypt key")),
        bridge_encryption_key: Some(manager.encrypt("BRIDGE KEY").expect("encrypt bridge")),
    };
    let relay = repo
        .create_managed_relay(relay_input)
        .await
        .expect("create relay");
    let relay_id = relay.relay_id;
    assert_eq!(relay.name, "primary relay");
    assert!(relay.has_relay_ca);
    assert!(relay.has_client_cert);
    assert!(relay.has_client_key);
    assert!(relay.has_bridge_key);

    let pool = store.pool();
    let row = sqlx::query("SELECT relay_ca_ciphertext, client_cert_ciphertext, client_key_ciphertext, bridge_encryption_key_ciphertext FROM standalone_relays WHERE relay_id = ?")
        .bind(relay_id.to_string())
        .fetch_one(pool)
        .await
        .expect("relay row");
    let ciphertexts: Vec<Vec<u8>> = vec![
        row.try_get("relay_ca_ciphertext").expect("ca ct"),
        row.try_get("client_cert_ciphertext").expect("cert ct"),
        row.try_get("client_key_ciphertext").expect("key ct"),
        row.try_get("bridge_encryption_key_ciphertext")
            .expect("bridge ct"),
    ];
    for plaintext in ["CA PEM", "CLIENT CERT", "CLIENT KEY", "BRIDGE KEY"] {
        for cipher in &ciphertexts {
            assert!(
                !String::from_utf8_lossy(cipher).contains(plaintext),
                "plaintext {plaintext:?} leaked into ciphertext",
            );
        }
    }

    let listed = repo
        .list_managed_relays_page(0, 10)
        .await
        .expect("list relays");
    assert_eq!(listed.0, 1);
    assert_eq!(listed.1, 1);
    assert_eq!(listed.2.len(), 1);

    let updated_input = crate::db::ManagedRelayInput {
        name: "primary relay (updated)".to_string(),
        relay_url: "ws://127.0.0.1:8788/ws/worker".to_string(),
        enabled: false,
        tls_mode: crate::config::TlsMode::Off,
        bridge_encryption_mode: crate::config::BridgeEncryptionMode::Off,
        relay_ca: None,
        client_cert: None,
        client_key: None,
        bridge_encryption_key: None,
    };
    let updated = repo
        .update_managed_relay(relay_id, updated_input)
        .await
        .expect("update relay")
        .expect("relay present");
    assert_eq!(updated.name, "primary relay (updated)");
    assert!(!updated.enabled);

    repo.delete_managed_relay(relay_id)
        .await
        .expect("delete relay");
    let listed = repo
        .list_managed_relays_page(0, 10)
        .await
        .expect("list relays after delete");
    assert_eq!(listed.0, 0);

    close_repository(store, path).await;
}

#[tokio::test]
async fn client_key_crud_round_trips_with_encrypted_secret() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());

    let created = repo
        .create_client_key(42, Some("local client"), true)
        .await
        .expect("create key");
    let secret = created.secret.clone();
    let key_uuid = created.key.key_id;
    assert!(secret.starts_with("pfy_"));
    assert_eq!(created.key.label, "local client");

    let pool = store.pool();
    let row = sqlx::query("SELECT secret_ciphertext FROM standalone_client_keys WHERE key_id = ?")
        .bind(key_uuid.to_string())
        .fetch_one(pool)
        .await
        .expect("client key row");
    let ciphertext: Vec<u8> = row.try_get("secret_ciphertext").expect("ct");
    assert!(!String::from_utf8_lossy(&ciphertext).contains(&secret));
    assert_eq!(hash_client_key(&secret), hash_client_key(&secret));

    let (total, keys) = repo
        .list_client_keys_page(42, 0, 10)
        .await
        .expect("list keys");
    assert_eq!(total, 1);
    assert_eq!(keys.len(), 1);

    let updated = repo
        .update_client_key(42, key_uuid, Some("renamed".to_string()), None)
        .await
        .expect("update key")
        .expect("key present");
    assert_eq!(updated.label, "renamed");

    repo.delete_client_key(42, key_uuid)
        .await
        .expect("delete key");
    let (total, _) = repo
        .list_client_keys_page(42, 0, 10)
        .await
        .expect("list keys after delete");
    assert_eq!(total, 0);

    close_repository(store, path).await;
}

#[tokio::test]
async fn settings_round_trip_through_unified_repository() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());

    let mut policy = RelayIpPolicy::default();
    policy.allowed_cidrs = vec!["10.0.0.0/8".to_string()];
    repo.set_json_setting("relay_ip_whitelist", &policy)
        .await
        .expect("set relay whitelist");
    let loaded: RelayIpPolicy = repo
        .get_json_setting("relay_ip_whitelist")
        .await
        .expect("get relay whitelist")
        .expect("policy present");
    assert_eq!(loaded.allowed_cidrs, vec!["10.0.0.0/8".to_string()]);

    repo.set_bool_setting("model_route_whitelist_enabled", false)
        .await
        .expect("set bool setting");
    let enabled = repo
        .get_bool_setting("model_route_whitelist_enabled", true)
        .await
        .expect("get bool setting");
    assert!(!enabled);

    close_repository(store, path).await;
}
