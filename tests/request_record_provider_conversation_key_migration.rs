#[path = "support/db_harness.rs"]
mod db_harness;

use prompt_ferry::db;
use sqlx::Executor;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};

#[tokio::test]
async fn migrate_adds_conversation_key_columns_to_renamed_request_records_table()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }

    let schema = TestSchema::new().await?;

    schema
        .pool
        .execute(
            r#"
            CREATE TABLE users (
                user_id BIGSERIAL PRIMARY KEY,
                login_name TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                display_name TEXT NOT NULL,
                is_admin BOOLEAN NOT NULL DEFAULT FALSE,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE provider_endpoints (
                endpoint_id UUID PRIMARY KEY DEFAULT (md5(random()::text || clock_timestamp()::text)::uuid),
                scope TEXT NOT NULL CHECK (scope IN ('admin', 'user')),
                owner_user_id BIGINT REFERENCES users(user_id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );

            CREATE TABLE usage_events (
                event_id BIGSERIAL PRIMARY KEY,
                request_id UUID NOT NULL,
                user_id BIGINT REFERENCES users(user_id) ON DELETE SET NULL,
                endpoint_id UUID REFERENCES provider_endpoints(endpoint_id) ON DELETE SET NULL,
                path TEXT NOT NULL,
                model TEXT,
                status INTEGER,
                ok BOOLEAN NOT NULL,
                duration_ms BIGINT NOT NULL,
                first_chunk_ms BIGINT,
                input_tokens BIGINT,
                output_tokens BIGINT,
                total_tokens BIGINT,
                cached_tokens BIGINT,
                cache_read_tokens BIGINT,
                cache_write_tokens BIGINT,
                request_prompt TEXT,
                response_prompt TEXT,
                upstream_error_body TEXT,
                error_code TEXT,
                error_message TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            );
            "#,
        )
        .await?;

    db::migrate(&schema.pool).await?;

    let columns = sqlx::query_as::<_, (bool, bool, bool)>(
        r#"
        SELECT
            EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'provider_conversation_key'
            ),
            EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'request_conversation_key'
            ),
            EXISTS (
                SELECT 1
                FROM information_schema.columns
                WHERE table_schema = current_schema()
                  AND table_name = 'request_records'
                  AND column_name = 'request_conversation_parent_found'
            )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;

    assert!(columns.0);
    assert!(columns.1);
    assert!(columns.2);

    let index_exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM pg_indexes
            WHERE schemaname = current_schema()
              AND tablename = 'request_records'
              AND indexname = 'idx_request_records_provider_conversation_key'
        )
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;

    assert!(index_exists);

    schema.cleanup().await?;
    Ok(())
}
