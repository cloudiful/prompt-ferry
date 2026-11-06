use axum::http::{StatusCode, header};

use super::{OAUTH_CALLBACK_HTML, oauth_callback_response, oauth_error_response};
use crate::mcp::oauth::McpOAuthError;

#[tokio::test]
async fn callback_page_is_static_and_never_cached_or_reflected() {
    let response = oauth_callback_response();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers.get(header::CACHE_CONTROL).unwrap(),
        "no-store",
        "an authorization result page must never be cached"
    );
    assert_eq!(
        headers.get(header::REFERRER_POLICY).unwrap(),
        "no-referrer",
        "the callback URL (code/state) must not leak through Referer"
    );
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let body = String::from_utf8(body.to_vec()).expect("utf8");
    assert_eq!(body, OAUTH_CALLBACK_HTML);
    assert!(!body.contains("code"));
    assert!(!body.contains("state"));
}

#[test]
fn oauth_errors_map_to_stable_status_codes_and_safe_messages() {
    let cases = [
        (McpOAuthError::Forbidden, StatusCode::FORBIDDEN),
        (McpOAuthError::NotFound, StatusCode::NOT_FOUND),
        (McpOAuthError::InvalidState, StatusCode::BAD_REQUEST),
        (McpOAuthError::AuthorizationRequired, StatusCode::CONFLICT),
        (McpOAuthError::RefreshInProgress, StatusCode::CONFLICT),
        (
            McpOAuthError::Disabled("redirect base missing".into()),
            StatusCode::CONFLICT,
        ),
        (McpOAuthError::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
        (
            McpOAuthError::Upstream("token exchange failed".into()),
            StatusCode::BAD_GATEWAY,
        ),
    ];
    for (error, expected) in cases {
        let code = error.code();
        let message = error.message();
        assert!(
            !message.contains("access_token") && !message.contains("client_secret"),
            "the API message must never carry a secret"
        );
        let response = oauth_error_response(error);
        assert_eq!(response.status(), expected, "{code}");
    }
}
