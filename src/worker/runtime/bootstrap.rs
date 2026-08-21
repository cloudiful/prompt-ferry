use crate::{
    bridge_crypto::{self},
    config::{WorkerConfig, normalize_relay_url},
    db,
    llm_review::{self},
    redact,
    relay_secrets::RelaySecretManager,
    replay_cache::ReplayCache,
    runtime_env, tls,
    worker_admin::{self, AdminState},
};
use anyhow::anyhow;
use std::{sync::Arc, time::Duration};
use tracing::{error, warn};

pub(super) fn validate_config(config: &WorkerConfig) -> anyhow::Result<()> {
    if config.worker_token.is_empty() {
        return Err(anyhow!("worker token is required"));
    }
    if config.mode().is_shared_managed() {
        RelaySecretManager::from_base64(&config.relay_secret_master_key)?;
    } else {
        runtime_env::resolve_standalone_database_path(&config.standalone_database_path)?;
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
    if !config.mode().is_shared_managed() && config.upstream_api_key.is_empty() {
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
    if !config.mode().is_shared_managed() {
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
    use super::validate_config;
    use crate::config::WorkerConfig;
    use base64::Engine as _;

    #[test]
    fn empty_database_url_selects_named_standalone_mode() {
        let config = WorkerConfig {
            upstream_api_key: "bootstrap-key".to_string(),
            worker_token: "token".to_string(),
            ..WorkerConfig::default()
        };

        assert_eq!(config.mode().as_str(), "standalone-managed");
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

        assert_eq!(config.mode().as_str(), "shared-managed");
        assert!(validate_config(&config).is_ok());
    }
}
