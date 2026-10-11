use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    config::NativeApi,
    db::{CompactMode, EndpointProvider, RouteConfig, RouteSelectionReason},
    worker::runtime::{
        WorkerRuntimeState,
        compaction::run_self_summarize_compact,
        context::{BridgeSender, ResponseLimits, RuntimeServices},
    },
    worker_admin::{
        quota_feedback::{QuotaFeedbackEligibility, QuotaFeedbackScope},
        token_plan_cache::TokenPlanQuotaCache,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn spawn_response(
    status: u16,
    reason: &'static str,
    headers: &'static str,
    body: &'static [u8],
) -> (SocketAddr, Arc<AtomicUsize>) {
    let count = Arc::new(AtomicUsize::new(0));
    let counter = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut request = [0u8; 8192];
                let _ = socket.read(&mut request).await;
                let response = format!(
                    "HTTP/1.1 {status} {reason}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (address, count)
}

fn route(address: SocketAddr) -> RouteConfig {
    RouteConfig {
        route_id: uuid::Uuid::new_v4(),
        user_id: 1,
        model_route_rule_id: None,
        base_url: format!("http://{address}"),
        api_key: "fixture-key".to_string(),
        endpoint_key_id: Some(uuid::Uuid::new_v4()),
        endpoint_key_label: None,
        api_keys: Vec::new(),
        key_lb_enabled: false,
        native_api: NativeApi::Chat,
        upstream_model: None,
        route_selection_reason: RouteSelectionReason::Default,
        provider: EndpointProvider::Minimax,
        service_tier: None,
        proxy_url: None,
        dev_system_normalize: false,
        thinking_effort_override: None,
        compact_mode: CompactMode::SelfSummarize,
        thinking_downgrade_enabled: false,
    }
}

fn services() -> RuntimeServices {
    let (out_tx, _control_rx, _data_rx) = BridgeSender::channel();
    RuntimeServices::new(
        None,
        out_tx,
        reqwest::Client::new(),
        WorkerRuntimeState::default(),
        ResponseLimits::default(),
    )
}

#[tokio::test]
async fn internal_compact_429_records_the_selected_key_cooldown() {
    let (address, count) = spawn_response(
        429,
        "Too Many Requests",
        "Content-Type: application/json\r\nRetry-After: 12\r\n",
        br#"{"error":{"message":"rate limited"}}"#,
    )
    .await;
    let route = route(address);
    let services = services();
    let cache = TokenPlanQuotaCache::default();
    let scope = QuotaFeedbackScope::new(route.route_id, route.endpoint_key_id);
    let factory = crate::worker::runtime::ai::quota_feedback::AttemptFeedbackFactory::new(
        Some(&cache),
        &route,
        None,
    );

    let result = run_self_summarize_compact(
        &reqwest::Client::new(),
        &format!("http://{address}/v1/chat/completions"),
        &route.api_key,
        NativeApi::Chat,
        br#"{"model":"gpt-test","input":"summarize this"}"#,
        Some("gpt-test"),
        &services,
        &route,
        &factory,
    )
    .await;

    assert!(
        result.is_err(),
        "an upstream 429 must fail the compact call"
    );
    assert_eq!(count.load(Ordering::SeqCst), 1, "compact sends one POST");
    assert_eq!(
        cache
            .feedback
            .eligibility(scope, Instant::now() + Duration::from_secs(1)),
        QuotaFeedbackEligibility::Cooling
    );
}
