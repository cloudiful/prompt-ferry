//! Authorization surface of the administrator configuration export.

use super::*;
use axum::body::to_bytes;
use tower::ServiceExt;

use crate::worker_admin;

#[tokio::test]
async fn export_requires_an_admin_session() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "export-non-admin", false).await;
    let app = worker_admin::router(state.clone());

    let anonymous = app
        .clone()
        .oneshot(export_request(None, PASSPHRASE))
        .await
        .expect("anonymous request");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let forbidden = app
        .oneshot(export_request(Some("export-non-admin"), PASSPHRASE))
        .await
        .expect("non-admin request");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    close_state(store, path).await;
}

#[tokio::test]
async fn export_rejects_a_short_passphrase_without_leaking_it() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "export-admin", true).await;
    let app = worker_admin::router(state.clone());

    let response = app
        .oneshot(export_request(Some("export-admin"), "short"))
        .await
        .expect("export request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    let text = String::from_utf8_lossy(&body).to_string();
    assert!(text.contains("archive_passphrase_too_short"));
    assert!(
        !text.contains("\"short\""),
        "the passphrase value is never echoed: {text}"
    );

    close_state(store, path).await;
}
