#[path = "support/db_harness.rs"]
mod db_harness;
#[path = "support/raw_prompt_logging_harness.rs"]
mod prompt_logging_harness;
#[path = "support/replay_harness.rs"]
mod relay_harness;
#[path = "support/replay_responses_upstream_harness.rs"]
mod replay_responses_upstream_harness;
#[path = "support/worker_database_url_harness.rs"]
mod worker_database_url_harness;
#[path = "support/worker_spawn_harness.rs"]
mod worker_spawn_harness;

use std::sync::Arc;

use anyhow::Context;
use axum::http::StatusCode;
use chrono::Utc;
use prompt_ferry::chat_replay::{ResponsesReplayRequest, prepare_responses_replay_request};
use prompt_ferry::config::NativeApiSource;
use prompt_ferry::db;
use prompt_ferry::replay_cache::{ReplayCache, ReplaySnapshotValue};
use serde_json::Value;
use uuid::Uuid;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::prompt_logging_harness::{enable_prompt_logging, enable_raw_prompt_logging};
use crate::relay_harness::{spawn_relay, wait_for_worker, worker_config};
use crate::replay_responses_upstream_harness::{
    ChatRequestLog, ResponsesRequestLog, spawn_replay_responses_upstream,
    spawn_replay_responses_upstream_without_conversation, spawn_replay_upstream,
};
use crate::worker_database_url_harness::worker_database_url;
use crate::worker_spawn_harness::spawn_worker;

struct LatestReplayRequestRow {
    event_id: i64,
    user_id: Option<i64>,
    conversation_id: Uuid,
    conversation_seq: i32,
    provider_response_id: String,
}

struct LatestReplaySnapshotRow {
    conversation_seq: i32,
    ref_count: i32,
    byte_size: i32,
}

struct ConversationContinuationRow {
    event_id: i64,
    parent_event_id: Option<i64>,
    conversation_seq: i32,
    request_state: String,
}

