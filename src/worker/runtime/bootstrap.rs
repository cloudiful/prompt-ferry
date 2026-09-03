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
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "file is empty or unreadable")
    }))
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
    let mcp_runtime = crate::mcp::McpRuntimeState::sqlite(
        config,
        crate::mcp::McpRuntimeStorage::from_repository(mcp_repository.clone()),
        store.pool().clone(),
    )
    .await;
    crate::mcp::McpCatalogService::new_with_repository(
        mcp_repository,
        mcp_runtime.catalog_cache.clone(),
    )
    .warm_enabled_servers()
    .await;
    let state =
        crate::worker::runtime::standalone::StandaloneRuntimeState::new(store, manager, snapshot)
            .with_mcp_runtime(mcp_runtime);
    state.hydrate_usage().await;
    Ok(state)
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
) -> anyhow::Result<Option<AdminState>> {
    let relay_secret_manager = relay_secrets::load_or_create_worker_config_key_for(
        config.effective_encryption_key(),
        secrets_dir,
    )?;
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
            mcp_catalog_service: crate::mcp::McpCatalogService::new_with_repository(
                config_repository.clone(),
                mcp_catalog_cache,
            ),
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
    use super::{
        build_admin_state, build_standalone_state, publish_bootstrap_password, validate_config,
    };
    use crate::config::WorkerConfig;
    use base64::Engine as _;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_key() -> String {
        base64::engine::general_purpose::STANDARD.encode([7_u8; 32])
    }

    fn temp_secrets_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("prompt-ferry-{name}-{}", uuid::Uuid::new_v4()))
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

    #[tokio::test]
    async fn standalone_runtime_opens_and_bootstraps_legacy_static_configuration() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-runtime-bootstrap-{}.sqlite",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let secrets_dir = temp_secrets_dir("standalone-bootstrap");
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
            upstream_base_url: "https://api.example.test".to_string(),
            upstream_api_key: "bootstrap-key".to_string(),
            worker_token: "token".to_string(),
            ..WorkerConfig::default()
        };

        let state = build_standalone_state(&config, Some(&secrets_dir))
            .await
            .expect("standalone state");
        let snapshot = state.snapshot().await;
        assert_eq!(snapshot.relays.len(), 1);
        assert_eq!(snapshot.endpoints.len(), 1);
        drop(state);
        let _ = std::fs::remove_file(path);
        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[tokio::test]
    async fn standalone_runtime_starts_without_endpoint_for_admin_setup() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-no-endpoint-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let secrets_dir = temp_secrets_dir("no-endpoint");
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            relay_urls: vec!["ws://127.0.0.1:8788/ws/worker".to_string()],
            upstream_base_url: "https://api.example.test".to_string(),
            upstream_api_key: String::new(),
            worker_token: String::new(),
            ..WorkerConfig::default()
        };

        let state = build_standalone_state(&config, Some(&secrets_dir))
            .await
            .expect("standalone state without endpoint or upstream API key");
        let snapshot = state.snapshot().await;
        assert!(snapshot.endpoints.is_empty());

        // The auto-generated encryption key file exists with restricted
        // permissions so restarts reuse the same key.
        let key_file = secrets_dir.join(crate::relay_secrets::WORKER_CONFIG_KEY_FILE);
        assert!(key_file.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        drop(state);
        let _ = std::fs::remove_file(path);
        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[test]
    fn publish_bootstrap_password_reuses_existing_file_without_overwriting() {
        let secrets_dir = temp_secrets_dir("publish-reuse");
        const FIRST: &str = "first-starter-candidate-password";
        const SECOND: &str = "second-starter-candidate-password";

        // The winning starter creates the file and gets its own candidate.
        let effective = publish_bootstrap_password(FIRST, Some(&secrets_dir)).unwrap();
        assert_eq!(effective, FIRST);

        // A racing starter loses creation and must reuse the existing
        // password verbatim instead of overwriting it.
        let effective = publish_bootstrap_password(SECOND, Some(&secrets_dir)).unwrap();
        assert_eq!(effective, FIRST);

        let path = secrets_dir.join(crate::relay_secrets::BOOTSTRAP_ADMIN_PASSWORD_FILE);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{FIRST}\n")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[tokio::test]
    async fn publish_bootstrap_password_fails_without_overwriting_unreadable_file() {
        let secrets_dir = temp_secrets_dir("publish-unreadable");
        let path = secrets_dir.join(crate::relay_secrets::BOOTSTRAP_ADMIN_PASSWORD_FILE);
        std::fs::create_dir_all(&secrets_dir).unwrap();
        std::fs::write(&path, "   \n").unwrap();

        let error = {
            let secrets_dir = secrets_dir.clone();
            tokio::task::spawn_blocking(move || {
                publish_bootstrap_password("candidate-password", Some(&secrets_dir))
            })
            .await
            .unwrap()
            .expect_err("an unreadable existing publication must fail")
        };
        assert!(error.to_string().contains("could not be read"));
        // The existing file is left untouched for an operator to resolve.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "   \n");

        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[tokio::test]
    async fn sqlite_admin_bootstrap_generates_password_into_protected_file() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-admin-bootstrap-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let secrets_dir = temp_secrets_dir("admin-bootstrap");
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            bootstrap_admin_login: "admin".to_string(),
            bootstrap_admin_password: String::new(),
            relay_secret_master_key: test_key(),
            ..WorkerConfig::default()
        };

        let state = build_admin_state(&config, false, Some(&secrets_dir))
            .await
            .expect("SQLite admin state")
            .expect("admin state");
        assert!(state.user_store.is_sqlite());

        let password_file = secrets_dir.join(crate::relay_secrets::BOOTSTRAP_ADMIN_PASSWORD_FILE);
        let generated = std::fs::read_to_string(&password_file)
            .expect("generated password file")
            .trim()
            .to_string();
        assert!(generated.len() >= 24);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&password_file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let stored = state
            .user_store
            .get_user_password_by_login("admin")
            .await
            .expect("load admin")
            .expect("admin user");
        assert!(crate::keys::verify_password(
            &generated,
            &stored.password_hash
        ));

        drop(state);
        let _ = std::fs::remove_file(path);
        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[tokio::test]
    async fn sqlite_admin_bootstrap_prefers_configured_password_without_file_writes() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-admin-configured-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let secrets_dir = temp_secrets_dir("admin-configured");
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            bootstrap_admin_login: "admin".to_string(),
            bootstrap_admin_password: "admin-password".to_string(),
            relay_secret_master_key: test_key(),
            ..WorkerConfig::default()
        };

        let state = build_admin_state(&config, false, Some(&secrets_dir))
            .await
            .expect("SQLite admin state")
            .expect("admin state");
        assert!(state.user_store.is_sqlite());
        assert!(
            !secrets_dir
                .join(crate::relay_secrets::BOOTSTRAP_ADMIN_PASSWORD_FILE)
                .exists()
        );

        drop(state);
        let _ = std::fs::remove_file(path);
        std::fs::remove_dir_all(secrets_dir).ok();
    }

    #[tokio::test]
    async fn sqlite_admin_bootstrap_rejects_empty_login_with_clear_error() {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-admin-empty-login-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let secrets_dir = temp_secrets_dir("admin-empty-login");
        let config = WorkerConfig {
            standalone_database_path: path.to_string_lossy().to_string(),
            bootstrap_admin_login: "   ".to_string(),
            bootstrap_admin_password: String::new(),
            relay_secret_master_key: test_key(),
            ..WorkerConfig::default()
        };

        let error = match build_admin_state(&config, false, Some(&secrets_dir)).await {
            Ok(_) => panic!("empty login must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("login must not be empty"));
        assert!(
            !secrets_dir
                .join(crate::relay_secrets::BOOTSTRAP_ADMIN_PASSWORD_FILE)
                .exists()
        );

        let _ = std::fs::remove_file(path);
        std::fs::remove_dir_all(secrets_dir).ok();
    }
}
