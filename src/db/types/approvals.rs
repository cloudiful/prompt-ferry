// Issue #384 Phase 5: approval DTOs moved to the `prompt-ferry-llm-review`
// leaf crate; `db` re-exports them so `db::ApprovalRequest` paths are unchanged.
pub use prompt_ferry_llm_review::{
    ApprovalRequest, ApprovalRequestCreate, ApprovalRequestPage, ApprovalStatusFilter,
    FlaggedApprovalRequestInput,
};
