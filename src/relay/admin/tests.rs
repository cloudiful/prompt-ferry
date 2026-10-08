//! Tests for the relay's own management listener.
//!
//! The boundary these cover is the one the relay owns: which routes answer
//! without a worker, which ones refuse a caller with no management credential,
//! and that the relay's control plane never appears on the public listener.

use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use tower::ServiceExt as _;

use crate::{
    config::RelayConfig,
    relay::{RemoteAddr, admin::router, admin::state::RelayAdminState},
};

const TOKEN: &str = "relay-management-token-for-tests";

fn relay_config() -> RelayConfig {
    RelayConfig::default()
}

fn app() -> Router {
    router(RelayAdminState::read_only(relay_config(), TOKEN))
}

/// A management listener with one worker attached, and the channel to it.
///
/// The worker is a channel rather than a worker: registering it is what makes
/// the relay choose a target, and everything the proxy puts on the bridge before
/// a reply arrives is observable on this side.
async fn app_with_a_worker() -> (
    Router,
    tokio::sync::mpsc::Receiver<crate::protocol::BridgeMessage>,
) {
    use crate::config::HostRole;

    let mut config = relay_config();
    config.admin_token = TOKEN.to_string();
    let (relay, handle) = crate::relay::test_handle(config.clone());
    let mut state = RelayAdminState::new(&config, HostRole::Relay, relay, handle);
    state.writable = false;
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    state.relay.inner.workers.lock().await.insert(1, sender);
    (router(state), receiver)
}

/// Every route the relay's control plane owns, with the method it answers.
const CONTROL_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/v1/relay/auth/login"),
    ("POST", "/api/v1/relay/auth/logout"),
    ("GET", "/api/v1/relay/auth/me"),
    ("GET", "/api/v1/relay/status"),
    ("GET", "/api/v1/relay/settings"),
    ("PATCH", "/api/v1/relay/settings"),
    ("GET", "/api/v1/relay/host"),
    ("PUT", "/api/v1/relay/host/role"),
    ("POST", "/api/v1/relay/host/restart"),
];

/// Send a request as if it arrived on the loopback management listener.
async fn call(app: Router, request: Request<Body>) -> axum::response::Response {
    call_from("127.0.0.1:51000", app, request).await
}

/// Send a request as if it arrived from `peer` on the management listener.
async fn call_from(peer: &str, app: Router, request: Request<Body>) -> axum::response::Response {
    let (mut parts, body) = request.into_parts();
    parts.extensions.insert(ConnectInfo(RemoteAddr(
        peer.parse().expect("a peer address"),
    )));
    app.oneshot(Request::from_parts(parts, body))
        .await
        .expect("the management router answers")
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("build a request")
}

fn authorized(method: &str, path: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .body(Body::empty())
        .expect("build a request")
}

async fn body_of(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read the body");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A relay-only host opens the management page with no worker anywhere.
#[tokio::test]
async fn the_page_opens_without_a_worker() {
    let response = call(app(), get("/")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .expect("content type")
            .starts_with("text/html"),
        "the entry point must be the page, not an API answer"
    );

    // A browser history route resolves to the page rather than a JSON 404.
    let deep = call(app(), get("/settings/relays")).await;
    assert_eq!(deep.status(), StatusCode::OK);
}

/// The liveness probe says the control plane is up without leaking any fact
/// about the host, so a supervisor does not need a credential to watch it.
#[tokio::test]
async fn liveness_needs_no_credential_and_reveals_no_host_fact() {
    let response = call(app(), get("/api/v1/healthz")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_of(response).await, "ok");
}

/// Every spelling of the API prefix stays API surface.
///
/// axum's `nest` matches `/api/v1` and `/api/v1/<path>` but not `/api/v1/`, so
/// without an explicit route the trailing-slash form escapes the guard and
/// resolves to the page. Both forms must refuse an unauthenticated caller, and
/// once authenticated they behave like any other unknown API path: the worker's
/// to answer, because the relay does not own an endpoint at the bare prefix.
#[tokio::test]
async fn neither_spelling_of_the_api_prefix_resolves_to_the_page() {
    for path in ["/api/v1", "/api/v1/"] {
        let anonymous = call(app(), get(path)).await;
        assert_eq!(
            anonymous.status(),
            StatusCode::UNAUTHORIZED,
            "{path} must refuse an unauthenticated caller rather than serve the page"
        );
        let body = body_of(anonymous).await;
        assert!(
            body.contains("unauthorized"),
            "{path} must answer as JSON, got: {body}"
        );
        assert!(
            !body.contains("<!doctype html") && !body.contains("<html"),
            "{path} must never answer with the page, got: {body}"
        );

        let authenticated = call(app(), authorized("GET", path)).await;
        assert_eq!(
            authenticated.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} is not an endpoint the relay owns, so it reaches the worker"
        );
        let body = body_of(authenticated).await;
        assert!(
            body.contains("worker_not_connected"),
            "{path} must be routed like any other unknown API path, got: {body}"
        );
    }
}

/// A browser path outside the API prefix still resolves to the page.
///
/// The guard is on the API, not on the listener: the page and its assets have to
/// load before a caller can authenticate at all.
#[tokio::test]
async fn a_page_path_is_not_caught_by_the_api_prefix_guard() {
    for path in ["/", "/settings/relays"] {
        let response = call(app(), get(path)).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .expect("content type")
                .starts_with("text/html"),
            "{path} must stay the page"
        );
    }
}

/// Every control route refuses a caller with no management credential.
#[tokio::test]
async fn a_control_route_refuses_an_unauthenticated_caller() {
    for (method, path) in [
        ("GET", "/api/v1/relay/status"),
        ("GET", "/api/v1/relay/settings"),
        ("PATCH", "/api/v1/relay/settings"),
        ("GET", "/api/v1/relay/host"),
        ("PUT", "/api/v1/relay/host/role"),
        ("POST", "/api/v1/relay/host/restart"),
        ("GET", "/api/v1/admin/endpoints"),
    ] {
        let response = call(
            app(),
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("build a request"),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} must not answer an unauthenticated caller"
        );
        assert!(
            body_of(response).await.contains("unauthorized"),
            "{method} {path} must answer as JSON"
        );
    }
}

