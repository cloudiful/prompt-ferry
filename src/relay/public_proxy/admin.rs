//! The public listener's fallback to the worker's management API.
//!
//! Every path the relay does not serve itself is handed to a connected worker
//! over the bridge, which is the authenticated Worker-admin proxy contract this
//! listener has always offered. The relay's own management listener uses the
//! same forwarding core for the worker's business API, so the two entry points
//! cannot drift apart.

use axum::{
    body::Body,
    extract::{ConnectInfo, Extension, State},
    http::{HeaderMap, Method, Uri},
    response::{IntoResponse, Response},
};

use super::{
    super::{
        request_compression::HttpRequestCompressionContext,
        state::{AppState, RemoteAddr},
        worker_proxy::{WorkerRequest, forward_to_worker},
    },
    enforce_public_ip_policy, no_worker_response,
};

pub(super) async fn proxy_admin_ui(
    State(state): State<AppState>,
    ConnectInfo(peer_addr): ConnectInfo<RemoteAddr>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    uri: Uri,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    if let Err(err) = enforce_public_ip_policy(&state, peer_addr.0.ip(), &headers).await {
        return err.into_response();
    }

    forward_to_worker(
        &state,
        WorkerRequest {
            method: &method,
            path: &uri.to_string(),
            headers: &headers,
            compression,
            body,
        },
        || no_worker_response(false),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::super::public_router;
    use crate::relay::{RemoteAddr, state::test_state};
    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt as _;

    /// The public listener keeps the fallback contract: with no worker connected
    /// an unknown path answers with the existing `no_worker` envelope, not with
    /// a relay-owned route.
    #[tokio::test]
    async fn an_unconnected_relay_keeps_answering_the_public_fallback_with_no_worker() {
        let (mut parts, body) = Request::builder()
            .uri("/api/v1/admin/endpoints")
            .body(Body::empty())
            .unwrap()
            .into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "203.0.113.4:51000".parse().expect("an address"),
        )));

        let response = public_router(test_state())
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the public router answers");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read the body");
        let body = std::str::from_utf8(&body).expect("JSON body");
        assert!(body.contains("no_worker"), "got: {body}");
        assert!(
            !body.contains("worker_not_connected"),
            "the public fallback keeps its own wording, got: {body}"
        );
    }
}
