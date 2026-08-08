#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/prompt_logging_harness.rs"]
mod prompt_logging_harness;
#[path = "support/replay_harness.rs"]
mod relay_harness;
#[path = "support/replay_upstream_harness.rs"]
mod replay_upstream_harness;
#[path = "support/worker_database_url_harness.rs"]
mod worker_database_url_harness;
#[path = "support/worker_spawn_harness.rs"]
mod worker_spawn_harness;

use std::sync::Arc;

use axum::http::StatusCode;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::prompt_logging_harness::enable_prompt_logging;
use crate::relay_harness::{spawn_relay, wait_for_worker};
use crate::replay_upstream_harness::{ChatRequestLog, spawn_replay_upstream};
use crate::worker_database_url_harness::worker_database_url;
use crate::worker_spawn_harness::spawn_worker;

#[derive(sqlx::FromRow)]
struct ChatSessionRow {
    conversation_id: Option<uuid::Uuid>,
    conversation_seq: Option<i32>,
    conversation_source: String,
    path: String,
}

async fn latest_rows(schema: &TestSchema, limit: i64) -> anyhow::Result<Vec<ChatSessionRow>> {
    Ok(sqlx::query_as::<_, ChatSessionRow>(
        r#"
        SELECT conversation_id, conversation_seq, conversation_source, path
        FROM request_records
        WHERE event_kind = 'request'
        ORDER BY event_id DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(&schema.pool)
    .await?)
}

async fn wait_for_persisted_requests(schema: &TestSchema, expected: i64) -> anyhow::Result<()> {
    for _ in 0..200 {
        let request_records = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM request_records
            WHERE event_kind = 'request'
            "#,
        )
        .fetch_one(&schema.pool)
        .await?;
        if request_records < expected {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            continue;
        }
        // Block refs are the final step of request persistence, so waiting for
        // them guarantees the worker finished its full persistence tail before
        // the schema is dropped.
        let block_refs =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM request_record_block_refs")
                .fetch_one(&schema.pool)
                .await?;
        if block_refs >= expected {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    anyhow::bail!("request records were not persisted before schema cleanup")
}

async fn wait_for_schema_quiescent(schema: &TestSchema) -> anyhow::Result<()> {
    for _ in 0..400 {
        let locks = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM pg_locks locks
            JOIN pg_class cls ON cls.oid = locks.relation
            JOIN pg_namespace nsp ON nsp.oid = cls.relnamespace
            WHERE nsp.nspname = $1
              AND locks.pid <> pg_backend_pid()
            "#,
        )
        .bind(&schema.schema_name)
        .fetch_one(&schema.pool)
        .await?;
        if locks == 0 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    anyhow::bail!("schema did not quiesce before cleanup")
}

struct ChatSessionHarness {
    schema: TestSchema,
    relay_addr: std::net::SocketAddr,
    worker_handle: tokio::task::JoinHandle<anyhow::Result<()>>,
}

impl ChatSessionHarness {
    async fn spawn() -> anyhow::Result<Self> {
        let schema = TestSchema::new().await?;
        enable_prompt_logging(&schema).await?;
        let upstream_log = Arc::new(ChatRequestLog::default());
        let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
        let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
        let mut worker_handle =
            spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
        wait_for_worker(&relay_handle, &mut worker_handle).await;
        Ok(Self {
            schema,
            relay_addr,
            worker_handle,
        })
    }

    async fn shutdown(self) -> anyhow::Result<()> {
        self.worker_handle.abort();
        wait_for_schema_quiescent(&self.schema).await?;
        self.schema.cleanup().await
    }

    async fn post_chat(&self, session_id: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("http://{}/v1/chat/completions", self.relay_addr))
            .bearer_auth("client-token")
            .header("X-Session-Id", session_id)
            .json(&serde_json::json!({
                "model": "gpt-test",
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .send()
            .await
            .expect("chat request should send")
    }

    async fn post_chat_with_assert(&self, session_id: &str) -> anyhow::Result<()> {
        let response = self.post_chat(session_id).await;
        let status = response.status();
        if status != StatusCode::OK {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("chat request failed with {status}: {body}");
        }
        Ok(())
    }

    async fn post_responses(&self, session_id: &str) -> reqwest::Response {
        reqwest::Client::new()
            .post(format!("http://{}/v1/responses", self.relay_addr))
            .bearer_auth("client-token")
            .header("X-Session-Id", session_id)
            .json(&serde_json::json!({
                "model": "gpt-test",
                "input": "hello"
            }))
            .send()
            .await
            .expect("responses request should send")
    }
}

impl Drop for ChatSessionHarness {
    fn drop(&mut self) {
        self.worker_handle.abort();
    }
}

#[tokio::test]
async fn chat_session_header_derives_stable_conversation_and_increments_seq() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let harness = ChatSessionHarness::spawn().await?;

    let turn1 = harness.post_chat("thread-1").await;
    assert_eq!(turn1.status(), StatusCode::OK);
    harness.post_chat_with_assert("thread-1").await?;

    wait_for_persisted_requests(&harness.schema, 2).await?;
    let rows = latest_rows(&harness.schema, 2).await?;
    assert_eq!(rows.len(), 2);
    let first = &rows[1];
    let second = &rows[0];
    assert_eq!(first.conversation_source, "chat_session_header");
    assert_eq!(second.conversation_source, "chat_session_header");
    assert_eq!(first.conversation_id, second.conversation_id);
    assert_eq!(first.conversation_seq, Some(1));
    assert_eq!(second.conversation_seq, Some(2));

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn chat_session_header_isolates_distinct_session_ids() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let harness = ChatSessionHarness::spawn().await?;

    let turn1 = harness.post_chat("thread-a").await;
    assert_eq!(turn1.status(), StatusCode::OK);
    let turn2 = harness.post_chat("thread-b").await;
    assert_eq!(turn2.status(), StatusCode::OK);

    wait_for_persisted_requests(&harness.schema, 2).await?;
    let rows = latest_rows(&harness.schema, 2).await?;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].conversation_id, rows[1].conversation_id);
    assert_eq!(rows[0].conversation_seq, Some(1));
    assert_eq!(rows[1].conversation_seq, Some(1));

    harness.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn chat_and_responses_share_session_header_but_stay_isolated() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let harness = ChatSessionHarness::spawn().await?;

    let chat = harness.post_chat("shared-thread").await;
    assert_eq!(chat.status(), StatusCode::OK);
    let responses = harness.post_responses("shared-thread").await;
    assert_eq!(responses.status(), StatusCode::OK);

    wait_for_persisted_requests(&harness.schema, 2).await?;
    let rows = latest_rows(&harness.schema, 2).await?;
    assert_eq!(rows.len(), 2);
    let chat_row = rows
        .iter()
        .find(|row| row.path == "/v1/chat/completions")
        .expect("chat row");
    let responses_row = rows
        .iter()
        .find(|row| row.path == "/v1/responses")
        .expect("responses row");
    assert_eq!(chat_row.conversation_source, "chat_session_header");
    assert_eq!(responses_row.conversation_source, "session_header");
    assert_ne!(chat_row.conversation_id, responses_row.conversation_id);

    harness.shutdown().await?;
    Ok(())
}