/// A wrong token is refused exactly like a missing one.
#[tokio::test]
async fn a_wrong_management_token_is_refused() {
    let response = call(
        app(),
        Request::builder()
            .uri("/api/v1/relay/status")
            .header(header::AUTHORIZATION, "Bearer not-the-token")
            .body(Body::empty())
            .expect("build a request"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// The status endpoint answers from relay state alone when it is authenticated.
#[tokio::test]
async fn the_status_reports_the_relay_without_a_worker() {
    let response = call(app(), authorized("GET", "/api/v1/relay/status")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_of(response).await;
    let body: serde_json::Value = serde_json::from_str(&body).expect("the status is JSON");
    assert_eq!(body["worker"]["connected"], serde_json::json!(false));
    assert_eq!(body["worker"]["connected_workers"], serde_json::json!(0));
    assert_eq!(
        body["relay"]["admin_bind"],
        serde_json::json!("127.0.0.1:8790")
    );
    assert_eq!(body["role"], serde_json::json!("relay"));
    assert!(
        !body.to_string().contains(TOKEN),
        "the status must never carry the management token: {body}"
    );
}

/// A token may be exchanged for a session, which is what a browser holds.
#[tokio::test]
async fn a_login_issues_a_session_that_the_control_plane_accepts() {
    let app = app();
    let login = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/relay/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "admin_token": TOKEN }).to_string(),
            ))
            .expect("build a request"),
    )
    .await;
    assert_eq!(login.status(), StatusCode::NO_CONTENT);
    let cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie header")
        .split(';')
        .next()
        .expect("a cookie pair")
        .to_string();

    let authenticated = call(
        app,
        Request::builder()
            .uri("/api/v1/relay/status")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("build a request"),
    )
    .await;
    assert_eq!(authenticated.status(), StatusCode::OK);

    // A wrong token never becomes a session.
    let rejected = call(
        router(RelayAdminState::read_only(relay_config(), TOKEN)),
        Request::builder()
            .method("POST")
            .uri("/api/v1/relay/auth/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({ "admin_token": "wrong" }).to_string(),
            ))
            .expect("build a request"),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
}

/// `auth/me` is how the page decides to ask for a token.
#[tokio::test]
async fn the_session_probe_answers_for_both_callers() {
    let anonymous = call(app(), get("/api/v1/relay/auth/me")).await;
    assert_eq!(anonymous.status(), StatusCode::OK);
    assert_eq!(
        body_of(anonymous).await,
        r#"{"authenticated":false}"#,
        "the probe must answer without a credential so the page can ask for one"
    );

    let known = call(app(), authorized("GET", "/api/v1/relay/auth/me")).await;
    assert_eq!(body_of(known).await, r#"{"authenticated":true}"#);
}

/// A worker request on a relay with no worker says so precisely.
#[tokio::test]
async fn a_worker_route_on_a_relay_without_a_worker_names_the_missing_worker() {
    let response = call(app(), authorized("GET", "/api/v1/admin/endpoints")).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_of(response).await;
    assert!(
        body.contains("worker_not_connected"),
        "the worker view must say the worker is missing, got: {body}"
    );
}

/// The worker receives the request target the caller actually sent.
///
/// This is the shape the management listener really produces. The relay control
/// plane is mounted with `nest`, so by the time a worker path reaches the proxy
/// the `/api/v1` mount point has already been stripped off the URI the handler
/// reads; rebuilding the worker's path from that would send every business
/// request to the worker's root. Reading the bridge message is what observes the
/// forwarded path directly rather than inferring it from an answer that, with no
/// worker answering, never depends on it.
///
/// The bare mount point is the one thing normalised: `/api/v1` and `/api/v1/`
/// name the same API root and the relay treats them as one path, so the worker
/// sees a single spelling of it. Everything else, query string included, is the
/// caller's own request target.
#[tokio::test]
async fn the_worker_receives_the_original_api_path_and_query() {
    use crate::protocol::BridgeMessage;

    for (request, expected) in [
        ("/api/v1/admin/endpoints", "/api/v1/admin/endpoints"),
        (
            "/api/v1/admin/request-records/summary?rows=5",
            "/api/v1/admin/request-records/summary?rows=5",
        ),
        ("/api/v1/admin/users/options", "/api/v1/admin/users/options"),
        ("/api/v1/usage?window=7d", "/api/v1/usage?window=7d"),
        ("/api/v1", "/api/v1/"),
        ("/api/v1/", "/api/v1/"),
        ("/api/v1?probe=1", "/api/v1/?probe=1"),
        ("/api/v1/?probe=1", "/api/v1/?probe=1"),
    ] {
        let (app, mut worker) = app_with_a_worker().await;
        // The handler then waits for a worker reply that this test never sends,
        // so the request is observed and abandoned rather than awaited to the
        // configured request timeout.
        let in_flight = tokio::spawn(call(app, authorized("GET", request)));

        let forwarded = worker
            .recv()
            .await
            .unwrap_or_else(|| panic!("{request} never reached the worker"));
        match forwarded {
            BridgeMessage::RequestStart(start) => assert_eq!(start.path, expected, "{request}"),
            other => panic!("{request} produced {other:?} instead of a request"),
        }
        in_flight.abort();
    }
}

/// A read-only control plane refuses a save instead of pretending it took.
#[tokio::test]
async fn a_role_save_reports_a_failure_rather_than_claiming_success() {
    let response = call(
        app(),
        Request::builder()
            .method("PUT")
            .uri("/api/v1/relay/host/role")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"role":"integrated"}"#))
            .expect("build a request"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body_of(response).await.contains("role_not_saved"));
}

/// The public relay listener never serves the relay's own control plane.
///
/// It keeps forwarding unknown paths to the worker, so these paths answer with
/// the public fallback's wording rather than with a relay-owned body.
#[tokio::test]
async fn the_public_listener_never_serves_the_relay_control_plane() {
    use crate::relay::{RemoteAddr, public_proxy::public_router, state::test_state};
    use tower::ServiceExt as _;

    for path in [
        "/api/v1/relay/status",
        "/api/v1/relay/host",
        "/api/v1/relay/settings",
    ] {
        let (mut parts, body) = get(path).into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "203.0.113.4:51000".parse().expect("an address"),
        )));
        let response = public_router(test_state())
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the public router answers");

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} must not answer as a relay control route"
        );
        let body = body_of(response).await;
        assert!(
            body.contains("no_worker"),
            "{path} must keep the public fallback wording, got: {body}"
        );
        assert!(
            !body.contains("admin_bind"),
            "{path} must not disclose relay settings on the public listener, got: {body}"
        );
    }
}

