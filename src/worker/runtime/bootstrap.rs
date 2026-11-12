use crate::{
    bridge_crypto::{self},
    config::{WorkerConfig, normalize_relay_url},
    db,
    llm_review::{self},
    redact,
    relay_secrets::{self, BOOTSTRAP_ADMIN_PASSWORD_FILE, RelaySecretManager},
    replay_cache::ReplayCache,
    runtime_env,
    standalone_config::{BootstrapSeed, StandaloneConfigStore},
    tls,
    worker::runtime::WorkerShutdown,
    worker_admin::{self, AdminState},
};
use anyhow::anyhow;
use sqlx::postgres::PgPoolOptions;
use std::{path::Path, sync::Arc, time::Duration};
use tracing::{error, info, warn};

/// Atomically publish a generated bootstrap admin password and return the
/// effective one to store in the database.
///
/// The protected file is created only if absent. If another starter already
/// created it (supported multi-process PostgreSQL startup), that process's
/// password is read back and reused so every racing process inserts the same
/// password and the file always matches the database. A short bounded retry
/// covers reading a file another process has just created but not yet
/// written; any other failure propagates so no user row is committed.
fn publish_bootstrap_password(
    candidate: &str,
    secrets_dir: Option<&Path>,
) -> anyhow::Result<String> {
    use anyhow::Context;
    let password_path =
        relay_secrets::resolve_data_file(BOOTSTRAP_ADMIN_PASSWORD_FILE, secrets_dir)?;
    let won_creation =
        runtime_env::create_private_file_exclusive(&password_path, &format!("{candidate}\n"))?;
    if won_creation {
        return Ok(candidate.to_string());
    }
    // Another starter won the race: reuse its published password verbatim
    // instead of overwriting it.
    const READ_ATTEMPTS: usize = 50;
    let mut last_error = None;
    for _ in 0..READ_ATTEMPTS {
        match std::fs::read_to_string(&password_path) {
            Ok(content) => {
                let existing = content.trim();
                if !existing.is_empty() {
                    return Ok(existing.to_string());
                }
            }
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("file is empty or unreadable")))
        .with_context(|| {
            format!(
                "bootstrap admin password file {} already exists but could not be read after \
             retries; remove the file or restore its contents",
                password_path.display()
            )
        })
}

pub(super) fn validate_config(config: &WorkerConfig) -> anyhow::Result<()> {
    // An empty worker token intentionally disables relay worker authentication;
    // the relay logs a warning when this mode is active.
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
    // An explicitly configured encryption key must already be valid; when none
    // is configured one is generated later during state construction.
    let configured_key = config.effective_encryption_key().trim();
    if !configured_key.is_empty() {
        RelaySecretManager::from_base64(configured_key)?;
    }
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
    secrets_dir: Option<&Path>,
    catalog_cache: Option<crate::mcp::McpCatalogCache>,
) -> anyhow::Result<crate::worker::runtime::standalone::StandaloneRuntimeState> {
    if !config.storage_backend().is_sqlite() {
        return Err(anyhow!(
            "standalone runtime state requires the SQLite storage backend"
        ));
    }
    let manager = relay_secrets::load_or_create_worker_config_key_for(
        config.effective_encryption_key(),
        secrets_dir,
    )?;
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
    // First startup may have no endpoint yet: the worker stays up and the
    // Admin setup flow creates the first endpoint. Upstream requests are not
    // possible until an enabled endpoint exists.
    if snapshot.endpoints.iter().all(|endpoint| !endpoint.enabled) {
        warn!(
            "standalone configuration has no enabled endpoints and no static upstream API key; \
             configure an upstream endpoint through the Admin console before sending requests"
        );
    }
    let mcp_repository = db::ConfigRepository::sqlite(store.clone(), manager.clone());
    // The request path must share the catalog cache (and therefore its refresh
    // coordinator) with the admin state and the managed refresh task; only a
    // caller without one builds it here.
    let catalog_cache = match catalog_cache {
        Some(cache) => cache,
        None => {
            crate::mcp::McpCatalogCache::from_config_with_sqlite(config, Some(store.pool().clone()))
                .await
        }
    };
    let mcp_runtime = crate::mcp::McpRuntimeState::sqlite(
        config,
        {
            let storage = crate::mcp::McpRuntimeStorage::from_repository(mcp_repository);
            storage.with_oauth_service(std::sync::Arc::new(
                crate::mcp::oauth::McpOAuthService::new(
                    storage.repository().clone(),
                    Some(manager.clone()),
                    Some(config.mcp_oauth_redirect_base.clone()),
                    crate::db::UserStore::sqlite(store.pool().clone()),
                ),
            ))
        },
        store.pool().clone(),
        catalog_cache,
    )
    .await;
    let state =
        crate::worker::runtime::standalone::StandaloneRuntimeState::new(store, manager, snapshot)
            .with_mcp_runtime(mcp_runtime);
    state.hydrate_usage().await;
    Ok(state)
}

