//! Page window of the configuration archive audit list.

use super::*;
use tower::ServiceExt;

use crate::db::config_repository::audit::MAX_AUDIT_PAGE_ROWS;
use crate::worker_admin;

#[tokio::test]
async fn audit_list_pages_newest_first_and_clamps_the_window() {
    let (state, store, path) = test_state().await;
    attach_session(&state, "audit-admin", true).await;
    let app = worker_admin::router(state.clone());

    for _ in 0..3 {
        let exported = app
            .clone()
            .oneshot(export_request(Some("audit-admin"), PASSPHRASE))
            .await
            .expect("export request");
        assert_eq!(exported.status(), StatusCode::OK);
    }

    let first_page = app
        .clone()
        .oneshot(audit_request(Some("audit-admin"), "?first=0&rows=2"))
        .await
        .expect("first page");
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_page = json_body(first_page).await;
    assert_eq!(first_page["total"], 3);
    assert_eq!(first_page["first"], 0);
    assert_eq!(first_page["rows"], 2);
    let entries = first_page["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    // Newest first: the audit ids descend.
    let ids: Vec<i64> = entries
        .iter()
        .map(|entry| entry["audit_id"].as_i64().expect("audit id"))
        .collect();
    assert!(
        ids[0] > ids[1],
        "expected descending audit ids, got {ids:?}"
    );

    let second_page = app
        .clone()
        .oneshot(audit_request(Some("audit-admin"), "?first=2&rows=2"))
        .await
        .expect("second page");
    let second_page = json_body(second_page).await;
    assert_eq!(second_page["total"], 3);
    assert_eq!(second_page["first"], 2);
    assert_eq!(second_page["entries"].as_array().expect("entries").len(), 1);

    let clamped = app
        .clone()
        .oneshot(audit_request(Some("audit-admin"), "?first=-4&rows=100000"))
        .await
        .expect("clamped page");
    let clamped = json_body(clamped).await;
    assert_eq!(clamped["first"], 0);
    assert_eq!(clamped["rows"], MAX_AUDIT_PAGE_ROWS);
    assert_eq!(clamped["entries"].as_array().expect("entries").len(), 3);

    close_state(store, path).await;
}