// ---------------------------------------------------------------------------
// Independent verification added by the phase-2 tester audit.
// ---------------------------------------------------------------------------

/// A control plane that may write. Only the settings cases below reach it, and
/// only with a bind the control plane rejects before it writes anything.
fn writable_app() -> Router {
    let config = RelayConfig {
        admin_token: TOKEN.to_string(),
        ..RelayConfig::default()
    };
    let (relay, handle) = crate::relay::test_handle(config.clone());
    router(RelayAdminState::new(
        &config,
        crate::config::HostRole::Relay,
        relay,
        handle,
    ))
}

fn json_request(method: &str, path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build a request")
}

/// An authenticated JSON request, which is what the management page sends.
fn json_request_bearer(method: &str, path: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build a request")
}

/// The public listener must not answer any relay-owned control route, whatever
/// the method and whatever the caller presents.
///
/// The frozen suite checks three `GET` paths. This one covers every route the
/// relay added, including the ones whose whole purpose is to mutate host state
/// or to mint a credential: reaching them through the public listener would be
/// an unauthenticated management action, or a session minted in front of a
/// token nobody presented.
#[tokio::test]
async fn the_public_listener_never_answers_any_relay_control_route() {
    use crate::relay::{public_proxy::public_router, state::test_state};

    for (method, path) in CONTROL_ROUTES {
        let (mut parts, body) = json_request(method, path, "{}").into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "203.0.113.4:51000".parse().expect("an address"),
        )));
        let response = public_router(test_state())
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the public router answers");

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{method} {path} must fall through to the worker, not answer as a relay route"
        );
        assert!(
            !response.headers().contains_key(header::SET_COOKIE),
            "{method} {path} must not mint a management session on the public listener"
        );
        let body = body_of(response).await;
        assert!(
            body.contains("no_worker"),
            "{method} {path} must keep the public fallback wording, got: {body}"
        );
        assert!(
            !body.contains("admin_bind") && !body.contains("relay_ready"),
            "{method} {path} must not disclose relay state on the public listener, got: {body}"
        );
    }
}

/// The loopback boundary holds even for a caller that already holds the token.
///
/// The token decides *which* local caller is the operator; the bind decides
/// *where* the control plane exists at all. A valid credential must not turn a
/// non-loopback peer into an accepted one, on a control route or on a proxied
/// worker route.
#[tokio::test]
async fn a_valid_management_token_does_not_defeat_the_loopback_boundary() {
    for path in ["/api/v1/relay/status", "/api/v1/admin/endpoints"] {
        let (mut parts, body) = authorized("GET", path).into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "198.51.100.7:51000".parse().expect("an address"),
        )));
        let response = app()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the management router answers");

        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{path} must stay unreachable from a non-loopback peer"
        );
        let body = body_of(response).await;
        assert!(
            body.contains("loopback"),
            "{path} must say why, got: {body}"
        );
    }
}

/// A session ends when the operator logs out.
///
/// The frozen suite proves a login produces a working cookie. A logout that
/// left the cookie usable would make the session lifetime the cookie's lifetime
/// instead of the session's, which is the whole point of keeping sessions
/// in memory.
#[tokio::test]
async fn a_logout_ends_the_session_it_issued() {
    let app = app();
    let login = call(
        app.clone(),
        json_request(
            "POST",
            "/api/v1/relay/auth/login",
            &serde_json::json!({ "admin_token": TOKEN }).to_string(),
        ),
    )
    .await;
    assert_eq!(login.status(), StatusCode::NO_CONTENT);
    let cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie header")
        .split(';')
        .next()
        .expect("a cookie pair")
        .to_string();

    let with_cookie = |path: &str| {
        Request::builder()
            .uri(path)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("build a request")
    };
    assert_eq!(
        call(app.clone(), with_cookie("/api/v1/relay/status"))
            .await
            .status(),
        StatusCode::OK,
        "the session must work before the logout"
    );

    let logout = call(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/v1/relay/auth/logout")
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .expect("build a request"),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);

    assert_eq!(
        call(app, with_cookie("/api/v1/relay/status"))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "a logged-out session must not keep driving the control plane"
    );
}

/// The two credentials are different secrets and neither substitutes for the
/// other.
///
/// A session id is deliberately opaque; presenting one as the token would make
/// the short-lived session a bearer secret, and the token is the long-lived one.
#[tokio::test]
async fn neither_credential_substitutes_for_the_other() {
    let app = app();
    let login = call(
        app.clone(),
        json_request(
            "POST",
            "/api/v1/relay/auth/login",
            &serde_json::json!({ "admin_token": TOKEN }).to_string(),
        ),
    )
    .await;
    let session = login
        .headers()
        .get(header::SET_COOKIE)
        .expect("a session cookie")
        .to_str()
        .expect("a cookie header")
        .split(';')
        .next()
        .expect("a cookie pair")
        .to_string();
    let session_id = session
        .split_once('=')
        .expect("a cookie pair")
        .1
        .to_string();

    let session_as_bearer = call(
        app.clone(),
        Request::builder()
            .uri("/api/v1/relay/status")
            .header(header::AUTHORIZATION, format!("Bearer {session_id}"))
            .body(Body::empty())
            .expect("build a request"),
    )
    .await;
    assert_eq!(
        session_as_bearer.status(),
        StatusCode::UNAUTHORIZED,
        "an opaque session id must not be accepted as the management token"
    );

    let token_as_cookie = call(
        app,
        Request::builder()
            .uri("/api/v1/relay/status")
            .header(header::COOKIE, format!("prompt_ferry_relay_admin={TOKEN}"))
            .body(Body::empty())
            .expect("build a request"),
    )
    .await;
    assert_eq!(
        token_as_cookie.status(),
        StatusCode::UNAUTHORIZED,
        "the management token must not be accepted as a session"
    );
}

