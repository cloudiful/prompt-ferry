use crate::{
    bridge_crypto::{self},
    config::{WorkerConfig, normalize_relay_url},
    db,
    llm_review::{self},
    redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    tls,
    worker_admin::{self, AdminState},
};
use anyhow::anyhow;
use std::time::Duration;
use tracing::{error, warn};

pub(super) fn validate_config(config: &WorkerConfig) -> anyhow::Result<()> {
    if config.worker_token.is_empty() {
        return Err(anyhow!("worker token is required"));
    }
    let managed_mode = !config.database_url.trim().is_empty();
    if managed_mode {
        RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    } else {
        let relay_urls = config
            .relay_urls
            .iter()
            .map(|relay_url| normalize_relay_url(relay_url))
            .filter(|relay_url| !relay_url.is_empty())
            .collect::<Vec<_>>();
        if relay_urls.is_empty() {
            return Err(anyhow!("at least one relay URL is required"));
        }
        let unique_relay_urls = relay_urls
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if unique_relay_urls.len() != relay_urls.len() {
            return Err(anyhow!(
                "worker relay URLs must be unique after normalization"
            ));
        }
        tls::validate_worker_config(config)?;
        bridge_crypto::validate_settings(
            "worker",
            config.bridge_encryption_mode,
            &config.bridge_encryption_key,
        )?;
    }
    if config.database_url.is_empty() && config.upstream_api_key.is_empty() {
        return Err(anyhow!("upstream api key is required"));
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

pub(super) async fn build_admin_state(
    config: &WorkerConfig,
    spawn_admin_server: bool,
) -> anyhow::Result<Option<AdminState>> {
    if config.database_url.trim().is_empty() {
        return Ok(None);
    }
    let relay_secret_manager = RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    let pool = db::connect(&config.database_url).await?;
    let lease_pool = db::connect_with_max_connections(&config.database_url, 2).await?;
    db::migrate(&pool).await?;
    db::bootstrap_admin(
        &pool,
        &config.bootstrap_admin_login,
        &config.bootstrap_admin_password,
    )
    .await?;
    if let Err(err) = db::prune_usage_events(&pool, 90).await {
        warn!(error = %err, "failed to prune usage events");
    }
    let redaction_config = db::get_redaction_config(&pool).await?;
    let user_redaction_configs = db::list_user_redaction_configs(&pool).await?;
    redact::apply_configs(&redaction_config, user_redaction_configs)?;
    let redaction_enabled = redact::has_any_enabled();
    let llm_review_settings = db::get_json_setting(&pool, llm_review::LLM_REVIEW_SETTINGS_KEY)
        .await?
        .unwrap_or_default();
    let model_route_whitelist_enabled =
        db::get_bool_setting(&pool, "model_route_whitelist_enabled", true).await?;
    let request_content_logging = db::get_request_content_logging(&pool).await?;
    let stream_delta_batching = db::get_stream_delta_batching(&pool).await?;
    let aborted_count = db::abort_pending_approval_requests(&pool).await?;
    if aborted_count > 0 {
        warn!(
            count = aborted_count,
            "aborted stale pending approval requests on startup"
        );
    }
    let stale_request_count = db::abort_stale_request_records(&lease_pool).await?;
    if stale_request_count > 0 {
        warn!(
            count = stale_request_count,
            "aborted stale leased request records on startup"
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
        stream_delta_batching,
        llm_review_settings,
        mcp_catalog_cache,
        mcp_catalog_service,
        mcp_session_store: crate::mcp::McpSessionStore::from_config(config).await,
        endpoint_model_cache: crate::endpoint_models::EndpointModelCache::new(Duration::from_secs(
            config.endpoint_model_cache_ttl_seconds.max(1),
        )),
    });
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
    }
    Ok(Some(state))
}
