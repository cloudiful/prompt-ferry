use prompt_ferry::{config::NativeApiSource, db, worker};

use crate::relay_harness::worker_config;

pub async fn spawn_worker(
    worker_addr: std::net::SocketAddr,
    upstream_addr: std::net::SocketAddr,
    database_url: &str,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    let pool = db::connect(database_url).await.unwrap();
    let endpoint = db::create_endpoint(
        &pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "replay-default-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
            proxy_url: None,
            active_windows: None,
        },
    )
    .await
    .unwrap();
    db::create_model_endpoint_rule(
        &pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "*".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                native_api: prompt_ferry::config::NativeApi::Auto,
                proxy_url_override: None,
                active_windows: None,
                dev_system_normalize: false,
                thinking_effort_override: None,
            }],
        },
    )
    .await
    .unwrap();
    pool.close().await;

    let config = worker_config(worker_addr, upstream_addr, database_url);
    tokio::spawn(async move {
        worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    })
}