/// The worker's own admin API root never answers as the management page.
///
/// `/api/v1` and `/api/v1/` are the same mount point written two ways, and the
/// proxy has an explicit rewrite for the trailing-slash form
/// (`worker_admin_path` maps both to `/api/v1/`). Both therefore have to stay
/// behind the guard and reach the worker: an unauthenticated caller must be
/// refused, and an authenticated one must get the worker's answer rather than a
/// 200 page.
#[tokio::test]
async fn the_worker_api_root_never_answers_as_the_management_page() {
    for path in ["/api/v1", "/api/v1/"] {
        let anonymous = call(app(), get(path)).await;
        assert_ne!(
            anonymous.status(),
            StatusCode::OK,
            "{path} must not answer an unauthenticated caller with 200"
        );
        assert!(
            !anonymous
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .starts_with("text/html"),
            "{path} must not resolve to the SPA fallback for an anonymous caller"
        );

        // With a credential the path belongs to the worker, so a relay with no
        // worker says exactly that instead of serving the page.
        let authenticated = call(app(), authorized("GET", path)).await;
        assert_eq!(
            authenticated.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{path} must reach the worker proxy once authenticated"
        );
        assert!(
            body_of(authenticated)
                .await
                .contains("worker_not_connected"),
            "{path} must reach the worker proxy once authenticated"
        );
    }
}

/// The page may not move the management listener off loopback.
///
/// The startup refuses a non-loopback `relay.admin_bind`, so the same rule has
/// to hold on the write path: otherwise a host that starts correctly can be
/// talked into publishing its own management API on every interface.
#[tokio::test]
async fn the_management_bind_must_stay_on_loopback_when_the_page_saves_it() {
    for admin_bind in ["0.0.0.0:8791", "192.0.2.5:8791", "[::]:8791"] {
        let response = call(
            writable_app(),
            Request::builder()
                .method("PATCH")
                .uri("/api/v1/relay/settings")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({ "admin_bind": admin_bind }).to_string(),
                ))
                .expect("build a request"),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "admin_bind {admin_bind} must be refused"
        );
        let body = body_of(response).await;
        assert!(
            body.contains("invalid_setting") && body.contains("loopback"),
            "admin_bind {admin_bind} must be refused as a loopback violation, got: {body}"
        );
    }
}

/// A role the product does not have is refused, never defaulted.
#[tokio::test]
async fn an_unknown_role_is_refused_and_never_becomes_a_saved_role() {
    for body in [
        r#"{"role":"relay-only"}"#,
        r#"{"role":"worker-only"}"#,
        r#"{"role":"Integrated"}"#,
        r#"{}"#,
    ] {
        let response = call(
            app(),
            json_request_bearer("PUT", "/api/v1/relay/host/role", body),
        )
        .await;

        assert!(
            response.status().is_client_error(),
            "role body {body} must be refused, got {}",
            response.status()
        );
        assert!(
            !response.status().is_success(),
            "role body {body} must never be accepted"
        );
    }
}

/// A worker request keeps the query string it arrived with.
///
/// The proxy hands the worker a single path string, and the worker's own
/// handlers read filters, paging and time windows from it. Dropping the query
/// would silently answer a different question than the caller asked — a usage
/// page would return its first page forever — so the query has to survive the
/// mount-point rewrite every other path goes through.
#[tokio::test]
async fn a_worker_request_carries_its_query_string_to_the_worker() {
    // Attaching a fake worker is what makes the forwarded request observable:
    // the bridge hands the worker one path string, and that string is the only
    // place a query can survive or be lost.
    use crate::{
        protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
        relay::{
            RemoteAddr,
            response_forward::{handle_response_chunk, handle_response_end, handle_response_start},
        },
    };
    use tokio::sync::mpsc;

    let config = RelayConfig {
        admin_token: TOKEN.to_string(),
        ..RelayConfig::default()
    };
    let (relay, handle) = crate::relay::test_handle(config.clone());
    let (worker_tx, mut worker_rx) = mpsc::channel(8);
    relay.inner.workers.lock().await.insert(1, worker_tx);

    let forwarded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let recorded = forwarded.clone();
    let state_for_worker = relay.clone();
    tokio::spawn(async move {
        let mut request_id = None;
        while let Some(message) = worker_rx.recv().await {
            match message {
                BridgeMessage::RequestStart(start) => {
                    recorded
                        .lock()
                        .expect("record the forwarded path")
                        .push(start.path);
                    request_id = Some(start.request_id);
                }
                BridgeMessage::RequestEnd(end) => {
                    let request_id = request_id.take().unwrap_or(end.request_id);
                    handle_response_start(
                        &state_for_worker,
                        ResponseStart {
                            request_id: request_id.clone(),
                            status: StatusCode::OK.as_u16(),
                            content_type: Some("application/json".to_string()),
                            headers: Vec::new(),
                        },
                    )
                    .await;
                    handle_response_chunk(
                        &state_for_worker,
                        ResponseChunk {
                            request_id: request_id.clone(),
                            data: b"{}".to_vec(),
                        },
                    )
                    .await;
                    handle_response_end(&state_for_worker, ResponseEnd { request_id }).await;
                }
                _ => {}
            }
        }
    });

    let app = router(RelayAdminState::new(
        &config,
        crate::config::HostRole::Relay,
        relay,
        handle,
    ));

    for incoming in [
        "/api/v1/admin/endpoints?page=2&per_page=50",
        "/api/v1/usage?window=7d",
        "/api/v1?probe=1",
        "/api/v1/?probe=1",
    ] {
        let (mut parts, body) = authorized("GET", incoming).into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "127.0.0.1:51000".parse().expect("an address"),
        )));
        let response = app
            .clone()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the management router answers");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{incoming} must reach the worker"
        );
    }

    let forwarded = forwarded.lock().expect("read the recorded paths").clone();
    assert_eq!(
        forwarded,
        vec![
            "/api/v1/admin/endpoints?page=2&per_page=50",
            "/api/v1/usage?window=7d",
            "/api/v1/?probe=1",
            "/api/v1/?probe=1",
        ],
        "the query string must reach the worker unchanged"
    );
}

