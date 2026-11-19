use crate::{
    authz,
    config::WorkerConfig,
    db, redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    worker_admin::{self, AdminState},
};
use anyhow::Context;
use std::{sync::Arc, time::Duration};

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

/// Resolve the shared Valkey configuration from the database.
///
/// Shared Valkey ownership is database-only. A missing row is the degraded
/// single-worker state; a decryption or parse failure aborts startup instead
/// of silently falling back to process-local coordination.
async fn resolve_shared_valkey(
    pool: &sqlx::PgPool,
    relay_secret_manager: &RelaySecretManager,
    seed: Option<&str>,
) -> anyhow::Result<db::SharedValkeyConfig> {
    // The candidate is a first-start initializer provided by the caller. The
    // database row always wins, so this never overrides a configured or
    // deliberately cleared setting.
    db::seed_shared_valkey_from_env(pool, relay_secret_manager, seed)
        .await
        .context("failed to apply the shared Valkey seed")?;
    db::load_shared_valkey_config(pool, relay_secret_manager)
        .await
        .context("failed to resolve the shared Valkey configuration")
}

/// Build the admin state from already connected PostgreSQL pools.
///
/// The caller owns the pools and the secret manager, so no key is generated
/// and no migration is run here: the node runtime connects its gated pools
/// itself and lets the node gate own the schema.
///
/// The shared settings, caches and the personal MCP OAuth backend are built
/// using the database as the authoritative configuration source.
async fn build_postgres_admin_state(
    config: &WorkerConfig,
    pool: sqlx::PgPool,
    lease_pool: sqlx::PgPool,
    relay_secret_manager: RelaySecretManager,
    shared_valkey: db::SharedValkeyConfig,
) -> anyhow::Result<AdminState> {
    let usage_retention = db::get_usage_retention(&pool).await?;
    let cache_alert = db::get_cache_alert_settings(&pool).await?;
    // These settings live on independent rows so their reads are safe to
    // overlap; previously they ran one after another and added several
    // round-trips of cold-start latency.
    let (
        redaction_config,
        user_redaction_configs,
        model_route_whitelist_enabled,
        request_content_logging,
        stream_delta_batching,
    ) = tokio::join!(
        db::get_redaction_config(&pool),
        db::list_user_redaction_configs(&pool),
        async { db::get_bool_setting(&pool, "model_route_whitelist_enabled", true).await },
        db::get_request_content_logging(&pool),
        db::get_stream_delta_batching(&pool),
    );
    let redaction_config = redaction_config?;
    let user_redaction_configs = user_redaction_configs?;
    let model_route_whitelist_enabled = model_route_whitelist_enabled?;
    let mut request_content_logging = request_content_logging?;
    request_content_logging.raw_retention_days = usage_retention.raw_retention_days;
    let stream_delta_batching = stream_delta_batching?;
    redact::apply_configs(&redaction_config, user_redaction_configs)?;
    let redaction_enabled = redact::has_any_enabled();
    let (mcp_catalog_cache, replay_cache, mcp_session_store) = tokio::try_join!(
        crate::mcp::McpCatalogCache::from_config(config, &shared_valkey),
        ReplayCache::from_config(config, &shared_valkey),
        crate::mcp::McpSessionStore::from_config(config, &shared_valkey),
    )?;
    let mcp_catalog_cache = mcp_catalog_cache;
    let replay_cache = replay_cache;
    let mcp_session_store = mcp_session_store;
    let mcp_catalog_service =
        crate::mcp::McpCatalogService::new(pool.clone(), mcp_catalog_cache.clone());
    let user_store = db::UserStore::postgres(&pool);
    let authorizer = Arc::new(
        authz::build()
            .await
            .context("failed to build the authorization engine")?,
    );
    let state = AdminState::new(crate::worker_admin_state::AdminStateInit {
        authorizer,
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
        stream_delta_batching,
        mcp_catalog_cache,
        mcp_catalog_service,
        mcp_session_store,
        mcp_allowed_origins: config.mcp_allowed_origins.clone(),
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(Duration::from_secs(
            config.endpoint_model_cache_ttl_seconds.max(1),
        )),
        shared_valkey,
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
    Ok(state)
}

/// Build the admin state of the node runtime from its gated pools.
///
/// The node runtime owns the pools: the node gate prepared the schema and
/// the gated pool connections already hold the shared barrier. No migration
/// runs, no key is generated and no bootstrap admin is created here; the
/// master secret of the node is the only key material.
pub(super) async fn build_admin_state_from_pools(
    config: &WorkerConfig,
    pool: sqlx::PgPool,
    lease_pool: sqlx::PgPool,
    relay_secret_manager: RelaySecretManager,
    valkey_seed: Option<&str>,
) -> anyhow::Result<AdminState> {
    db::initial_admin::close_for_existing_users(&pool).await?;
    let shared_valkey = resolve_shared_valkey(&pool, &relay_secret_manager, valkey_seed).await?;
    build_postgres_admin_state(
        config,
        pool,
        lease_pool,
        relay_secret_manager,
        shared_valkey,
    )
    .await
}
