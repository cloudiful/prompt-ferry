use prompt_ferry::{
    db,
    worker_admin_types::{RequestContentLoggingMode, RequestContentLoggingResponse},
};

use crate::db_harness::TestSchema;

pub async fn enable_prompt_logging(schema: &TestSchema) -> anyhow::Result<()> {
    set_prompt_logging(schema, RequestContentLoggingMode::NormalizedOnly).await
}

pub async fn set_prompt_logging(
    schema: &TestSchema,
    mode: RequestContentLoggingMode,
) -> anyhow::Result<()> {
    db::migrate(&schema.pool).await?;
    db::set_request_content_logging(
        &schema.pool,
        &RequestContentLoggingResponse {
            mode,
            raw_retention_days: 3,
        },
    )
    .await?;
    Ok(())
}