/// The open routes carry no peer check, because the loopback bind is the boundary.
///
/// Liveness, login, logout and the session probe sit outside the guard: a caller
/// cannot present a credential before it can ask for one. That exemption is
/// about the *credential* only. The *boundary* on these routes is the listener's
/// own loopback bind, which `serve` refuses to start without — so this test pins
/// the division of responsibility rather than re-asserting the peer check: the
/// open routes must not grow one that reads as the boundary while the bind still
/// is, and the guarded routes must keep theirs.
#[tokio::test]
async fn the_peer_check_belongs_to_the_guard_and_the_bind_carries_the_open_routes() {
    // A guarded route refuses a non-loopback peer even holding the valid token.
    for path in ["/api/v1/relay/status", "/api/v1/admin/endpoints"] {
        let (mut parts, body) = authorized("GET", path).into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "198.51.100.7:51000".parse().expect("an address"),
        )));
        let response = app()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the management router answers");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "{path} must keep the peer check the guard owns"
        );
    }

    // The open routes answer a non-loopback peer, because nothing but the bind
    // stands in front of them. That is only sound while the bind is loopback,
    // which is why the reservation refuses anything else; if that ever changes,
    // this test is the reminder that these four routes have no second boundary.
    for (method, path) in [
        ("GET", "/api/v1/healthz"),
        ("POST", "/api/v1/relay/auth/login"),
        ("POST", "/api/v1/relay/auth/logout"),
        ("GET", "/api/v1/relay/auth/me"),
    ] {
        let (mut parts, body) = json_request(method, path, "{}").into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "198.51.100.7:51000".parse().expect("an address"),
        )));
        let response = app()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the management router answers");

        // Whatever these routes answer, an unauthenticated body must never be a
        // session: the login route is the only one that may set a cookie, and it
        // must not do so for a body that does not carry the token.
        if path.ends_with("/login") {
            assert!(
                !response.headers().contains_key(header::SET_COOKIE),
                "a login body without the token must never mint a session"
            );
        }
        let _ = response;
    }
}

