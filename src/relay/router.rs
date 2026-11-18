use crate::{config::RelayConfig, ip_acl::CompiledRelayIpPolicy};

use super::{
    public_proxy::{public_api_router, public_router},
    state::{AppState, RelayState},
};
use axum::{Router, body::Body, response::Response};
use futures::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicUsize},
};
use tokio::sync::Mutex;

/// The node router and the relay state it serves.
///
/// The node runtime owns one listener that serves the public AI surface and
/// the worker admin API at the same time. The admin router owns the single
/// SPA fallback of that listener; the public routes are merged into it and
/// carry no fallback of their own.
///
/// The static client token is disabled for the node: the public AI surface
/// authenticates managed client keys only, and the master secret never
/// authenticates an AI request.
pub(crate) fn node_apps(config: RelayConfig, admin_router: Router) -> (Router, AppState) {
    let inner = relay_state();
    let state = AppState {
        config,
        inner,
        managed_only_auth: true,
    };
    let public_app = public_api_router(state.clone());
    let app = merge_node_router(admin_router, public_app);
    (app, state)
}

/// The router of a database-less edge node.
///
/// The edge serves the public API itself and forwards everything else — the
/// administration pages, the API and the assets — to its database node. It
/// has no local administration listener and no static client token: the
/// managed keys of the database node are the only credential.
pub(crate) fn node_edge_apps(config: RelayConfig) -> (Router, AppState) {
    let inner = relay_state();
    let state = AppState {
        config,
        inner,
        managed_only_auth: true,
    };
    (public_router(state.clone()), state)
}

/// Merge the admin router with the public API routes.
///
/// The admin router carries the custom SPA fallback and the public router
/// only the default one, so the merge keeps the admin fallback and the
/// public routes win over it. Merging two routers that both carry a custom
/// fallback would panic, which is why the public routes are extracted
/// without one.
pub(crate) fn merge_node_router(admin_router: Router, public_router: Router) -> Router {
    admin_router.merge(public_router)
}

fn relay_state() -> Arc<RelayState> {
    Arc::new(RelayState {
        workers: Mutex::new(HashMap::new()),
        worker_loads: Mutex::new(HashMap::new()),
        pending: Mutex::new(HashMap::new()),
        pending_mcp: Mutex::new(HashMap::new()),
        pending_realtime_sessions: Mutex::new(HashMap::new()),
        routes: Mutex::new(HashMap::new()),
        relay_ip_policy: Mutex::new(CompiledRelayIpPolicy::default()),
        config_version: Mutex::new(None),
        next_worker_id: AtomicUsize::new(1),
    })
}

pub(crate) async fn drain_body_then(body: Body, response: Response) -> Response {
    let mut stream = body.into_data_stream();
    while let Some(next) = stream.next().await {
        if next.is_err() {
            break;
        }
    }
    response
}

#[cfg(test)]
mod node_router_tests {
    use super::{merge_node_router, public_api_router};
    use crate::relay::state::{RemoteAddr, test_state};
    use axum::{
        Router,
        body::Body,
        extract::connect_info::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;

    /// A stand-in for the worker admin router: it owns the SPA fallback of
    /// the node listener.
    fn admin_router() -> Router {
        Router::new()
            .route("/api/v1/healthz", get(|| async { "admin" }))
            .fallback(get(|| async { "spa" }))
    }

    fn request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("request builds")
    }

    #[tokio::test]
    async fn the_node_router_merges_the_public_routes_with_the_admin_fallback() {
        let public = public_api_router(test_state());
        let app = merge_node_router(admin_router(), public);

        // The public route is served by the public handler.
        let mut healthz = request("/healthz");
        healthz.extensions_mut().insert(ConnectInfo(RemoteAddr(
            "127.0.0.1:1234".parse().expect("address parses"),
        )));
        let response = app
            .clone()
            .oneshot(healthz)
            .await
            .expect("the merged router answers");
        assert_eq!(response.status(), StatusCode::OK);

        // An unknown path still reaches the single SPA fallback of the
        // admin router.
        let response = app
            .oneshot(request("/some/spa/path"))
            .await
            .expect("the merged router answers");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn edge_fallback_uses_connected_bridge_without_a_peer_origin() {
        use crate::protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart};
        use crate::relay::worker_bridge::{dispatch_worker_bridge_message, register_worker};
        use axum::body::to_bytes;
        use tokio::sync::mpsc;

        let (app, state) = super::node_edge_apps(crate::config::RelayConfig::default());
        let (tx, mut rx) = mpsc::channel(16);
        assert!(register_worker(&state, 1, tx).await);
        let executor_state = state.clone();
        let executor = tokio::spawn(async move {
            for expected in ["/index.html", "/api/v1/auth/setup?x=1"] {
                let start = loop {
                    if let Some(BridgeMessage::RequestStart(start)) = rx.recv().await {
                        break start;
                    }
                };
                assert_eq!(start.path, expected);
                if expected.starts_with("/api/") {
                    assert!(
                        start
                            .headers
                            .iter()
                            .any(|(name, value)| name == "host" && value == "edge.example")
                    );
                }
                loop {
                    if let Some(BridgeMessage::RequestEnd(_)) = rx.recv().await {
                        break;
                    }
                }
                let request_id = start.request_id;
                dispatch_worker_bridge_message(
                    &executor_state,
                    BridgeMessage::ResponseStart(ResponseStart {
                        request_id: request_id.clone(),
                        status: 201,
                        content_type: Some("text/plain".into()),
                        headers: vec![("set-cookie".into(), "session=ok; HttpOnly".into())],
                    }),
                )
                .await;
                dispatch_worker_bridge_message(
                    &executor_state,
                    BridgeMessage::ResponseChunk(ResponseChunk {
                        request_id: request_id.clone(),
                        data: b"bridge response".to_vec(),
                    }),
                )
                .await;
                dispatch_worker_bridge_message(
                    &executor_state,
                    BridgeMessage::ResponseEnd(ResponseEnd { request_id }),
                )
                .await;
            }
        });
        for uri in ["/index.html", "/api/v1/auth/setup?x=1"] {
            let mut req = Request::builder()
                .uri(uri)
                .header("host", "edge.example")
                .body(Body::empty())
                .unwrap();
            req.extensions_mut()
                .insert(ConnectInfo(RemoteAddr("127.0.0.1:1234".parse().unwrap())));
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            assert_eq!(
                response.headers().get("set-cookie").unwrap(),
                "session=ok; HttpOnly"
            );
            assert_eq!(
                to_bytes(response.into_body(), usize::MAX).await.unwrap(),
                "bridge response"
            );
        }
        executor.await.unwrap();
    }

    #[tokio::test]
    async fn the_public_routes_of_a_node_have_no_fallback_of_their_own() {
        let app = public_api_router(test_state());
        let response = app
            .oneshot(request("/some/spa/path"))
            .await
            .expect("the public router answers");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "the node has exactly one fallback, and it belongs to the admin router"
        );
    }
}