/// Attach the personal MCP OAuth backend to a freshly built admin state.
///
/// The backend is optional by construction: a deployment without a redirect
/// base still builds, and only personal OAuth reports the missing setting.
fn attach_mcp_oauth(state: AdminState, config: &crate::config::WorkerConfig) -> AdminState {
    let shared_key_missing = config.mcp_oauth_shared_key_missing();
    let mut service = crate::mcp::oauth::McpOAuthService::new(
        state.config_repository.clone(),
        if shared_key_missing {
            None
        } else {
            state.relay_secret_manager.clone()
        },
        Some(config.mcp_oauth_redirect_base.clone()),
        state.user_store.clone(),
    );
    if shared_key_missing {
        service = service.with_disabled_reason(
            "PostgreSQL personal MCP OAuth requires an explicitly configured shared \
             WORKER_CONFIG_ENCRYPTION_KEY so every worker can decrypt the same grants",
        );
    }
    let service = std::sync::Arc::new(service);
    let catalog = state
        .mcp_catalog_service
        .clone()
        .with_oauth_service(service.clone());
    state
        .with_mcp_catalog_service(catalog)
        .with_mcp_oauth_service(service)
}

/// Start the admin HTTP server on a socket the caller already bound.
///
/// The integrated entrypoint binds and reserves the admin socket before the
/// worker begins its slow bootstrap and passes the result in, so a bind
/// failure has already been reported by the time this runs. The standalone
/// worker passes `None` and the server binds `admin_bind` itself, keeping its
/// existing behaviour of staying up and logging a failure.
fn start_admin_server(
    state: AdminState,
    config: &WorkerConfig,
    admin_listener: Option<tokio::net::TcpListener>,
    worker_shutdown: Option<&WorkerShutdown>,
) {
    let admin_config = config.clone();
    let shutdown_rx = worker_shutdown.map(WorkerShutdown::subscribe);
    tokio::spawn(async move {
        let served = worker_admin::run_admin_server(
            state,
            &admin_config.admin_bind,
            admin_listener,
            shutdown_rx,
        )
        .await;
        if let Err(err) = served {
            error!(error = %err, "worker admin server stopped");
        }
    });
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
    secrets_dir: Option<&Path>,
    worker_shutdown: Option<&WorkerShutdown>,
    admin_listener: Option<tokio::net::TcpListener>,
) -> anyhow::Result<Option<AdminState>> {
    let relay_secret_manager = relay_secrets::load_or_create_worker_config_key_for(
        config.effective_encryption_key(),
        secrets_dir,
    )?;
    let is_postgres = config.storage_backend().is_postgres();
    let (pool, lease_pool, user_store, config_repository, sqlite_pool) = if is_postgres {
        let pool = db::connect(&config.database_url).await?;
        // The lease pool serves request admission, heartbeat, cancel and
        // delete; a 2-connection cap queued them behind each other.
        let lease_pool = db::connect_with_max_connections(&config.database_url, 8).await?;
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
        let config_repository = db::ConfigRepository::sqlite(store, relay_secret_manager.clone());
        (
            pool.clone(),
            pool,
            db::UserStore::sqlite(sqlite_pool.clone()),
            config_repository,
            Some(sqlite_pool),
        )
    };
    // When no active user exists and no password is configured, a strong
    // random candidate password is atomically published to the protected file
    // (create-if-absent) BEFORE the admin user is committed; if another
    // starter won the publication race, its existing password is reused so
    // the file and the database always agree. A failing publication aborts
    // startup without leaving an account whose password cannot be retrieved.
    // Only the path and login are logged, never the password itself.
    user_store
        .ensure_bootstrap_admin(
            &config.bootstrap_admin_login,
            (!config.bootstrap_admin_password.trim().is_empty())
                .then_some(config.bootstrap_admin_password.as_str()),
            |candidate| {
                let effective = publish_bootstrap_password(candidate, secrets_dir)?;
                let password_path =
                    relay_secrets::resolve_data_file(BOOTSTRAP_ADMIN_PASSWORD_FILE, secrets_dir)?;
                info!(
                    login = %config.bootstrap_admin_login,
                    password_file = %password_path.display(),
                    "no active admin existed and no bootstrap password was configured; an \
                     initial admin password is available in the logged file"
                );
                Ok(effective)
            },
        )
        .await?;

    if !is_postgres {
        // The SQLite path has no Valkey backend; each `from_config_with_sqlite`
        // call only touches the embedded store so they can run together
        // without changing semantics.
        let (mcp_catalog_cache, replay_cache, mcp_session_store) = tokio::join!(
            crate::mcp::McpCatalogCache::from_config_with_sqlite(config, sqlite_pool.clone()),
            ReplayCache::from_config_with_sqlite(config, sqlite_pool.clone()),
            crate::mcp::McpSessionStore::from_config_with_sqlite(config, sqlite_pool),
        );
        let stream_delta_batching: db::StreamDeltaBatchingSettings = config_repository
            .get_json_setting::<db::StreamDeltaBatchingSettings>("stream_delta_batching")
            .await?
            .map(|v| db::StreamDeltaBatchingSettings {
                enabled: v.enabled,
                flush_window_ms: v.flush_window_ms.clamp(1, 1_000),
                max_buffer_chars: v.max_buffer_chars.clamp(1, 8_192),
                max_buffer_bytes: v.max_buffer_bytes.clamp(1, 65_536),
                flush_on_line_break: v.flush_on_line_break,
                flush_on_sentence_end: v.flush_on_sentence_end,
            })
            .unwrap_or_default();
        let state = AdminState::new(crate::worker_admin_state::AdminStateInit {
            pool: pool.clone(),
            lease_pool,
            replay_cache,
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
            stream_delta_batching,
            llm_review_settings: llm_review::LlmReviewSettings::default(),
            mcp_catalog_cache: mcp_catalog_cache.clone(),
            mcp_catalog_service: crate::mcp::McpCatalogService::new_with_repository(
                config_repository.clone(),
                mcp_catalog_cache,
            ),
            mcp_session_store,
            mcp_allowed_origins: config.mcp_allowed_origins.clone(),
            endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(
                Duration::from_secs(config.endpoint_model_cache_ttl_seconds.max(1)),
            ),
        })
        .with_user_store(user_store)
        .with_config_repository(config_repository);
        let state = attach_mcp_oauth(state, config);
        if spawn_admin_server {
            start_admin_server(state.clone(), config, admin_listener, worker_shutdown);
        }
        return Ok(Some(state));
    }

    let usage_retention = db::get_usage_retention(&pool).await?;
    let cache_alert = db::get_cache_alert_settings(&pool).await?;
    // Raw payloads always go to the managed store (bucket or local fallback);
    // when a persisted raw object-store setting exists it replaces the
    // environment-based fallback without a restart.
    let raw_payload_store = {
        let persisted = crate::db::get_raw_object_store_persisted(&pool).await?;
        let raw_config = if let Some(persisted) = persisted {
            match persisted.into_config(&relay_secret_manager) {
                Ok(cfg) => cfg,
                Err(err) => {
                    warn!(error = %err, "failed to decrypt persisted raw object store config; falling back to environment-based configuration");
                    crate::raw_payload_store::RawObjectStoreConfig::from_worker_config(config)
                }
            }
        } else {
            crate::raw_payload_store::RawObjectStoreConfig::from_worker_config(config)
        };
        match raw_config.build_store() {
            Ok(store) => store.map(Arc::new),
            Err(err) => {
                warn!(error = %err, "failed to build raw payload store; raw payloads will be dropped");
                None
            }
        }
    };
    // These settings live on independent rows so their reads are safe to
    // overlap; previously they ran one after another and added several
    // round-trips of cold-start latency.
    let (
        redaction_config,
        user_redaction_configs,
        llm_review_settings,
        model_route_whitelist_enabled,
        request_content_logging,
        stream_delta_batching,
    ) = tokio::join!(
        db::get_redaction_config(&pool),
        db::list_user_redaction_configs(&pool),
        async {
            Ok::<_, anyhow::Error>(
                db::get_json_setting(&pool, llm_review::LLM_REVIEW_SETTINGS_KEY)
                    .await?
                    .unwrap_or_default(),
            )
        },
        async { db::get_bool_setting(&pool, "model_route_whitelist_enabled", true).await },
        db::get_request_content_logging(&pool),
        db::get_stream_delta_batching(&pool),
    );
    let redaction_config = redaction_config?;
    let user_redaction_configs = user_redaction_configs?;
    let llm_review_settings = llm_review_settings?;
    let model_route_whitelist_enabled = model_route_whitelist_enabled?;
    let mut request_content_logging = request_content_logging?;
    request_content_logging.raw_retention_days = usage_retention.raw_retention_days;
    let stream_delta_batching = stream_delta_batching?;
    redact::apply_configs(&redaction_config, user_redaction_configs)?;
    let redaction_enabled = redact::has_any_enabled();
    let aborted_count = db::abort_pending_approval_requests(&pool).await?;
    if aborted_count > 0 {
        warn!(
            count = aborted_count,
            "aborted stale pending approval requests on startup"
        );
    }
    let (mcp_catalog_cache, replay_cache, mcp_session_store) = tokio::join!(
        crate::mcp::McpCatalogCache::from_config(config),
        ReplayCache::from_config(config),
        crate::mcp::McpSessionStore::from_config(config),
    );
    let mcp_catalog_cache = mcp_catalog_cache;
    let replay_cache = replay_cache;
    let mcp_session_store = mcp_session_store;
    let mcp_catalog_service =
        crate::mcp::McpCatalogService::new(pool.clone(), mcp_catalog_cache.clone());
    let state = AdminState::new(crate::worker_admin_state::AdminStateInit {
        pool,
        lease_pool,
        replay_cache,
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
        mcp_session_store,
        mcp_allowed_origins: config.mcp_allowed_origins.clone(),
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(Duration::from_secs(
            config.endpoint_model_cache_ttl_seconds.max(1),
        )),
    })
    .with_user_store(user_store)
    .with_cache_alert(cache_alert);
    let state = attach_mcp_oauth(state, config);
    let quota_service = Arc::new(
        worker_admin::chatgpt_quota_service::ChatGptQuotaService::new_with_feedback(
            state.pool.clone(),
            state.config_repository.clone(),
            state.token_plan_quota.feedback.clone(),
        ),
    );
    let state = state.with_chatgpt_quota_service(quota_service);
    if spawn_admin_server {
        start_admin_server(state.clone(), config, admin_listener, worker_shutdown);

        let _ = &state.pool;
    }
    Ok(Some(state))
}

