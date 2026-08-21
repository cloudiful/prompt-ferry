use crate::{
    bridge_crypto::{self},
    config::{WorkerConfig, normalize_relay_url},
    db,
    llm_review::{self},
    redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    runtime_env,
    standalone_config::{BootstrapSeed, StandaloneConfigStore},
    tls,
    worker_admin::{self, AdminState},
};
use anyhow::anyhow;
use sqlx::postgres::PgPoolOptions;
use std::{sync::Arc, time::Duration};
use tracing::{error, warn};

pub(super) fn validate_config(config: &WorkerConfig) -> anyhow::Result<()> {
    if config.worker_token.is_empty() {
        return Err(anyhow!("worker token is required"));
    }
    let contract = config.storage_contract();
    if contract.backend.is_sqlite() {
        runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
        let relay_urls = config
            .relay_urls
            .iter()
            .map(|relay_url| normalize_relay_url(relay_url))
            .filter(|relay_url| !relay_url.is_empty())
            .collect::<Vec<_>>();
        if !relay_urls.is_empty() {
            let unique_relay_urls = relay_urls
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if unique_relay_urls.len() != relay_urls.len() {
                return Err(anyhow!(
                    "worker relay URLs must be unique after normalization"
                ));
            }
        }
        if !relay_urls.is_empty() {
            tls::validate_worker_config(config)?;
            bridge_crypto::validate_settings(
                "worker",
                config.bridge_encryption_mode,
                &config.bridge_encryption_key,
            )?;
        }
    }
    RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    if config
        .upstream_base_url
        .trim()
        .trim_end_matches('/')
        .ends_with("/v1")
    {
        return Err(anyhow!(
            "upstream_base_url must be the provider base URL without /v1"
        ));
    }
    Ok(())
}

pub(super) async fn build_standalone_state(
    config: &WorkerConfig,
) -> anyhow::Result<crate::worker::runtime::standalone::StandaloneRuntimeState> {
    if !config.storage_backend().is_sqlite() {
        return Err(anyhow!(
            "standalone runtime state requires the SQLite storage backend"
        ));
    }
    let manager = RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    let path = runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            anyhow!(error).context("failed to create standalone SQLite parent directory")
        })?;
    }
    let store = Arc::new(StandaloneConfigStore::open(&path).await?);
    if !config.relay_urls.is_empty() && !config.upstream_api_key.is_empty() {
        let tls_mode = config
            .relay_urls
            .first()
            .map(|url| tls::worker_tls_mode(config, url))
            .transpose()?;
        let seed = BootstrapSeed {
            relay_urls: config
                .relay_urls
                .iter()
                .map(|url| normalize_relay_url(url))
                .collect(),
            tls_mode: tls_mode.unwrap_or_default(),
            relay_ca_pem: optional_file_pem(&config.relay_ca)?,
            client_cert_pem: optional_file_pem(&config.client_cert)?,
            client_key_pem: optional_file_pem(&config.client_key)?,
            bridge_encryption_mode: config.bridge_encryption_mode,
            bridge_encryption_key: (!config.bridge_encryption_key.trim().is_empty())
                .then(|| config.bridge_encryption_key.clone()),
            upstream_base_url: config.upstream_base_url.clone(),
            upstream_api_key: config.upstream_api_key.clone(),
            upstream_native_api: config.upstream_native_api,
        };
        store.bootstrap_if_empty(&manager, seed).await?;
    }
    let snapshot = store.load_snapshot(&manager).await?;
    if snapshot.relays.iter().all(|relay| !relay.enabled) && config.relay_urls.is_empty() {
        return Err(anyhow!(
            "standalone configuration has no enabled persisted relays or static relay URLs"
        ));
    }
    if snapshot.endpoints.iter().all(|endpoint| !endpoint.enabled)
        && config.upstream_api_key.trim().is_empty()
    {
        return Err(anyhow!(
            "standalone configuration has no enabled persisted endpoints or static upstream API key"
        ));
    }
    Ok(crate::worker::runtime::standalone::StandaloneRuntimeState::new(store, manager, snapshot))
}

fn optional_file_pem(path: &str) -> anyhow::Result<Option<String>> {
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(tls::read_pem_file(path)?))
    }
}