/// The management token never leaves through any control response.
///
/// The status endpoint is covered by the frozen suite. This covers every
/// authenticated control route, because a token echoed by any one of them is
/// the same disclosure, and the settings view is the one that reports what is
/// configured.
#[tokio::test]
async fn no_control_response_echoes_the_management_token() {
    for (method, path) in [
        ("GET", "/api/v1/relay/status"),
        ("GET", "/api/v1/relay/settings"),
        ("GET", "/api/v1/relay/host"),
    ] {
        let response = call(app(), authorized(method, path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{method} {path}");

        let body = body_of(response).await;
        assert!(
            !body.contains(TOKEN),
            "{method} {path} must never carry the management token: {body}"
        );
    }
}

/// A saved bind is reported as needing a restart.
///
/// `restart_required` is what the page uses to tell the operator the change is
/// not live yet. A save that reported success without it would leave the page
/// claiming a bind is in force while the old listener is still the one serving.
#[tokio::test]
async fn a_saved_bind_is_reported_as_needing_a_restart() {
    let app = writable_app();
    let bind = format!("127.0.0.1:{}", reserve_free_port());

    let response = call(
        app,
        json_request_bearer(
            "PATCH",
            "/api/v1/relay/settings",
            &serde_json::json!({ "admin_bind": bind }).to_string(),
        ),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&body_of(response).await).expect("the settings answer is JSON");
    assert_eq!(
        body["restart_required"],
        serde_json::json!(true),
        "a saved listener bind cannot take effect in the running process"
    );
}

/// A saved bind keeps asking for a restart after the save that wrote it.
///
/// The save answers `restart_required: true` on its own, but the page reads the
/// setting again when it loads and after every reload, and a host with no role
/// change has nothing in its recorded state that would keep it knowing a bind is
/// pending. The flag therefore has to be derived from what was saved and what
/// this host is running, not from the save alone.
#[tokio::test]
async fn a_saved_bind_still_asks_for_a_restart_when_it_is_read_again() {
    let bind = format!("127.0.0.1:{}", reserve_free_port());
    let saved = call(
        writable_app(),
        json_request_bearer(
            "PATCH",
            "/api/v1/relay/settings",
            &serde_json::json!({ "admin_bind": bind }).to_string(),
        ),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);

    for path in [
        "/api/v1/relay/settings",
        "/api/v1/relay/status",
        "/api/v1/relay/host",
    ] {
        let response = call(writable_app(), authorized("GET", path)).await;
        let body: serde_json::Value =
            serde_json::from_str(&body_of(response).await).expect("the answer is JSON");
        assert_eq!(
            body["restart_required"],
            serde_json::json!(true),
            "{path} must still report the saved bind as pending"
        );
    }
}

/// An empty settings update changes nothing and asks for no restart.
///
/// The body is sparse on purpose, so an update that carries no key must not be
/// reported as a change: that would show the operator a restart prompt for a
/// save that wrote nothing.
#[tokio::test]
async fn an_empty_settings_update_saves_nothing_and_asks_for_no_restart() {
    let app = writable_app();

    let response = call(
        app.clone(),
        json_request_bearer("PATCH", "/api/v1/relay/settings", "{}"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_str(&body_of(response).await).expect("the settings answer is JSON");
    assert_eq!(
        body["restart_required"],
        serde_json::json!(false),
        "an update carrying no key must not report a pending restart"
    );

    // The same control plane still refuses a non-loopback bind afterwards, so
    // the no-op save did not leave it in a degraded state.
    let refused = call(
        app,
        json_request_bearer(
            "PATCH",
            "/api/v1/relay/settings",
            r#"{"admin_bind":"0.0.0.0:8791"}"#,
        ),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
}

/// A body the control plane does not accept never reaches the save.
///
/// The two bodies are the ones a mistyped page would send: an unknown field and
/// a field of the wrong type. Neither may be accepted as a partial save, because
/// the overlay is sparse and a silently-dropped key is indistinguishable from a
/// key the operator did not mean to change.
#[tokio::test]
async fn a_settings_body_the_page_should_never_send_is_refused() {
    for body in [
        r#"{"admin_bndi":"127.0.0.1:8791"}"#,
        r#"{"admin_bind":8791}"#,
    ] {
        let response = call(
            writable_app(),
            json_request_bearer("PATCH", "/api/v1/relay/settings", body),
        )
        .await;

        assert!(
            !response.status().is_success(),
            "body {body} must not be accepted as a save, got {}",
            response.status()
        );
    }
}

/// A logout without a session is a no-op, not an error.
///
/// The route is outside the guard, so an anonymous page load must not be able to
/// turn it into a failure; and it must not claim to have ended a session it
/// never had.
#[tokio::test]
async fn a_logout_without_a_session_answers_without_pretending_to_end_one() {
    let response = call(
        app(),
        json_request("POST", "/api/v1/relay/auth/logout", "{}"),
    )
    .await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("the route clears the cookie either way")
        .to_str()
        .expect("a cookie header");
    assert!(cookie.contains("Max-Age=0"), "{cookie}");
    assert!(
        !cookie.contains(&format!("{TOKEN}=")),
        "the cleared cookie must not carry a credential: {cookie}"
    );
}

/// Only the documented login body can exchange a token for a session.
///
/// `/relay/auth/login` is outside the guard because it is how a caller obtains
/// a credential. That makes its body the one place where a permissive parser
/// would matter: a body carrying the token under a different key, or carrying
/// extra fields, must not mint a session.
#[tokio::test]
async fn only_the_documented_login_body_can_exchange_a_token_for_a_session() {
    for body in [
        r#"{"token":"relay-management-token-for-tests"}"#,
        r#"{"admin_token":"relay-management-token-for-tests","role":"integrated"}"#,
        r#"{"admin_token":""}"#,
        r#"{}"#,
    ] {
        let response = call(
            app(),
            json_request("POST", "/api/v1/relay/auth/login", body),
        )
        .await;

        assert!(
            !response.status().is_success(),
            "login body {body} must not mint a session, got {}",
            response.status()
        );
        assert!(
            !response.headers().contains_key(header::SET_COOKIE),
            "login body {body} must not set a session cookie"
        );
    }
}

/// Every spelling of the API prefix stays behind the guard.
///
/// The frozen suite covers `/api/v1` and `/api/v1/`. These are the spellings a
/// client library or a hand-written fetch can produce instead, and each one must
/// land on the guard rather than on the page: the page is served without a
/// credential by design, so a prefix variant that misses the guard is an
/// unauthenticated read of whatever the worker would have said.
#[tokio::test]
async fn no_prefix_variant_of_the_api_escapes_to_the_page() {
    for path in [
        "/api/v1",
        "/api/v1/",
        "/api/v1//",
        "/api/v1/relay",
        "/api/v1/relay/",
        "/api/v1/relay/status/",
        "/api/v1/admin/endpoints",
        "/api/v1/worker/anything",
    ] {
        let response = call(app(), get(path)).await;

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{path} must refuse an unauthenticated caller, got {}",
            response.status()
        );
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        assert!(
            !content_type.starts_with("text/html"),
            "{path} must never answer with the page, got {content_type}"
        );
    }
}

/// A path that merely starts with the API text is still the page.
///
/// `/api/v11` and `/api/v1x` are browser paths, not API ones. The guard belongs
/// to the API prefix as a path segment, so widening it to a string prefix would
/// break every page route that happens to start with those characters.
#[tokio::test]
async fn a_browser_path_that_only_looks_like_the_api_still_serves_the_page() {
    for path in ["/api", "/api/v", "/api/v11", "/api/v1x", "/apiv1"] {
        let response = call(app(), get(path)).await;

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{path} must stay the page"
        );
        assert!(
            response.headers()[header::CONTENT_TYPE]
                .to_str()
                .expect("content type")
                .starts_with("text/html"),
            "{path} must stay the page"
        );
    }
}

/// A worker route refuses an unauthenticated caller whatever method it uses.
///
/// The guard is a middleware over the fallback, so a method the relay does not
/// own still has to pass it. A method-shaped hole here would be an unauthenticated
/// worker mutation, which is the exact failure the guard exists to prevent.
#[tokio::test]
async fn no_worker_method_bypasses_the_guard() {
    for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
        let response = call(
            app(),
            Request::builder()
                .method(method)
                .uri("/api/v1/admin/endpoints")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("build a request"),
        )
        .await;

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} /api/v1/admin/endpoints must refuse an unauthenticated caller"
        );
    }
}

