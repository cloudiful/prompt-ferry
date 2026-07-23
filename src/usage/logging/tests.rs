use crate::db::{self, RequestFailureFamily};

use super::{UsageLog, UsageRequestMetadata, inference::infer_failure_family};

#[test]
fn ai_request_constructor_sets_ai_defaults() {
    let log = UsageLog::ai_request(
        uuid::Uuid::new_v4(),
        UsageRequestMetadata {
            path: "/v1/responses".to_string(),
            ..UsageRequestMetadata::default()
        },
        Some("gpt-5".to_string()),
    );

    assert_eq!(log.event_kind, db::UsageEventKind::Request);
    assert_eq!(log.request_category, db::RequestRecordCategory::Ai);
    assert_eq!(log.request_state, db::RequestRecordState::Received);
    assert_eq!(log.path, "/v1/responses");
    assert_eq!(log.model.as_deref(), Some("gpt-5"));
    assert_eq!(log.conversation_source, "none");
}

#[test]
fn mcp_request_constructor_sets_mcp_defaults() {
    let log = UsageLog::mcp_request(
        uuid::Uuid::new_v4(),
        UsageRequestMetadata {
            path: "/mcp".to_string(),
            ..UsageRequestMetadata::default()
        },
        Some("catalog".to_string()),
        Some("tools/list".to_string()),
        Some("list_tools".to_string()),
    );

    assert_eq!(log.event_kind, db::UsageEventKind::Request);
    assert_eq!(log.request_category, db::RequestRecordCategory::Mcp);
    assert_eq!(log.request_state, db::RequestRecordState::Received);
    assert_eq!(log.path, "/mcp");
    assert_eq!(log.mcp_server_name.as_deref(), Some("catalog"));
    assert_eq!(log.mcp_protocol_method.as_deref(), Some("tools/list"));
    assert_eq!(log.mcp_operation_name.as_deref(), Some("list_tools"));
}

#[test]
fn infers_failure_family_for_auth_rate_limit_and_empty_success() {
    let auth = UsageLog::ai_request(
        uuid::Uuid::new_v4(),
        UsageRequestMetadata::default(),
        Some("gpt-5".to_string()),
    )
    .with_status(Some(401), Some(false), None, None)
    .with_error(None, Some("unauthorized".to_string()), None);
    let rate_limit = UsageLog::ai_request(
        uuid::Uuid::new_v4(),
        UsageRequestMetadata::default(),
        Some("gpt-5".to_string()),
    )
    .with_status(Some(429), Some(false), None, None)
    .with_error(None, Some("too many requests".to_string()), None);
    let empty_success = UsageLog::ai_request(
        uuid::Uuid::new_v4(),
        UsageRequestMetadata::default(),
        Some("gpt-5".to_string()),
    )
    .with_status(Some(200), Some(true), Some(100), Some(20));

    assert_eq!(
        infer_failure_family(&auth),
        Some(RequestFailureFamily::Auth)
    );
    assert_eq!(
        infer_failure_family(&rate_limit),
        Some(RequestFailureFamily::RateLimit)
    );
    assert_eq!(
        infer_failure_family(&empty_success),
        Some(RequestFailureFamily::EmptySuccess)
    );
}
