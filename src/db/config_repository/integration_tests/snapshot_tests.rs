//! Configuration snapshot tests: domain coverage and the SQLite reader.
//!
//! Two guarantees are pinned here. The exported payload carries exactly the
//! restorable configuration domains (never runtime history), and the SQLite
//! reader fills those domains with the recoverable secrets of a live store.

use super::snapshot_fixtures::{PASSPHRASE, sample_snapshot};
use super::*;

use crate::db::config_repository::archive::{decode_archive, encode_archive};
use crate::db::config_repository::snapshot::ConfigBackendKind;
use crate::db::config_repository::snapshot_source::build_config_snapshot;
use crate::db::{McpServerInput, UserStore};

#[test]
fn snapshot_payload_has_exactly_the_restorable_domains() {
    let snapshot = sample_snapshot(ConfigBackendKind::Postgres);
    let json = serde_json::to_value(&snapshot).expect("serialize snapshot");
    let mut keys = json
        .as_object()
        .expect("snapshot object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "client_keys",
            "endpoints",
            "manifest",
            "mcp_credentials",
            "mcp_servers",
            "model_routes",
            "raw_object_store",
            "relays",
            "settings",
            "user_redaction_configs",
            "users",
        ],
        "the snapshot payload must carry only restorable configuration domains"
    );
    for forbidden in [
        "request_records",
        "usage_events",
        "billing",
        "approvals",
        "sessions",
        "replay",
    ] {
        assert!(
            json.get(forbidden).is_none(),
            "runtime domain {forbidden} must never be exported"
        );
    }
}

#[tokio::test]
async fn sqlite_snapshot_covers_restorable_domains_and_excludes_runtime_history() {
    let (store, manager, path) = open_repository().await;
    let repo = ConfigRepository::sqlite(store.clone(), manager.clone());
    let users = UserStore::sqlite(store.pool().clone());
    users
        .bootstrap_admin("admin", "admin-password")
        .await
        .expect("bootstrap admin");

    let endpoint_id = Uuid::new_v4();
    repo.create_endpoint(
        endpoint_id,
        EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "primary upstream".to_string(),
            provider: EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: "https://upstream.example".to_string(),
            native_api: NativeApi::Chat,
            native_api_source: DbNativeApiSource::Manual,
            api_key: "endpoint-secret".to_string(),
            api_keys: vec![crate::db::EndpointApiKeyCreate {
                key_label: "primary".to_string(),
                api_key: "endpoint-secret".to_string(),
                position: 0,
                enabled: true,
                key_id: None,
            }],
            key_lb_enabled: false,
            enabled: true,
            proxy_url: Some("http://user:pass@proxy.example:3128".to_string()),
            active_windows: None,
        },
        false,
    )
    .await
    .expect("create endpoint");

    let route_id = Uuid::new_v4();
    repo.create_model_route(
        route_id,
        ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-*".to_string(),
            routing_strategy: ModelRouteRoutingStrategy::default(),
            enabled: true,
            targets: vec![ModelRouteTargetCreate {
                endpoint_id,
                enabled: true,
                upstream_model: Some("gpt-4o".to_string()),
                native_api: NativeApi::Chat,
                proxy_url_override: None,
                active_windows: None,
                dev_system_normalize: false,
                thinking_downgrade_enabled: false,
                thinking_effort_override: None,
                compact_mode: Default::default(),
                service_tier: None,
            }],
        },
    )
    .await
    .expect("create route");

    let user_id = users
        .list_users()
        .await
        .expect("list users")
        .first()
        .expect("admin user")
        .user_id;
    let created_key = repo
        .create_client_key(user_id, Some("codex"), true)
        .await
        .expect("create client key");
    repo.set_json_setting("redaction_config", &serde_json::json!({"enabled": true}))
        .await
        .expect("set redaction setting");
    repo.create_mcp_server(
        Uuid::new_v4(),
        McpServerInput {
            scope: "admin".to_string(),
            owner_user_id: None,
            source_endpoint_id: None,
            name: "files".to_string(),
            aggregate_naming_mode: "passthrough_preferred".to_string(),
            transport: "http".to_string(),
            provider_kind: None,
            url: Some("https://mcp.example/rpc".to_string()),
            command: None,
            args: serde_json::json!([]),
            env_json: serde_json::json!({"TOKEN": "env-secret"}),
            bearer_tokens_json: serde_json::json!([{"token": "mcp-secret", "enabled": true}]),
            http_headers_json: serde_json::json!({"Authorization": "Bearer header-secret"}),
            auth_mode: "bearer".to_string(),
            basic_username: None,
            basic_password: None,
            proxy_url: None,
            tool_filter_mode: "blacklist".to_string(),
            allowed_tools: serde_json::json!([]),
            disabled_tools: serde_json::json!([]),
            disabled_resources: serde_json::json!([]),
            enabled: true,
            timeout_ms: 30_000,
            lifecycle_policy: "auto".to_string(),
            lifecycle_manual_protocol_version: None,
        },
    )
    .await
    .expect("create mcp server");

    let snapshot = build_config_snapshot(&repo, &users, Some(&manager))
        .await
        .expect("build snapshot");
    assert_eq!(snapshot.manifest.backend_kind, ConfigBackendKind::Sqlite);
    assert_eq!(snapshot.domains.users.len(), 1);
    assert!(
        snapshot.domains.users[0].password_hash.is_some(),
        "password hashes are exported"
    );
    let exported_endpoint = snapshot
        .domains
        .endpoints
        .iter()
        .find(|endpoint| endpoint.endpoint_id == endpoint_id)
        .expect("endpoint in snapshot");
    assert_eq!(
        exported_endpoint.api_key.as_deref(),
        Some("endpoint-secret")
    );
    assert_eq!(
        exported_endpoint.proxy_url.as_deref(),
        Some("http://user:pass@proxy.example:3128")
    );
    assert_eq!(snapshot.domains.model_routes.len(), 1);
    assert_eq!(snapshot.domains.mcp_servers.len(), 1);
    assert_eq!(snapshot.domains.mcp_credentials.len(), 1);
    assert_eq!(snapshot.domains.mcp_credentials[0].secret, "mcp-secret");
    let exported_key = snapshot
        .domains
        .client_keys
        .iter()
        .find(|key| key.key_id == created_key.key.key_id)
        .expect("client key in snapshot");
    assert_eq!(
        exported_key.secret.as_deref(),
        Some(created_key.secret.as_str())
    );
    assert_eq!(
        exported_key.key_hash.as_deref(),
        Some(crate::keys::hash_client_key(&created_key.secret).as_str())
    );
    assert!(
        snapshot
            .domains
            .settings
            .iter()
            .any(|setting| setting.key == "redaction_config"),
        "settings domain carries the redaction configuration"
    );

    let encoded =
        encode_archive(PASSPHRASE, ConfigBackendKind::Sqlite, snapshot).expect("encode snapshot");
    let decoded = decode_archive(PASSPHRASE, &encoded.bytes).expect("decode snapshot");
    assert_eq!(decoded.domains.endpoints.len(), 1);
    assert_eq!(decoded.domains.mcp_credentials[0].secret, "mcp-secret");

    close_repository(store, path).await;
}
