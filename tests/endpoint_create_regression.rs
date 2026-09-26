//! Issue #599 R2e.2: endpoint create/update success responses.
//!
//! The create and update handlers used to re-fetch the endpoint after a
//! committed write and could answer `not_found: endpoint not found` when that
//! follow-up read missed. The fix reuses the repository write result, so these
//! tests pin the success contract end to end through the SQLite-backed admin
//! router: the response echoes the write result (and the in-handler
//! transitions) and the row is actually persisted. The fixture, request
//! builder, and payload helpers live in `support/endpoint_create_fixture.rs`.

#[path = "support/endpoint_create_fixture.rs"]
mod fixture;

use axum::http::StatusCode;
use prompt_ferry::{
    db::{self, EndpointOAuthTokenSet, EndpointProvider},
    worker_admin,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use crate::fixture::{Fixture, json_body, openai_endpoint_body, request};

#[tokio::test]
async fn create_and_update_endpoint_echo_the_committed_write() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());

    let created = app
        .clone()
        .oneshot(request(
            "POST",
            "/api/v1/admin/endpoints".to_string(),
            Some(openai_endpoint_body("openai-platform", "sk-platform-1")),
        ))
        .await?;
    assert_eq!(
        created.status(),
        StatusCode::OK,
        "create must answer from the committed write, never a false 404"
    );
    let created_body = json_body(created).await;
    let endpoint_id = created_body["endpoint_id"]
        .as_str()
        .expect("created endpoint id")
        .parse::<Uuid>()
        .expect("endpoint id is a UUID");
    assert_eq!(created_body["name"], "openai-platform");
    assert_eq!(created_body["provider"], "openai");
    assert_eq!(created_body["plan"], "platform_api_key");
    assert_eq!(created_body["has_oauth_token"], false);
    assert_eq!(created_body["mcp_enabled"], false);
    let created_keys = created_body["api_keys"]
        .as_array()
        .expect("created api keys");
    assert_eq!(created_keys.len(), 1);
    assert_eq!(created_keys[0]["key_label"], "openai-platform");

    // The success response must describe a persisted row, not a transient one.
    let stored = fixture
        .state
        .config_repository
        .get_endpoint(endpoint_id)
        .await?
        .expect("created endpoint is persisted");
    assert_eq!(stored.name, "openai-platform");

    let updated = app
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/endpoints/{endpoint_id}"),
            Some(openai_endpoint_body(
                "openai-platform-renamed",
                "sk-platform-2",
            )),
        ))
        .await?;
    assert_eq!(
        updated.status(),
        StatusCode::OK,
        "update must answer from the committed write, never a false 404"
    );
    let updated_body = json_body(updated).await;
    assert_eq!(updated_body["endpoint_id"], endpoint_id.to_string());
    assert_eq!(updated_body["name"], "openai-platform-renamed");
    assert_eq!(updated_body["plan"], "platform_api_key");
    assert_eq!(updated_body["has_oauth_token"], false);
    assert_eq!(updated_body["mcp_enabled"], false);

    let stored = fixture
        .state
        .config_repository
        .get_endpoint(endpoint_id)
        .await?
        .expect("updated endpoint is persisted");
    assert_eq!(stored.name, "openai-platform-renamed");

    fixture.cleanup().await;
    Ok(())
}

#[tokio::test]
async fn update_derives_plan_and_token_presence_from_the_write() -> anyhow::Result<()> {
    let fixture = Fixture::open().await?;
    let app = worker_admin::router(fixture.state.clone());

    let endpoint_id = Uuid::new_v4();
    fixture
        .state
        .config_repository
        .create_endpoint(
            endpoint_id,
            db::EndpointCreate {
                scope: "admin".to_string(),
                owner_user_id: None,
                name: "openai-subscription".to_string(),
                provider: EndpointProvider::OpenAi,
                provider_region: None,
                service_tier: Default::default(),
                base_url: "https://api.openai.com/v1".to_string(),
                native_api: prompt_ferry::config::NativeApi::Chat,
                native_api_source: prompt_ferry::config::NativeApiSource::Manual,
                api_key: "sk-platform".to_string(),
                api_keys: Vec::new(),
                key_lb_enabled: false,
                enabled: true,
                proxy_url: None,
                active_windows: None,
            },
            false,
        )
        .await?;
    fixture
        .state
        .config_repository
        .set_endpoint_oauth_token(
            endpoint_id,
            Some(EndpointOAuthTokenSet {
                access_token: "access-token".to_string(),
                refresh_token: "refresh-token".to_string(),
                expires_at: None,
            }),
        )
        .await?;

    // A token-preserving PATCH must report the derived subscription plan even
    // though the repository write result does not stamp it.
    let kept = app
        .clone()
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/endpoints/{endpoint_id}"),
            Some(openai_endpoint_body("openai-subscription", "sk-platform")),
        ))
        .await?;
    assert_eq!(kept.status(), StatusCode::OK);
    let kept_body = json_body(kept).await;
    assert_eq!(kept_body["has_oauth_token"], true);
    assert_eq!(kept_body["plan"], "chatgpt_subscription");

    // An explicit platform plan clears the token; the response must reflect the
    // cleared credential in the same write.
    let mut clearing_body = openai_endpoint_body("openai-subscription", "sk-platform");
    clearing_body["plan"] = json!("platform_api_key");
    let cleared = app
        .oneshot(request(
            "PATCH",
            format!("/api/v1/admin/endpoints/{endpoint_id}"),
            Some(clearing_body),
        ))
        .await?;
    assert_eq!(cleared.status(), StatusCode::OK);
    let cleared_body = json_body(cleared).await;
    assert_eq!(cleared_body["has_oauth_token"], false);
    assert_eq!(cleared_body["plan"], "platform_api_key");

    let stored = fixture
        .state
        .config_repository
        .get_endpoint(endpoint_id)
        .await?
        .expect("endpoint is persisted");
    assert!(!stored.has_oauth_token);
    assert_eq!(stored.plan, db::EndpointPlan::PlatformApiKey);

    fixture.cleanup().await;
    Ok(())
}
