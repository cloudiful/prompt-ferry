//! Issue #564 Task 3: regression coverage for upstream redaction session
//! stability. The same conversation must keep the same token per entity across
//! turns, the persisted `conversation_redaction_sessions` row must keep
//! advancing, and a failed turn must not clear it.

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
use prompt_ferry::{db, redact::RedactionConfig};
use redactor::RedactionRules;
use serde_json::Value;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::relay_harness::{spawn_relay, wait_for_worker};
use crate::replay_upstream_harness::{ChatRequestLog, spawn_replay_upstream};
use crate::worker_database_url_harness::worker_database_url;
use crate::worker_spawn_harness::spawn_worker;

const SESSION_HEADER: &str = "x-session-id";
const SESSION_ID: &str = "redaction-stability-session";
const SECRET_DOMAIN: &str = "secret.example.com";

/// Collect every redaction token in a serialized JSON value, in document order.
fn redaction_tokens(value: &Value) -> Vec<String> {
    let mut tokens = Vec::new();
    collect_tokens(value, &mut tokens);
    tokens
}

fn collect_tokens(value: &Value, tokens: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let mut rest = text.as_str();
            while let Some(start) = rest.find("[[RDX:v2:") {
                let tail = &rest[start..];
                let Some(end) = tail.find("]]") else {
                    break;
                };
                tokens.push(tail[..end + 2].to_string());
                rest = &tail[end + 2..];
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tokens(item, tokens);
            }
        }
        Value::Object(fields) => {
            for field in fields.values() {
                collect_tokens(field, tokens);
            }
        }
        _ => {}
    }
}

fn turn_body(counter: usize) -> Value {
    serde_json::json!({
        "model": "gpt-test",
        "messages": [{
            "role": "user",
            "content": format!("lookup #{counter} for {SECRET_DOMAIN}")
        }],
        "stream": false
    })
}

async fn send_turn(
    client: &reqwest::Client,
    relay_addr: std::net::SocketAddr,
    counter: usize,
) -> anyhow::Result<reqwest::Response> {
    Ok(client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .header(SESSION_HEADER, SESSION_ID)
        .json(&turn_body(counter))
        .send()
        .await?)
}

async fn session_row(pool: &sqlx::PgPool) -> anyhow::Result<Option<(i64, Option<i64>)>> {
    Ok(sqlx::query_as::<_, (i64, Option<i64>)>(
        "SELECT policy_version, last_event_id
         FROM conversation_redaction_sessions
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .fetch_optional(pool)
    .await?)
}

/// The usage record (and with it the redaction session upsert) is written after
/// the response body has already reached the client, so every read polls until
/// the turn's own write has landed. Waiting for the row's `last_event_id` to
/// advance (rather than merely exist) keeps a previous turn's row from
/// satisfying the read.
async fn wait_for_session_row_after(
    pool: &sqlx::PgPool,
    previous_event_id: Option<i64>,
) -> anyhow::Result<Option<(i64, Option<i64>)>> {
    for _ in 0..250 {
        let row = session_row(pool).await?;
        if row.as_ref().is_some_and(
            |(_, last_event_id)| match (last_event_id, previous_event_id) {
                (Some(last), Some(previous)) => *last > previous,
                (Some(_), None) => true,
                _ => false,
            },
        ) {
            return Ok(row);
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    session_row(pool).await
}

/// Persist a domain redaction policy before the worker boots: the worker
/// bootstrap reloads the global redaction runtime from the database, so a
/// process-local `apply_config` would be overwritten.
async fn enable_domain_redaction(schema: &TestSchema) -> anyhow::Result<()> {
    let config = RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    };
    db::set_redaction_config(&schema.pool, &config).await
}

#[tokio::test]
async fn single_turn_persists_redaction_session() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_logging_harness::enable_prompt_logging(&schema).await?;
    enable_domain_redaction(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = send_turn(&client, relay_addr, 1).await?;
    assert_eq!(response.status(), StatusCode::OK);

    let row = wait_for_session_row_after(&schema.pool, None).await?;
    assert!(
        row.is_some(),
        "conversation redaction session must be persisted after a redacted turn"
    );
    let (policy_version, last_event_id) = row.expect("row");
    assert!(policy_version > 0);
    assert!(last_event_id.is_some());

    let upstream = upstream_log.bodies.lock().await;
    let tokens = redaction_tokens(&upstream[0]);
    assert!(
        !tokens.is_empty(),
        "upstream chat body must carry a redaction token for {SECRET_DOMAIN}"
    );
    drop(upstream);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn token_stays_stable_across_turns_and_a_failed_turn_keeps_the_session() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    prompt_logging_harness::enable_prompt_logging(&schema).await?;
    enable_domain_redaction(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    // The third upstream call fails with a 502 so the turn is recorded as a
    // failure while still carrying the conversation's redaction session.
    upstream_log.fail_next_chat_turns.lock().await.push(3);
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let mut last_event_ids = Vec::new();
    for turn in 1..=4 {
        let response = send_turn(&client, relay_addr, turn).await?;
        if turn == 3 {
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        } else {
            assert_eq!(response.status(), StatusCode::OK, "turn {turn}");
        }

        let (policy_version, last_event_id) =
            wait_for_session_row_after(&schema.pool, last_event_ids.last().copied())
                .await?
                .expect("redaction session row after every turn");
        assert!(policy_version > 0);
        last_event_ids.push(last_event_id.expect("last_event_id"));
    }

    assert!(
        last_event_ids.windows(2).all(|pair| pair[0] < pair[1]),
        "last_event_id must advance across turns: {last_event_ids:?}"
    );

    let upstream = upstream_log.bodies.lock().await;
    assert_eq!(upstream.len(), 4, "one upstream attempt per turn");
    let first_tokens = redaction_tokens(&upstream[0]);
    assert!(!first_tokens.is_empty(), "turn 1 must be redacted");
    for (index, body) in upstream.iter().enumerate() {
        let tokens = redaction_tokens(body);
        assert_eq!(
            tokens,
            first_tokens,
            "turn {} must reuse the same entity token",
            index + 1
        );
    }
    drop(upstream);

    schema.cleanup().await?;
    Ok(())
}