#[cfg(test)]
mod tests {
    use super::validate_config;
    use crate::config::WorkerConfig;
    use base64::Engine as _;

    fn test_key() -> String {
        base64::engine::general_purpose::STANDARD.encode([7_u8; 32])
    }

    #[test]
    fn empty_database_url_selects_named_standalone_mode() {
        let config = WorkerConfig {
            upstream_api_key: "bootstrap-key".to_string(),
            worker_token: "token".to_string(),
            relay_secret_master_key: test_key(),
            ..WorkerConfig::default()
        };

        assert_eq!(config.storage_backend().as_str(), "sqlite");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn first_startup_validates_without_token_upstream_key_or_encryption_key() {
        let config = WorkerConfig {
            worker_token: String::new(),
            ..WorkerConfig::default()
        };

        assert!(config.worker_token.is_empty());
        assert!(config.upstream_api_key.is_empty());
        assert_eq!(config.storage_backend().as_str(), "sqlite");
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn invalid_configured_encryption_key_fails_validation() {
        let config = WorkerConfig {
            relay_secret_master_key: "not-base64!!".to_string(),
            ..WorkerConfig::default()
        };
        let error = validate_config(&config).unwrap_err();
        assert!(error.to_string().contains("valid base64"));
    }

    #[test]
    fn current_encryption_key_takes_precedence_over_legacy() {
        // A valid current key wins even when the legacy key is invalid.
        let config = WorkerConfig {
            worker_config_encryption_key: test_key(),
            relay_secret_master_key: "not-base64!!".to_string(),
            ..WorkerConfig::default()
        };
        assert!(validate_config(&config).is_ok());

        // An invalid current key is not silently replaced by the legacy one.
        let config = WorkerConfig {
            worker_config_encryption_key: "not-base64!!".to_string(),
            relay_secret_master_key: test_key(),
            ..WorkerConfig::default()
        };
        assert!(validate_config(&config).is_err());
    }

    #[test]
    fn configured_database_url_keeps_shared_managed_mode() {
        let config = WorkerConfig {
            database_url: "  postgres://postgres@localhost/prompt_ferry  ".to_string(),
            relay_urls: Vec::new(),
            relay_secret_master_key: test_key(),
            worker_token: "token".to_string(),
            ..WorkerConfig::default()
        };

        assert_eq!(config.storage_backend().as_str(), "postgres");
        assert!(validate_config(&config).is_ok());
    }
}
