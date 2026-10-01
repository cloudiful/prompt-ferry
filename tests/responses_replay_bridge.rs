use axum::http::StatusCode;
use prompt_ferry::{config, relay};
use serde_json::Value;

/// Spawn the relay's public and worker listeners on ephemeral ports. The public
/// listener serves the client-facing API surface under test.
async fn spawn_relay() -> std::net::SocketAddr {
    let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_addr = public_listener.local_addr().unwrap();
    let worker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_addr = worker_listener.local_addr().unwrap();
    let (public_app, worker_app, _handle) = relay::apps(config::RelayConfig {
        bind: public_addr.to_string(),
        worker_bind: worker_addr.to_string(),
        client_token: "client-token".to_string(),
        worker_token: "worker-token".to_string(),
        request_timeout_seconds: 5,
        ..config::RelayConfig::default()
    });
    tokio::spawn(async move {
        axum::serve(
            public_listener,
            public_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    tokio::spawn(async move {
        axum::serve(
            worker_listener,
            worker_app.into_make_service_with_connect_info::<relay::RemoteAddr>(),
        )
        .await
        .unwrap();
    });
    public_addr
}

#[tokio::test]
async fn creates_responses_conversation_via_public_api() -> anyhow::Result<()> {
    let relay_addr = spawn_relay().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/conversations"))
        .bearer_auth("client-token")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.json::<Value>().await?;
    assert_eq!(body["object"].as_str(), Some("conversation"));
    assert!(
        body["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("conv_"))
    );
    assert!(body["created_at"].as_i64().is_some());
    assert_eq!(body["metadata"], serde_json::json!({}));
    Ok(())
}