pub(super) async fn build_admin_state(
    config: &WorkerConfig,
    spawn_admin_server: bool,
) -> anyhow::Result<Option<AdminState>> {
    let relay_secret_manager = RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    let is_postgres = config.storage_backend().is_postgres();
    let (pool, lease_pool, user_store, config_repository, sqlite_pool) = if is_postgres {
        let pool = db::connect(&config.database_url).await?;
        let lease_pool = db::connect_with_max_connections(&config.database_url, 2).await?;
        db::migrate(&pool).await?;
        let user_store = db::UserStore::postgres(&pool);
        let config_repository = db::ConfigRepository::postgres(&pool);
        (pool, lease_pool, user_store, config_repository, None)
    } else {
        let path = runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                anyhow!(error).context("failed to create standalone SQLite parent directory")
            })?;
        }
        let store = Arc::new(StandaloneConfigStore::open(&path).await?);
        let sqlite_pool = store.pool().clone();
        let pool =
            PgPoolOptions::new().connect_lazy("postgres://postgres@localhost/prompt_ferry")?;
        let manager = RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
        let config_repository = db::ConfigRepository::sqlite(store, manager);
        (
            pool.clone(),
            pool,
            db::UserStore::sqlite(sqlite_pool.clone()),
            config_repository,
            Some(sqlite_pool),
        )
    };
    user_store
        .bootstrap_admin(
            &config.bootstrap_admin_login,
            &config.bootstrap_admin_password,
        )
        .await?;

    if !is_postgres {
        let mcp_catalog_cache =
            crate::mcp::McpCatalogCache::from_config_with_sqlite(config, sqlite_pool.clone()).await;
        let state = AdminState::new(crate::worker_admin_state::AdminStateInit {
            pool: pool.clone(),
            lease_pool,
            replay_cache: ReplayCache::from_config_with_sqlite(config, sqlite_pool.clone()).await,
            configured_relays: config.relay_urls.clone(),
            managed_mode: false,
            relay_secret_manager: Some(relay_secret_manager),
            redaction_enabled: false,
            model_route_whitelist_enabled: true,
            request_content_logging: crate::worker_admin_types::RequestContentLoggingResponse {
                mode: crate::worker_admin_types::RequestContentLoggingMode::Off,
                raw_retention_days: 3,
            },
            usage_retention: crate::worker_admin_types::UsageRetentionSettings::default(),
            raw_payload_store: None,
            stream_delta_batching: db::StreamDeltaBatchingSettings::default(),
            llm_review_settings: llm_review::LlmReviewSettings::default(),
            mcp_catalog_cache: mcp_catalog_cache.clone(),
            mcp_catalog_service: crate::mcp::McpCatalogService::new(pool, mcp_catalog_cache),
            mcp_session_store: crate::mcp::McpSessionStore::from_config_with_sqlite(
                config,
                sqlite_pool,
            )
            .await,
            mcp_allowed_origins: config.mcp_allowed_origins.clone(),
            mcp_quota_valkey: crate::mcp::McpQuotaValkey::new(),
            endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
                Duration::from_secs(config.endpoint_model_cache_ttl_seconds.max(1)),
            ),
        })
        .with_user_store(user_store)
        .with_config_repository(config_repository);
        if spawn_admin_server {
            let admin_config = config.clone();
            let admin_state = state.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    worker_admin::run_admin_server(admin_state, &admin_config.admin_bind).await
                {
                    error!(error = %err, "worker admin server stopped");
                }
            });
        }
        return Ok(Some(state));
    }

    let usage_retention = db::get_usage_retention(&pool).await?;
    let raw_payload_store = match crate::raw_payload_store::RawPayloadStore::from_config(config)? {
        Some(store) => Some(Arc::new(store)),
        None if usage_retention.raw_backend == "object_store" => {
            warn!(
                "raw backend is configured as object_store but no bucket is configured; retaining raw payloads in postgres"
            );
            None
        }
        None => None,
    };
    let redaction_config = db::get_redaction_config(&pool).await?;
    let user_redaction_configs = db::list_user_redaction_configs(&pool).await?;
    redact::apply_configs(&redaction_config, user_redaction_configs)?;
    let redaction_enabled = redact::has_any_enabled();
    let llm_review_settings = db::get_json_setting(&pool, llm_review::LLM_REVIEW_SETTINGS_KEY)
        .await?
        .unwrap_or_default();
    let model_route_whitelist_enabled =
        db::get_bool_setting(&pool, "model_route_whitelist_enabled", true).await?;
    let mut request_content_logging = db::get_request_content_logging(&pool).await?;
    request_content_logging.raw_retention_days = usage_retention.raw_retention_days;
    let stream_delta_batching = db::get_stream_delta_batching(&pool).await?;
    let aborted_count = db::abort_pending_approval_requests(&pool).await?;
    if aborted_count > 0 {
        warn!(
            count = aborted_count,
            "aborted stale pending approval requests on startup"
        );
    }
    let mcp_catalog_cache = crate::mcp::McpCatalogCache::from_config(config).await;
    let mcp_catalog_service =
        crate::mcp::McpCatalogService::new(pool.clone(), mcp_catalog_cache.clone());
    let state = AdminState::new(crate::worker_admin_state::AdminStateInit {
        pool,
        lease_pool,
        replay_cache: ReplayCache::from_config(config).await,
        configured_relays: Vec::new(),
        managed_mode: true,
        relay_secret_manager: Some(relay_secret_manager),
        redaction_enabled,
        model_route_whitelist_enabled,
        request_content_logging,
        usage_retention,
        raw_payload_store,
        stream_delta_batching,
        llm_review_settings,
        mcp_catalog_cache,
        mcp_catalog_service,
        mcp_session_store: crate::mcp::McpSessionStore::from_config(config).await,
        mcp_allowed_origins: config.mcp_allowed_origins.clone(),
        mcp_quota_valkey: crate::mcp::McpQuotaValkey::from_config(config).await,
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(Duration::from_secs(
            config.endpoint_model_cache_ttl_seconds.max(1),
        )),
    })
    .with_user_store(user_store);
    if spawn_admin_server {
        let admin_config = config.clone();
        let admin_state = state.clone();
        tokio::spawn(async move {
            if let Err(err) =
                worker_admin::run_admin_server(admin_state, &admin_config.admin_bind).await
            {
                error!(error = %err, "worker admin server stopped");
            }
        });

        let mcp_catalog_service = state.mcp_catalog_service.clone();
        tokio::spawn(async move {
            mcp_catalog_service.warm_enabled_servers().await;
        });

        let quota_pool = state.pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                if let Err(err) = crate::db::release_expired_reservations(&quota_pool).await {
                    warn!(error = %err, "MCP quota reservation cleanup failed");
                }
            }
        });
    }
    Ok(Some(state))
}

