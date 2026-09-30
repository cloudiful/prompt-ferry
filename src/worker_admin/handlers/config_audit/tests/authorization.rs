//! Authorization surface of the configuration archive audit list.

use super::*;
use tower::ServiceExt;

use crate::worker_admin;

#[tokio::test]
async fn audit_list_requires_an_admin_session() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "audit-non-admin", false).await;
    let app = worker_admin::router(state.clone());

    let anonymous = app
        .clone()
        .oneshot(audit_request(None, ""))
        .await
        .expect("anonymous request");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let forbidden = app
        .clone()
        .oneshot(audit_request(Some("audit-non-admin"), ""))
        .await
        .expect("non-admin request");
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    attach_session(&state, "audit-admin", true).await;
    let allowed = app
        .clone()
        .oneshot(audit_request(Some("audit-admin"), ""))
        .await
        .expect("admin request");
    assert_eq!(allowed.status(), StatusCode::OK);
    let body = json_body(allowed).await;
    assert_eq!(body["total"], 0);
    assert_eq!(body["entries"].as_array().expect("entries").len(), 0);

    close_state(store, path).await;
}
