//! Owner isolation for personal MCP OAuth through the real SQLite admin
//! router: an admin who does not own the row is rejected before any provider
//! request for test, status and start.

#[path = "support/endpoint_create_fixture.rs"]
mod fixture;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Router, http::StatusCode, routing::any};
use prompt_ferry::{
    db::{McpServerInput, UserCreate},
    worker_admin,
};
use serde_json::json;
use tower::ServiceExt;
use uuid::Uuid;

use crate::fixture::{Fixture, request};

async fn spawn_counting_mcp() -> (String, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = Router::new().route(
        "/mcp",
        any(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                (StatusCode::ACCEPTED, "")
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}/mcp"), hits)
}

async fn personal_oauth_server(fixture: &Fixture, url: String, owner: i64) -> Uuid {
    let server_id = Uuid::new_v4();
    fixture
        .state
        .config_repository
        .create_mcp_server(
            server_id,
            McpServerInput {
                scope: "user".to_string(),
                owner_user_id: Some(owner),
                source_endpoint_id: None,
                name: "other-owner-oauth".to_string(),
                aggregate_naming_mode: "qualified_only".to_string(),
                transport: "http".to_string(),
                provider_kind: None,
                url: Some(url),
                command: None,
                args: json!([]),
                env_json: json!({}),
                bearer_tokens_json: json!([]),
                http_headers_json: json!({}),
                auth_mode: "oauth".to_string(),
                basic_username: None,
                basic_password: None,
                proxy_url: None,
                tool_filter_mode: "blacklist".to_string(),
                allowed_tools: json!([]),
                disabled_tools: json!([]),
                disabled_resources: json!([]),
                enabled: true,
                timeout_ms: 30_000,
                lifecycle_policy: "auto".to_string(),
                lifecycle_manual_protocol_version: None,
            },
        )
        .await
        .expect("create oauth server");
    server_id
}

#[tokio::test]
async fn admin_cannot_test_status_or_start_another_owners_oauth_server() {
    let _ = fixture::json_body;
    let fixture = Fixture::open().await.expect("fixture");
    let owner = fixture
        .state
        .user_store
        .create_user(UserCreate {
            login_name: "oauth-owner".to_string(),
            password_hash: "hash".to_string(),
            display_name: "OAuth Owner".to_string(),
            is_admin: false,
        })
        .await
        .expect("create owner");
    let (url, hits) = spawn_counting_mcp().await;
    let server_id = personal_oauth_server(&fixture, url, owner.user_id).await;
    let app = worker_admin::router(fixture.state.clone());

    for (method, path) in [
        (
            "POST",
            format!("/api/v1/admin/mcp-servers/{server_id}/test"),
        ),
        (
            "GET",
            format!("/api/v1/admin/mcp-servers/{server_id}/oauth"),
        ),
        (
            "POST",
            format!("/api/v1/admin/mcp-servers/{server_id}/oauth"),
        ),
    ] {
        let request = request(method, path.clone(), None);
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{method} {path} must reject a non-owner admin"
        );
    }

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "owner rejection must happen before any MCP/AS request"
    );
    fixture.cleanup().await;
}