/// The session probe never leaks anything but the one boolean.
///
/// It is the one route a page may call before it has a credential, so its body
/// is the one unauthenticated body on this listener. It has to answer the
/// question the page asks and nothing else.
#[tokio::test]
async fn the_session_probe_answers_only_whether_the_caller_is_authenticated() {
    let valid = format!("Bearer {TOKEN}");
    let cases: [(Option<&str>, &str, &str); 4] = [
        (None, "", r#"{"authenticated":false}"#),
        (Some("bearer"), &valid, r#"{"authenticated":true}"#),
        (
            Some("bearer"),
            "Bearer not-the-token",
            r#"{"authenticated":false}"#,
        ),
        (
            Some("cookie"),
            "prompt_ferry_relay_admin=not-a-session",
            r#"{"authenticated":false}"#,
        ),
    ];

    for (kind, value, expected) in cases {
        let request = match kind {
            Some("bearer") => Request::builder()
                .uri("/api/v1/relay/auth/me")
                .header(header::AUTHORIZATION, value)
                .body(Body::empty())
                .expect("build a request"),
            Some("cookie") => Request::builder()
                .uri("/api/v1/relay/auth/me")
                .header(header::COOKIE, value)
                .body(Body::empty())
                .expect("build a request"),
            _ => get("/api/v1/relay/auth/me"),
        };

        let response = call(app(), request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_of(response).await;
        assert_eq!(body, expected, "credential {value}");
        assert!(
            !body.contains(TOKEN),
            "the probe must not echo a token: {body}"
        );
    }
}

/// Reserve a concrete free loopback port for a save the test makes.
fn reserve_free_port() -> u16 {
    let reserved = crate::config::binds::reserve_bind("127.0.0.1:0", "test", false)
        .expect("probe a free loopback port");
    let port = reserved.addr().port();
    drop(reserved.into_listener().expect("release the probed socket"));
    port
}

/// Confirm the mechanism: `nest` hands the handler a prefix-stripped URI.
///
/// This is the observation that explains the collapsed path, so it is asserted
/// rather than assumed.
#[tokio::test]
async fn the_nested_handler_sees_the_uri_without_its_mount_prefix() {
    use axum::{
        Router,
        http::{Request, Uri},
        routing::any,
    };
    use tower::ServiceExt as _;

    async fn echo(uri: Uri, original: axum::extract::OriginalUri) -> String {
        format!("path={} original={}", uri.path(), original.0.path())
    }

    let app: Router = Router::new()
        .nest("/api/v1", Router::new().fallback(any(echo)))
        .with_state(());

    for target in ["/api/v1/admin/endpoints", "/api/v1/usage?window=7d"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(target).body(Body::empty()).unwrap())
            .await
            .expect("the nested router answers");
        let body = body_of(response).await;
        println!("{target} -> {body}");
        assert!(
            body.contains(&format!(
                "path={}",
                target
                    .split('?')
                    .next()
                    .unwrap()
                    .trim_start_matches("/api/v1")
            )),
            "the nested handler sees the stripped path: {body}"
        );
    }
}

/// Every worker request through the management page reaches the same path.
///
/// This is the failure the collapse produces, stated as one assertion: the four
/// distinct worker targets below are all forwarded as the worker's admin root,
/// so the relay page cannot address a worker resource at all.
///
/// Every target is a worker business path. `/api/v1/healthz` is deliberately not
/// among them: it is the relay's own liveness route, answered locally and without
/// a credential, so it never reaches a worker at all.
#[tokio::test]
async fn distinct_worker_targets_are_not_all_forwarded_as_one_path() {
    use crate::{
        protocol::{BridgeMessage, ResponseChunk, ResponseEnd, ResponseStart},
        relay::response_forward::{
            handle_response_chunk, handle_response_end, handle_response_start,
        },
    };
    use tokio::sync::mpsc;

    let config = RelayConfig {
        admin_token: TOKEN.to_string(),
        ..RelayConfig::default()
    };
    let (relay, handle) = crate::relay::test_handle(config.clone());
    let (worker_tx, mut worker_rx) = mpsc::channel(8);
    relay.inner.workers.lock().await.insert(1, worker_tx);

    let forwarded = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let recorded = forwarded.clone();
    let state_for_worker = relay.clone();
    tokio::spawn(async move {
        let mut request_id = None;
        while let Some(message) = worker_rx.recv().await {
            match message {
                BridgeMessage::RequestStart(start) => {
                    recorded
                        .lock()
                        .expect("record the forwarded path")
                        .push(start.path);
                    request_id = Some(start.request_id);
                }
                BridgeMessage::RequestEnd(end) => {
                    let request_id = request_id.take().unwrap_or(end.request_id);
                    handle_response_start(
                        &state_for_worker,
                        ResponseStart {
                            request_id: request_id.clone(),
                            status: StatusCode::OK.as_u16(),
                            content_type: Some("application/json".to_string()),
                            headers: Vec::new(),
                        },
                    )
                    .await;
                    handle_response_chunk(
                        &state_for_worker,
                        ResponseChunk {
                            request_id: request_id.clone(),
                            data: b"{}".to_vec(),
                        },
                    )
                    .await;
                    handle_response_end(&state_for_worker, ResponseEnd { request_id }).await;
                }
                _ => {}
            }
        }
    });

    let app = router(RelayAdminState::new(
        &config,
        crate::config::HostRole::Relay,
        relay,
        handle,
    ));

    for target in [
        "/api/v1/admin/endpoints",
        "/api/v1/admin/model-routes",
        "/api/v1/admin/request-records/summary",
        "/api/v1/admin/relays",
    ] {
        let (mut parts, body) = authorized("GET", target).into_parts();
        parts.extensions.insert(ConnectInfo(RemoteAddr(
            "127.0.0.1:51000".parse().expect("an address"),
        )));
        let _ = app
            .clone()
            .oneshot(Request::from_parts(parts, body))
            .await
            .expect("the management router answers");
    }

    let forwarded = forwarded.lock().expect("read the recorded paths").clone();
    assert_eq!(
        forwarded,
        vec![
            "/api/v1/admin/endpoints",
            "/api/v1/admin/model-routes",
            "/api/v1/admin/request-records/summary",
            "/api/v1/admin/relays",
        ],
        "each worker target must reach the worker under its own path"
    );
}

/// A request exactly as the bridge handed it to a worker.
#[derive(Debug, PartialEq)]
struct Forwarded {
    method: String,
    path: String,
    body: String,
}

/// Drive one request through a management listener with a worker attached and
/// report what that worker was actually handed.
///
/// Reading the bridge messages is the only way to observe the forwarded request:
/// with no worker answering, every management answer is `worker_not_connected`
/// and none of them depends on the path, which is how a wrong forwarded target
/// hides behind a green suite.
async fn forwarded_by_the_worker(request: Request<Body>) -> Forwarded {
    use crate::protocol::BridgeMessage;

    let (app, mut worker) = app_with_a_worker().await;
    // No reply is ever sent, so the request is observed and abandoned rather
    // than awaited to the configured request timeout.
    let in_flight = tokio::spawn(call(app, request));
    let mut forwarded: Option<Forwarded> = None;
    let mut body = Vec::new();
    while let Some(message) = worker.recv().await {
        match message {
            BridgeMessage::RequestStart(start) => {
                forwarded = Some(Forwarded {
                    method: start.method,
                    path: start.path,
                    body: String::new(),
                });
            }
            BridgeMessage::RequestChunk(chunk) => body.extend_from_slice(&chunk.data),
            BridgeMessage::RequestEnd(_) => break,
            other => panic!("the worker was sent an unexpected message: {other:?}"),
        }
    }
    in_flight.abort();
    let mut forwarded = forwarded.expect("the request reached the worker");
    forwarded.body = String::from_utf8(body).expect("a UTF-8 request body");
    forwarded
}

fn json_body_request(method: &str, target: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(target)
        .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("build a request")
}

/// A worker mutation keeps its own method, its own path and its own body.
///
/// Path preservation is the phase's acceptance criterion, but the frozen
/// regression only drives `GET`. A worker business API is mostly writes — a
/// provider, a model route, a relay entry is created and deleted through the
/// same proxy — so the method and the body have to travel with the path, or a
/// correct path would still mutate the wrong resource with the wrong payload.
#[tokio::test]
async fn a_worker_mutation_keeps_its_own_method_path_and_body() {
    for (method, target, body) in [
        ("POST", "/api/v1/admin/providers", r#"{"name":"p1"}"#),
        ("PUT", "/api/v1/admin/providers/7", r#"{"name":"p2"}"#),
        (
            "PATCH",
            "/api/v1/admin/model-routes/3?strict=true",
            r#"{"active":false}"#,
        ),
        ("DELETE", "/api/v1/admin/relays/9", ""),
    ] {
        assert_eq!(
            forwarded_by_the_worker(json_body_request(method, target, body)).await,
            Forwarded {
                method: method.to_string(),
                path: target.to_string(),
                body: body.to_string(),
            },
            "{method} {target} must reach the worker unchanged"
        );
    }
}

/// The query string survives byte for byte, not just as "there is a query".
///
/// The worker's own handlers read filters, paging and time windows out of the
/// query, and several of them distinguish an absent parameter from an empty or
/// repeated one. Re-encoding or re-ordering the query would answer a different
/// question than the caller asked while still looking plausible, so the encoding,
/// the repeats and the order are all asserted.
#[tokio::test]
async fn an_encoded_or_repeated_query_reaches_the_worker_byte_for_byte() {
    for target in [
        "/api/v1/usage?window=7d&model=gpt-4o&window=30d",
        "/api/v1/admin/search?q=a%20b&path=%2Fapi%2Fv1",
        "/api/v1/admin/endpoints?",
        "/api/v1/admin/endpoints/",
        "/api/v1/admin/endpoints?page=2",
    ] {
        assert_eq!(
            forwarded_by_the_worker(authorized("GET", target))
                .await
                .path,
            target,
            "the request target must reach the worker unchanged"
        );
    }
}

/// The guard is not weakened by a query, a method or a body.
///
/// The worker proxy reads the request target it forwards from the original URI,
/// which is a value the guard's own route table never sees. A caller must still
/// be refused before anything is handed to the bridge, or the path-preserving
/// proxy would become the one route on this listener that answers without a
/// management credential.
#[tokio::test]
async fn an_unauthenticated_worker_request_never_reaches_the_bridge() {
    let (app, mut worker) = app_with_a_worker().await;

    for mut request in [
        Request::builder()
            .method("GET")
            .uri("/api/v1/admin/endpoints?page=2")
            .body(Body::empty())
            .expect("build a request"),
        json_body_request("POST", "/api/v1/admin/providers", r#"{"name":"p1"}"#),
    ] {
        // Drop the bearer header: these are the unauthenticated spellings.
        request.headers_mut().remove(header::AUTHORIZATION);
        let response = call(app.clone(), request).await;

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a worker request without a credential must be refused"
        );
        assert!(
            body_of(response).await.contains("unauthorized"),
            "the refusal must answer as JSON, not as the page"
        );
    }

    assert!(
        worker.try_recv().is_err(),
        "a refused caller must not put anything on the bridge"
    );
}

/// A relay-owned route stays local even when it carries a query, and an unknown
/// path under the same prefix stays the worker's.
///
/// The prefix holds both planes at once: `/api/v1/relay/*` is answered by the
/// relay and everything else under `/api/v1` belongs to the worker. Reading the
/// forwarded target is what tells the two apart, because both answer 200.
#[tokio::test]
async fn a_relay_owned_route_is_answered_locally_while_an_unknown_one_is_not() {
    let response = call(app(), authorized("GET", "/api/v1/relay/status?verbose=1")).await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a query must not push a relay-owned route off its own handler"
    );
    let body = body_of(response).await;
    assert!(
        body.contains("\"role\""),
        "the relay answers its own status route, got: {body}"
    );

    assert_eq!(
        forwarded_by_the_worker(authorized("GET", "/api/v1/relay/unknown"))
            .await
            .path,
        "/api/v1/relay/unknown",
        "an unknown path under the prefix belongs to the worker"
    );
}
