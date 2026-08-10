use prompt_ferry::worker_admin_types::RequestContentLoggingMode;

use crate::db_harness::TestSchema;

#[path = "prompt_logging_harness.rs"]
mod base;

pub use base::enable_prompt_logging;

pub async fn enable_raw_prompt_logging(schema: &TestSchema) -> anyhow::Result<()> {
    base::set_prompt_logging(schema, RequestContentLoggingMode::NormalizedAndRaw).await
}
