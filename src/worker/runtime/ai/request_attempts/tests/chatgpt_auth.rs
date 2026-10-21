//! Issue #1004 P1: the ChatGPT subscription credential resolution.
//!
//! A route hydrated as subscription-eligible must never fall back to the
//! platform path with an empty bearer. A missing or unreadable credential
//! fails closed with an actionable login prerequisite.

use super::super::resolve_chatgpt_auth;
use crate::{
    config::{NativeApi, NativeApiSource},
    db::{ConfigRepository, EndpointCreate, EndpointOAuthTokenSet, EndpointProvider},
    relay_secrets::RelaySecretManager,
    standalone_config::{StandaloneConfig, StandaloneConfigStore},
    worker::runtime::{
        WorkerRuntimeState,
        context::{ResponseLimits, RuntimeServices},
        standalone::StandaloneRuntimeState,
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{path::PathBuf, sync::Arc};

struct ChatgptAuthFixture {
    services: RuntimeServices,
    repository: ConfigRepository,
    path: PathBuf,
}

impl ChatgptAuthFixture {
    async fn open() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prompt-ferry-chatgpt-auth-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let store = Arc::new(StandaloneConfigStore::open(&path).await.expect("store"));
        let manager =
            RelaySecretManager::from_base64(&STANDARD.encode([23_u8; 32])).expect("secret manager");
        let state = StandaloneRuntimeState::new(
            store.clone(),
            manager.clone(),
            StandaloneConfig::default(),
        );
        let (out_tx, _bridge_log) = super::spawn_bridge_log();
        let services = RuntimeServices::new(
            None,
            out_tx,
            reqwest::Client::new(),
            WorkerRuntimeState::default(),
            ResponseLimits::default(),
        )
        .with_standalone_state(state);
        Self {
            services,
            repository: ConfigRepository::sqlite(store, manager),
            path,
        }
    }

    async fn create_openai_endpoint(&self) -> uuid::Uuid {
        let endpoint_id = uuid::Uuid::new_v4();
        self.repository
            .create_endpoint(endpoint_id, endpoint_create(), false)
            .await
            .expect("create endpoint");
        endpoint_id
    }

    async fn store_credential(&self, endpoint_id: uuid::Uuid) {
        self.repository
            .set_endpoint_oauth_token(
                endpoint_id,
                Some(EndpointOAuthTokenSet {
                    access_token: "access-token".to_string(),
                    refresh_token: "refresh-token".to_string(),
                    expires_at: Some(chrono::Utc::now() + chrono::Duration::seconds(3600)),
                }),
            )
            .await
            .expect("store token");
    }

    async fn cleanup(self) {
        let _ = std::fs::remove_file(self.path);
    }
}

fn endpoint_create() -> EndpointCreate {
    EndpointCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        name: format!("chatgpt-auth-{}", uuid::Uuid::new_v4()),
        provider: EndpointProvider::OpenAi,
        provider_region: None,
        service_tier: Default::default(),
        base_url: "https://api.openai.com/v1".to_string(),
        native_api: NativeApi::Responses,
        native_api_source: NativeApiSource::Manual,
        api_key: String::new(),
        api_keys: Vec::new(),
        key_lb_enabled: false,
        enabled: true,
        proxy_url: None,
        active_windows: None,
    }
}

fn openai_route(endpoint_id: uuid::Uuid, has_oauth_token: bool) -> crate::db::RouteConfig {
    crate::db::RouteConfig {
        route_id: endpoint_id,
        user_id: 1,
        model_route_rule_id: None,
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: String::new(),
        endpoint_key_id: None,
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Responses,
        upstream_model: None,
        route_selection_reason: crate::db::RouteSelectionReason::Default,
        provider: EndpointProvider::OpenAi,
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: crate::db::CompactMode::Passthrough,
        thinking_downgrade_enabled: false,
        has_oauth_token,
    }
}

#[tokio::test]
async fn a_subscription_route_without_a_stored_credential_fails_closed() {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let fixture = ChatgptAuthFixture::open().await;
    let endpoint_id = fixture.create_openai_endpoint().await;
    let route = openai_route(endpoint_id, true);

    let error = resolve_chatgpt_auth(&fixture.services, &route)
        .await
        .expect_err("a subscription route without a stored credential must fail closed");
    assert_eq!(error.code, "chatgpt_login_required");
    assert_eq!(error.status.as_u16(), 401);
    assert!(
        error.message.contains("ChatGPT login"),
        "the error must name the missing prerequisite: {}",
        error.message
    );
    assert!(
        !error.message.contains("access-token"),
        "the error must never carry a credential"
    );

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_route_without_a_stored_credential_keeps_the_platform_path() {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let fixture = ChatgptAuthFixture::open().await;
    let endpoint_id = fixture.create_openai_endpoint().await;
    let route = openai_route(endpoint_id, false);

    let auth = resolve_chatgpt_auth(&fixture.services, &route)
        .await
        .expect("a platform route resolves without a subscription credential");
    assert!(auth.is_none(), "the platform path must stay available");

    fixture.cleanup().await;
}

#[tokio::test]
async fn a_subscription_route_resolves_its_stored_credential() {
    let _redaction_guard = crate::redact_test_support::lock_async().await;
    let fixture = ChatgptAuthFixture::open().await;
    let endpoint_id = fixture.create_openai_endpoint().await;
    fixture.store_credential(endpoint_id).await;
    let route = openai_route(endpoint_id, true);

    let auth = resolve_chatgpt_auth(&fixture.services, &route)
        .await
        .expect("a stored credential resolves")
        .expect("a stored subscription credential is used");
    assert_eq!(auth.access_token, "access-token");
    assert!(!auth.refreshed, "a valid credential is used as stored");

    fixture.cleanup().await;
}
