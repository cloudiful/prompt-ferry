use base64::{Engine as _, engine::general_purpose::STANDARD};
use prompt_ferry::{
    config::{self, NativeApi},
    relay,
};

pub async fn spawn_relay() -> (
    std::net::SocketAddr,
    std::net::SocketAddr,
    relay::RelayHandle,
) {
    let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_addr = public_listener.local_addr().unwrap();
    let worker_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_addr = worker_listener.local_addr().unwrap();
    let (public_app, worker_app, handle) = relay::apps(config::RelayConfig {
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
    (public_addr, worker_addr, handle)
}

pub fn worker_config(
    worker_addr: std::net::SocketAddr,
    upstream_addr: std::net::SocketAddr,
    database_url: &str,
) -> config::WorkerConfig {
    config::WorkerConfig {
        relay_urls: vec![format!("ws://{worker_addr}/ws/worker")],
        worker_token: "worker-token".to_string(),
        upstream_base_url: format!("http://{upstream_addr}"),
        upstream_api_key: "upstream-key".to_string(),
        upstream_native_api: NativeApi::Chat,
        connect_timeout_seconds: 5,
        database_url: database_url.to_string(),
        bootstrap_admin_password: "password-123".to_string(),
        relay_secret_master_key: STANDARD.encode([5_u8; 32]),
        ..config::WorkerConfig::default()
    }
}

pub async fn wait_for_worker(
    handle: &relay::RelayHandle,
    worker_handle: &mut tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    for _ in 0..200 {
        if handle.worker_count().await > 0 {
            return;
        }
        tokio::select! {
            result = &mut *worker_handle => panic!("worker exited before connecting: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }
    panic!("worker did not connect");
}
