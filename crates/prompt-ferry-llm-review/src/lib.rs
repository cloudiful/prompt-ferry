// Issue #384 Phase 5: LLM review gate moved from `prompt_ferry::llm_review`.
// Leaf crate with no intra-workspace dependencies. Approval DTOs plus the
// webhook result recorder moved from `prompt_ferry::db` into `approvals` so
// the former `db <-> llm_review` cycle resolves to a single `db -> leaf`
// edge; `truncate_chars` is a small in-crate copy to avoid a `usage` edge.
// The root crate re-exports this crate so `prompt_ferry::llm_review::*`
// paths are unchanged.
mod approvals;
mod parser;
mod reviewer;
mod types;
mod webhook;

pub use approvals::{
    ApprovalRequest, ApprovalRequestCreate, ApprovalRequestPage, ApprovalStatusFilter,
    FlaggedApprovalRequestInput, record_approval_webhook_result,
};
pub use parser::parse_review_completion_body;
pub use reviewer::{
    LLM_REVIEW_SETTINGS_KEY, ReviewRequest, compute_wait_deadline_unix_ms, request_payload_json,
    review_request,
};
pub use types::{
    ApprovalResolution, ApprovalStatus, ApprovalWaiter, LlmReviewSettings,
    LlmReviewWebhookSettings, ReviewDecision, ReviewFailure, ReviewFailurePolicy, ReviewResult,
};
pub use webhook::{approval_webhook_enabled, spawn_approval_webhook};