async fn wait_for_assistant_artifact(schema: &TestSchema) -> anyhow::Result<(bool, bool)> {
    for _ in 0..100 {
        if let Some(row) = sqlx::query_as::<_, (bool, bool)>(
            "SELECT has_reasoning_content, has_tool_calls
             FROM request_record_assistant_artifacts
             ORDER BY created_at DESC
             LIMIT 1",
        )
        .fetch_optional(&schema.pool)
        .await?
        {
            return Ok(row);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    anyhow::bail!(
        "assistant artifact was not persisted in test schema {}",
        schema.schema_name
    )
}

#[tokio::test]
async fn creates_responses_conversation_via_public_api() -> anyhow::Result<()> {
    let (relay_addr, _, _) = spawn_relay().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/conversations"))
        .bearer_auth("client-token")
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.json::<Value>().await?;
    assert_eq!(body["object"].as_str(), Some("conversation"));
    assert!(
        body["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("conv_"))
    );
    assert!(body["created_at"].as_i64().is_some());
    assert_eq!(body["metadata"], serde_json::json!({}));
    Ok(())
}

#[tokio::test]
async fn sanitizes_nul_bytes_for_request_storage_without_mutating_upstream_payload()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_raw_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "role": "user",
                "content": "before\u{0000}after"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(
        requests[0]["messages"][0]["content"].as_str(),
        Some("before\u{0000}after")
    );
    drop(requests);

    let row = sqlx::query_as::<_, (bool, i32, Option<String>)>(
        r#"
        SELECT rr.storage_sanitized,
               rr.storage_sanitized_nul_count,
               raw.request_raw_json #>> '{input,0,content}'
        FROM request_records rr
        JOIN request_record_raw_payloads raw
          ON raw.event_id = rr.event_id
         AND raw.created_at = rr.created_at
        WHERE rr.event_kind = 'request'
        ORDER BY rr.created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(row.0);
    assert!(row.1 > 0);
    assert_eq!(row.2.as_deref(), Some("beforeafter"));

    let prompt_block = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT content_json ->> 'content', preview_text
        FROM usage_prompt_blocks
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(prompt_block.0, "beforeafter");
    assert_eq!(prompt_block.1, "beforeafter");

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn opencode_go_chat_history_passes_through_without_local_rejection() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let pool = db::connect(&worker_database_url(&schema)?).await?;
    let endpoint = db::create_endpoint(
        &pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "opencode-go-aggregate".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Chat,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "deepseek-v4-flash".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    pool.close().await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [{"role":"user","content":"need weather"}],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);
    assert_eq!(wait_for_assistant_artifact(&schema).await?, (true, true));

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/chat/completions"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role":"user","content":"need weather"},
                {"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Boston\"}"}}]},
                {"role":"tool","tool_call_id":"call_1","content":"72F"}
            ],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]["messages"][1]
            .get("reasoning_content")
            .is_none()
    );
    assert_eq!(
        requests[1]["messages"][1]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[1]["messages"][2]["content"].as_str(), Some("72F"));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn replays_tool_outputs_after_assistant_tool_calls_even_with_instructions()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
                "model": "deepseek-chat",
                "input": "need weather",
                "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);
    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "instructions": "be terse",
            "previous_response_id": "chatcmpl_turn1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["messages"][0]["role"].as_str(), Some("system"));
    assert_eq!(
        requests[1]["messages"][0]["content"].as_str(),
        Some("be terse")
    );
    assert_eq!(
        requests[1]["messages"][2]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(
        requests[1]["messages"][2]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[1]["messages"][3]["role"].as_str(), Some("tool"));
    assert_eq!(
        requests[1]["messages"][3]["tool_call_id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[1]["messages"][3]["content"].as_str(), Some("72F"));

    let parent_event_id = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT event_id
        FROM request_records
        WHERE provider_response_id = 'chatcmpl_turn1'
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    let chain_rows = sqlx::query_as::<_, (Option<i64>, Option<i32>)>(
        r#"
        SELECT parent_event_id, conversation_seq
        FROM request_records
        WHERE provider_response_id = 'chatcmpl_turn2'
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(chain_rows, (Some(parent_event_id), Some(2)));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn does_not_retry_transient_bad_gateway_for_chat_native_continuations() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    upstream_log.fail_next_chat_turns.lock().await.push(2);
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need time",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);
    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "previous_response_id": "chatcmpl_turn1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "2026-05-23T20:45:00+08:00"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::BAD_GATEWAY);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["messages"][2]["tool_call_id"].as_str(),
        Some("call_1")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn persists_sparse_replay_snapshots_for_long_conversations() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let mut previous_response_id: Option<String> = None;
    for turn in 1..=17 {
        let mut body = serde_json::json!({
            "model": "gpt-test",
            "input": format!("turn {turn}"),
            "stream": false
        });
        if let Some(previous) = previous_response_id.as_ref() {
            body["previous_response_id"] = Value::String(previous.clone());
        }
        let response = client
            .post(format!("http://{relay_addr}/v1/responses"))
            .bearer_auth("client-token")
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        previous_response_id = response
            .json::<Value>()
            .await?
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string);
    }

    let snapshot_count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM request_record_replay_snapshots
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert!(snapshot_count >= 2);

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn falls_back_to_pg_snapshot_when_local_cache_snapshot_is_stale() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let worker_db_url = worker_database_url(&schema)?;
    let mut worker_handle = spawn_worker(worker_addr, upstream_addr, &worker_db_url).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let mut previous_response_id: Option<String> = None;
    for turn in 1..=33 {
        let mut body = serde_json::json!({
            "model": "gpt-test",
            "input": format!("turn {turn}"),
            "stream": false
        });
        if let Some(previous) = previous_response_id.as_ref() {
            body["previous_response_id"] = Value::String(previous.clone());
        }
        let response = client
            .post(format!("http://{relay_addr}/v1/responses"))
            .bearer_auth("client-token")
            .json(&body)
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        previous_response_id = response
            .json::<Value>()
            .await?
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string);
    }

    let latest = sqlx::query_file_as!(
        LatestReplayRequestRow,
        "tests/sql/latest_replay_request_row.sql",
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(latest.conversation_seq, 33);

    let persisted = sqlx::query_file_as!(
        LatestReplaySnapshotRow,
        "tests/sql/latest_replay_snapshot_for_conversation.sql",
        latest.conversation_id,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(persisted.conversation_seq, 33);

    let replay_cache = ReplayCache::for_tests();
    replay_cache
        .replace_snapshot_for_tests(ReplaySnapshotValue {
            conversation_id: latest.conversation_id,
            base_event_id: -1,
            conversation_seq: persisted.conversation_seq,
            prompt_refs: Vec::new(),
            ref_count: persisted.ref_count,
            byte_size: persisted.byte_size,
            updated_at: Utc::now(),
        })
        .await?;

    let request_body = serde_json::to_vec(&serde_json::json!({
        "model": "gpt-test",
        "previous_response_id": latest.provider_response_id.clone(),
        "input": "turn 34",
        "stream": false
    }))?;
    let replayed = prepare_responses_replay_request(ResponsesReplayRequest {
        pool: &schema.pool,
        replay_cache: &replay_cache,
        user_id: latest.user_id,
        resolved_parent_event_id: Some(latest.event_id),
        request_body: &request_body,
        native_api: prompt_ferry::config::NativeApi::Responses,
        route_base_url: "http://example.test",
        current_request_model: Some("gpt-test"),
    })
    .await
    .map_err(|err| anyhow::anyhow!("replay assembly failed: {}: {}", err.code, err.message))?;

    let replayed_json: Value = serde_json::from_slice(&replayed)?;
    let replay_input = replayed_json["input"]
        .as_array()
        .context("missing replay input")?;
    assert!(replay_input.len() >= 33);

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn accepts_item_reference_tool_continuations_for_chat_native_routes() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "previous_response_id": "chatcmpl_turn1",
            "input": [{
                "type": "item_reference",
                "id": "call_1"
            }, {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["messages"][1]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(
        requests[1]["messages"][1]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[1]["messages"][2]["role"].as_str(), Some("tool"));
    assert_eq!(
        requests[1]["messages"][2]["tool_call_id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[1]["messages"][2]["content"].as_str(), Some("72F"));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn replays_completed_tool_loop_before_plain_followup_for_chat_native_routes()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "previous_response_id": "chatcmpl_turn1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let turn3 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "previous_response_id": "chatcmpl_turn2",
            "input": "what next",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn3.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(
        requests[2]["messages"][0]["content"].as_str(),
        Some("need weather")
    );
    assert_eq!(
        requests[2]["messages"][1]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(
        requests[2]["messages"][1]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[2]["messages"][2]["role"].as_str(), Some("tool"));
    assert_eq!(
        requests[2]["messages"][2]["tool_call_id"].as_str(),
        Some("call_1")
    );
    assert_eq!(requests[2]["messages"][2]["content"].as_str(), Some("72F"));
    assert_eq!(
        requests[2]["messages"][3]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(requests[2]["messages"][3]["content"].as_str(), Some("done"));
    assert_eq!(requests[2]["messages"][4]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[2]["messages"][4]["content"].as_str(),
        Some("what next")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn returns_explicit_400_when_replay_state_is_missing() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "deepseek-chat",
            "previous_response_id": "missing_resp",
            "input": "use it",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.text().await?.contains("replay_unavailable"));

    let requests = upstream_log.bodies.lock().await;
    assert!(requests.is_empty());

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn repairs_missing_artifact_from_response_prompt_on_replay() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut worker_handle =
        spawn_worker(worker_addr, upstream_addr, &worker_database_url(&schema)?).await;
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    sqlx::query("DELETE FROM request_record_assistant_artifacts")
        .execute(&schema.pool)
        .await?;

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "previous_response_id": "chatcmpl_turn1",
            "input": "use it",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let repaired =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM request_record_assistant_artifacts")
            .fetch_one(&schema.pool)
            .await?;
    assert!(repaired >= 1);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["messages"][1]["content"].as_str(), Some("done"));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn replays_previous_response_id_for_responses_native_upstream_without_forwarding_it()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "instructions": "be terse",
            "previous_response_id": "resp_turn1",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1].get("previous_response_id").is_none());
    assert_eq!(requests[1]["instructions"].as_str(), Some("be terse"));
    assert_eq!(requests[1]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[1]["input"][1]["type"].as_str(),
        Some("function_call")
    );
    assert_eq!(requests[1]["input"][1]["call_id"].as_str(), Some("call_1"));
    assert_eq!(requests[1]["input"][1]["name"].as_str(), Some("lookup"));
    assert_eq!(
        requests[1]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn passthrough_responses_native_tool_continuations_use_stored_replay() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "previous_response_id": "resp_turn1",
            "input": [{
                "type": "item_reference",
                "id": "call_1"
            }, {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1].get("previous_response_id").is_none());
    assert_eq!(requests[1]["store"].as_bool(), Some(true));
    assert_eq!(requests[1]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[1]["input"][1]["type"].as_str(),
        Some("function_call")
    );
    assert_eq!(requests[1]["input"][1]["call_id"].as_str(), Some("call_1"));
    assert_eq!(
        requests[1]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn force_replay_does_not_infer_session_from_tool_output_without_explicit_identity()
-> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_raw_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "type": "item_reference",
                "id": "fc_1"
            }, {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::BAD_REQUEST);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    drop(requests);

    let row = sqlx::query_as::<_, (Option<String>, Option<i64>, Option<i32>, String)>(
        r#"
        SELECT rr.conversation_id::text, rr.parent_event_id, rr.conversation_seq, rr.conversation_source
        FROM request_records rr
        JOIN request_record_raw_payloads raw
          ON raw.event_id = rr.event_id
         AND raw.created_at = rr.created_at
        WHERE raw.request_raw_json -> 'input' -> 0 ->> 'type' = 'item_reference'
        ORDER BY rr.event_id DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(row.0, None);
    assert_eq!(row.1, None);
    assert_eq!(row.2, None);
    assert_eq!(row.3, "none");

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn codex_thread_identity_keeps_tool_output_replay_on_one_conversation() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_raw_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let codex_identity = serde_json::json!({
        "prompt_cache_key": "guardian-parent-session",
        "client_metadata": {
            "x-codex-window-id": "thread-codex-1:0",
            "x-codex-installation-id": "install-1"
        }
    });

    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false,
            "prompt_cache_key": codex_identity["prompt_cache_key"],
            "client_metadata": codex_identity["client_metadata"]
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "type": "item_reference",
                "id": "fc_1"
            }, {
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false,
            "prompt_cache_key": codex_identity["prompt_cache_key"],
            "client_metadata": codex_identity["client_metadata"]
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let turn3 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": [{
                "role": "user",
                "type": "message",
                "content": [{
                    "type": "input_text",
                    "text": "summarize result"
                }]
            }],
            "stream": false,
            "prompt_cache_key": codex_identity["prompt_cache_key"],
            "client_metadata": codex_identity["client_metadata"]
        }))
        .send()
        .await?;
    assert_eq!(turn3.status(), StatusCode::OK);

    let rows = sqlx::query_as::<_, (Option<String>, Option<i32>, String)>(
        r#"
        SELECT rr.conversation_id::text, rr.conversation_seq, rr.conversation_source
        FROM request_records rr
        JOIN request_record_raw_payloads raw
          ON raw.event_id = rr.event_id
         AND raw.created_at = rr.created_at
        WHERE rr.event_kind = 'request'
          AND rr.request_category = 'ai'
          AND raw.request_raw_json ->> 'prompt_cache_key' = 'guardian-parent-session'
        ORDER BY rr.created_at ASC
        "#,
    )
    .fetch_all(&schema.pool)
    .await?;

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].1, Some(1));
    assert_eq!(rows[1].1, Some(2));
    assert_eq!(rows[2].1, Some(3));
    assert_eq!(rows[0].0, rows[1].0);
    assert_eq!(rows[1].0, rows[2].0);
    assert_eq!(rows[0].2, "codex_thread_key");
    assert_eq!(rows[1].2, "codex_thread_key");
    assert_eq!(rows[2].2, "codex_thread_key");

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn responses_session_header_creates_affinity_and_conversation() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let cctq_log = Arc::new(ResponsesRequestLog::default());
    let right_code_log = Arc::new(ResponsesRequestLog::default());
    let cctq_addr = spawn_replay_responses_upstream(cctq_log.clone()).await;
    let right_code_addr = spawn_replay_responses_upstream(right_code_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, cctq_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let Some(valkey_url) = std::env::var("PROMPT_FERRY_TEST_VALKEY_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
    else {
        eprintln!("skipping session affinity bridge test: PROMPT_FERRY_TEST_VALKEY_URL is not set");
        schema.cleanup().await?;
        return Ok(());
    };
    config.valkey_url = valkey_url;

    let cctq = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "cctq".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{cctq_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "cctq-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    let right_code = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "right-code".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{right_code_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "right-code-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ResponsesSessionAffinity,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![
                db::ModelRouteTargetCreate {
                    endpoint_id: cctq.endpoint_id,
                    enabled: true,
                    upstream_model: None,
                    responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                },
                db::ModelRouteTargetCreate {
                    endpoint_id: right_code.endpoint_id,
                    enabled: true,
                    upstream_model: None,
                    responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
                },
            ],
        },
    )
    .await?;

    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    for input in ["first turn", "second turn"] {
        let response = client
            .post(format!("http://{relay_addr}/v1/responses"))
            .bearer_auth("client-token")
            .header("X-Session-Id", "04b1167f")
            .json(&serde_json::json!({
                "model": "gpt-test",
                "input": input,
                "stream": false
            }))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
    }

    let cctq_count = cctq_log.bodies.lock().await.len();
    let right_code_count = right_code_log.bodies.lock().await.len();
    assert_eq!(cctq_count + right_code_count, 2);
    assert!(cctq_count == 0 || right_code_count == 0);

    let rows = sqlx::query_as::<_, (Option<String>, Option<i32>, String, Option<String>)>(
        r#"
        SELECT conversation_id::text, conversation_seq, conversation_source, endpoint_id::text
        FROM request_records
        WHERE event_kind = 'request'
          AND request_category = 'ai'
        ORDER BY created_at ASC
        "#,
    )
    .fetch_all(&schema.pool)
    .await?;

    assert_eq!(rows.len(), 2);
    assert!(rows[0].0.is_some());
    assert_eq!(rows[0].0, rows[1].0);
    assert_eq!(rows[0].1, Some(1));
    assert_eq!(rows[1].1, Some(2));
    assert_eq!(rows[0].2, "session_header");
    assert_eq!(rows[1].2, "session_header");
    assert_eq!(rows[0].3, rows[1].3);

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn raw_passthrough_keeps_previous_response_id_without_replay_state() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForcePassthrough,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "previous_response_id": "resp_turn1",
            "input": [
                {"role":"user","content":"hello"},
                {"role":"developer","content":"keep raw"}
            ],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["previous_response_id"].as_str(),
        Some("resp_turn1")
    );
    assert_eq!(requests[0]["input"][1]["role"].as_str(), Some("developer"));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn raw_passthrough_keeps_conversation_without_replay_state() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForcePassthrough,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": "conv_passthrough",
            "input": [{"role":"user","content":"hello"}],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["conversation"].as_str(),
        Some("conv_passthrough")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn replays_conversation_for_responses_native_upstream() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": "conv_replay",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[1].get("conversation").is_none());
    assert_eq!(requests[1]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[1]["input"][1]["type"].as_str(),
        Some("function_call")
    );
    assert_eq!(requests[1]["input"][1]["call_id"].as_str(), Some("call_1"));
    assert_eq!(
        requests[1]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn replays_conversation_for_chat_native_upstream() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": "conv_replay",
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[1]["input"][1]["type"].as_str(),
        Some("item_reference")
    );
    assert_eq!(
        requests[1]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn starts_explicit_conversation_without_prior_history() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let conversation = client
        .post(format!("http://{relay_addr}/v1/conversations"))
        .bearer_auth("client-token")
        .send()
        .await?
        .json::<Value>()
        .await?;

    let response = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation["id"].as_str().unwrap(),
            "input": "hello",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let row = sqlx::query_as::<_, (String, String, i32)>(
        r#"
        SELECT request_conversation_key, conversation_source, conversation_seq
        FROM request_records
        WHERE path = '/v1/responses'
        ORDER BY event_id DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&schema.pool)
    .await?;
    assert_eq!(row.0, conversation["id"].as_str().unwrap());
    assert_eq!(row.1, "explicit_conversation");
    assert_eq!(row.2, 1);

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn local_conversation_replay_survives_upstream_without_conversation_id() -> anyhow::Result<()>
{
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    let upstream_addr =
        spawn_replay_responses_upstream_without_conversation(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let conversation = client
        .post(format!("http://{relay_addr}/v1/conversations"))
        .bearer_auth("client-token")
        .send()
        .await?
        .json::<Value>()
        .await?;
    let conversation_id = conversation["id"]
        .as_str()
        .expect("conversation id")
        .to_string();

    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation_id,
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation["id"].as_str().unwrap(),
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests[0].get("conversation").is_none());
    assert!(requests[1].get("conversation").is_none());
    assert_eq!(requests[1]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[1]["input"][1]["type"].as_str(),
        Some("function_call")
    );
    assert_eq!(requests[1]["input"][1]["call_id"].as_str(), Some("call_1"));
    assert_eq!(
        requests[1]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );
    drop(requests);

    let rows = sqlx::query_as::<_, (String, String, String, i32)>(
        r#"
        SELECT
            request_conversation_key,
            provider_conversation_key,
            conversation_source,
            conversation_seq
        FROM request_records
        WHERE path = '/v1/responses'
        ORDER BY event_id ASC
        LIMIT 2
        "#,
    )
    .fetch_all(&schema.pool)
    .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, conversation["id"].as_str().unwrap());
    assert_eq!(rows[0].1, conversation["id"].as_str().unwrap());
    assert_eq!(rows[0].2, "explicit_conversation");
    assert_eq!(rows[0].3, 1);
    assert_eq!(rows[1].0, conversation["id"].as_str().unwrap());
    assert_eq!(rows[1].1, conversation["id"].as_str().unwrap());
    assert_eq!(rows[1].2, "explicit_conversation");
    assert_eq!(rows[1].3, 2);

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn explicit_conversation_skips_failed_turn_when_selecting_parent() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ResponsesRequestLog::default());
    {
        let mut fail_turns = upstream_log.fail_next_response_turns.lock().await;
        fail_turns.push(2);
    }
    let upstream_addr = spawn_replay_responses_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Responses;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "responses-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::Responses,
            native_api_source: NativeApiSource::Manual,
            daily_max_requests: None,
            monthly_max_requests: None,
            api_key: "upstream-key".to_string(),
            api_keys: vec![],
            key_lb_enabled: false,
            enabled: true,
        },
    )
    .await?;
    db::create_model_endpoint_rule(
        &schema.pool,
        db::ModelEndpointRuleCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: "gpt-test".to_string(),
            routing_strategy: db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            daily_max_requests: None,
            monthly_max_requests: None,
            enabled: true,
            targets: vec![db::ModelRouteTargetCreate {
                endpoint_id: endpoint.endpoint_id,
                enabled: true,
                upstream_model: None,
                responses_continuation_policy: db::ResponsesContinuationPolicy::ForceReplay,
            }],
        },
    )
    .await?;
    let mut worker_handle = tokio::spawn(async move {
        prompt_ferry::worker::connect_for_test_with_admin(config, reqwest::Client::new()).await
    });
    wait_for_worker(&relay_handle, &mut worker_handle).await;

    let client = reqwest::Client::new();
    let conversation = client
        .post(format!("http://{relay_addr}/v1/conversations"))
        .bearer_auth("client-token")
        .send()
        .await?
        .json::<Value>()
        .await?;
    let conversation_id = conversation["id"]
        .as_str()
        .context("conversation id")?
        .to_string();

    let turn1 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation_id,
            "input": "need weather",
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn1.status(), StatusCode::OK);

    let turn2 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation["id"].as_str().unwrap(),
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "72F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn2.status(), StatusCode::BAD_GATEWAY);

    let turn3 = client
        .post(format!("http://{relay_addr}/v1/responses"))
        .bearer_auth("client-token")
        .json(&serde_json::json!({
            "model": "gpt-test",
            "conversation": conversation["id"].as_str().unwrap(),
            "input": [{
                "type": "function_call_output",
                "call_id": "call_1",
                "output": "73F"
            }],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(turn3.status(), StatusCode::OK);

    let requests = upstream_log.bodies.lock().await;
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[2]["input"][0]["role"].as_str(), Some("user"));
    assert_eq!(
        requests[2]["input"][1]["type"].as_str(),
        Some("function_call")
    );
    assert_eq!(requests[2]["input"][1]["call_id"].as_str(), Some("call_1"));
    assert_eq!(
        requests[2]["input"][2]["type"].as_str(),
        Some("function_call_output")
    );
    assert_eq!(requests[2]["input"][2]["output"].as_str(), Some("73F"));
    drop(requests);

    let rows = sqlx::query_file_as!(
        ConversationContinuationRow,
        "tests/sql/conversation_continuation_rows.sql",
        conversation["id"].as_str().unwrap(),
    )
    .fetch_all(&schema.pool)
    .await?;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].conversation_seq, 1);
    assert_eq!(rows[0].request_state, "completed");
    assert_eq!(rows[1].conversation_seq, 2);
    assert_eq!(rows[1].request_state, "failed");
    assert_eq!(rows[2].conversation_seq, 3);
    assert_eq!(rows[2].request_state, "completed");
    assert_eq!(rows[2].parent_event_id, Some(rows[0].event_id));

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}
