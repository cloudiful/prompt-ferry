//! Authorization surface of the administrator configuration import.

use super::*;
use tower::ServiceExt;

use crate::worker_admin;

#[tokio::test]
async fn import_endpoints_require_an_admin_session() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "import-non-admin", false).await;
    let archive = export_archive(&state).await;
    let app = worker_admin::router(state.clone());

    for uri in [
        "/api/v1/admin/config-import/preview",
        "/api/v1/admin/config-import",
    ] {
        let anonymous = app
            .clone()
            .oneshot(import_request(uri, None, PASSPHRASE, &archive))
            .await
            .expect("anonymous request");
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED, "{uri}");

        let forbidden = app
            .clone()
            .oneshot(import_request(
                uri,
                Some("import-non-admin"),
                PASSPHRASE,
                &archive,
            ))
            .await
            .expect("non-admin request");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "{uri}");
    }

    close_state(store, path).await;
}