#[cfg(test)]
mod tests {
    use super::{build_admin_state, validate_config};
    use crate::config::WorkerConfig;
    use base64::Engine as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn empty_database_url_selects_named_standalone_mode() {
        let config = WorkerConfig {
            upstream_api_key: "bootstrap-key".to_string(),
            worker_token: "token".to_string(),
            relay_secret_master_key: base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
            ..WorkerConfig::default()
        };

        assert_eq!(config.storage_backend().as_str(), "sqlite");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn configured_database_url_keeps_shared_managed_mode() {
        let config = WorkerConfig {
            database_url: "  postgres://postgres@localhost/prompt_ferry  ".to_string(),
            relay_urls: Vec::new(),
            relay_secret_master_key: base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
            worker_token: "token".to_string(),
            ..WorkerConfig::default()
        };

        assert_eq!(config.storage_backend().as_str(), "postgres");
        assert!(validate_config(&config).is_ok());
    }

    #[tokio::test]
    async fn standalone_runtime_opens_and_bootstraps_legacy_static_configuration() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-runtime-bootstrap-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
            upstream_base_url: "https://api.example.test".to_string(),
            upstream_api_key: "bootstrap-key".to_string(),
            worker_token: "token".to_string(),
            relay_secret_master_key: base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
            ..WorkerConfig::default()
        };

        let state = super::super::bootstrap::build_standalone_state(&config)
            .await
            .expect("standalone state");
        let snapshot = state.snapshot().await;
        assert_eq!(snapshot.relays.len(), 1);
        assert_eq!(snapshot.endpoints.len(), 1);
        drop(state);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sqlite_admin_bootstrap_requires_a_non_empty_password() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-admin-bootstrap-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let mut config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            bootstrap_admin_login: "admin".to_string(),
            bootstrap_admin_password: String::new(),
            relay_secret_master_key: base64::engine::general_purpose::STANDARD.encode([7_u8; 32]),
            ..WorkerConfig::default()
        };

        let error = match build_admin_state(&config, false).await {
            Ok(_) => panic!("empty bootstrap password must fail"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("bootstrap admin login and password")
        );

        config.bootstrap_admin_password = "admin-password".to_string();
        let state = build_admin_state(&config, false)
            .await
            .expect("SQLite admin state")
            .expect("admin state");
        assert!(state.user_store.is_sqlite());
        drop(state);
        let _ = std::fs::remove_file(path);
    }
}
