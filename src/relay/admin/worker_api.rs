//! The worker's business API, reached through the relay.
//!
//! A relay has no worker business persistence of its own, so a request the
//! relay does not own goes over the bridge to whichever worker is attached. With
//! no worker attached the caller gets a precise "not connected" answer instead of
//! a gateway error, so the management page stays readable and only the worker
//! views report that there is nothing behind them.

use axum::{
    body::Body,
    extract::{Extension, OriginalUri, State},
    http::{HeaderMap, Method, Uri},
    response::Response,
};

use crate::relay::{
    request_compression::HttpRequestCompressionContext,
    worker_proxy::{WorkerRequest, forward_to_worker, no_worker_connected},
};

use super::state::RelayAdminState;

pub(super) async fn proxy_worker_admin(
    State(state): State<RelayAdminState>,
    Extension(compression): Extension<HttpRequestCompressionContext>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Body,
) -> Response {
    // The worker serves its business API under the same `/api/v1` prefix this
    // listener does, so the request target is forwarded as it arrived and no
    // business rewrite is involved.
    //
    // The URI this handler is given is *not* that one: the relay control plane is
    // mounted with `nest`, which strips `/api/v1` before the fallback runs, so a
    // request for `/api/v1/admin/endpoints` reaches this handler as
    // `/admin/endpoints`. Rebuilding the worker's path from that would drop
    // everything the mount point owns and send every business request to the
    // worker's root, so the original URI is taken instead.
    let path = worker_request_target(&uri);
    forward_to_worker(
        &state.relay,
        WorkerRequest {
            method: &method,
            path: &path,
            headers: &headers,
            compression,
            body,
        },
        no_worker_connected,
    )
    .await
}

/// The request target to forward to the worker.
///
/// Everything the caller sent is preserved, including the query string the
/// worker's own handlers read their filters and paging from — dropping it would
/// silently answer a different question than the caller asked. The one thing
/// normalised is the mount point on its own: `/api/v1` and `/api/v1/` name the
/// same API root, and the relay treats them as one unknown API path, so the
/// worker sees one spelling of it rather than two that mean the same thing.
fn worker_request_target(uri: &Uri) -> String {
    let (path, query) = match uri.path_and_query() {
        Some(target) => (target.path(), target.query()),
        None => (uri.path(), None),
    };
    let mut target = String::from(if path == "/api/v1" { "/api/v1/" } else { path });
    if let Some(query) = query {
        target.push('?');
        target.push_str(query);
    }
    target
}

#[cfg(test)]
mod tests {
    use crate::relay::test_handle;
    use axum::extract::OriginalUri;

    /// A router shaped like the one that serves this handler.
    ///
    /// The unit-level fact it pins down is that the nested fallback sees a URI
    /// with the mount point stripped while the original URI still carries it.
    /// Asserting that here keeps the reason for `OriginalUri` in this file rather
    /// than only in a test that happens to exercise the whole router.
    #[tokio::test]
    async fn the_nested_fallback_sees_a_stripped_uri_and_an_intact_original() {
        use axum::{
            Router,
            body::Body,
            http::{Request, Uri},
            routing::any,
        };
        use tower::ServiceExt as _;

        async fn probe(uri: Uri, OriginalUri(original): OriginalUri) -> String {
            format!("{uri}|{original}")
        }

        let (_relay, handle) = test_handle(crate::config::RelayConfig::default());
        let app: Router = Router::new()
            .route("/api/v1/", any(probe))
            .nest("/api/v1", Router::new().fallback(probe))
            .with_state(handle);

        for (request, expected) in [
            ("/api/v1", "/|/api/v1"),
            ("/api/v1/", "/api/v1/|/api/v1/"),
            (
                "/api/v1/admin/endpoints",
                "/admin/endpoints|/api/v1/admin/endpoints",
            ),
            (
                "/api/v1/admin/request-records/summary?rows=5",
                "/admin/request-records/summary?rows=5|/api/v1/admin/request-records/summary?rows=5",
            ),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(request)
                        .body(Body::empty())
                        .expect("build a request"),
                )
                .await
                .expect("the router answers");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read the body");
            assert_eq!(
                String::from_utf8_lossy(&body),
                expected,
                "request {request}"
            );
        }
    }
}
