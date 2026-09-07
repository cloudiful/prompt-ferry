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

use axum::http::StatusCode;
use prompt_ferry::config::NativeApiSource;
use prompt_ferry::db;
use serde_json::Value;

use crate::db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use crate::prompt_logging_harness::{enable_prompt_logging, enable_raw_prompt_logging};
use crate::relay_harness::{spawn_relay, wait_for_worker, worker_config};
use crate::replay_responses_upstream_harness::{
    ChatRequestLog, ResponsesRequestLog, spawn_replay_responses_upstream, spawn_replay_upstream,
};
use crate::worker_database_url_harness::worker_database_url;
use crate::worker_spawn_harness::spawn_worker;

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
                },
                db::ModelRouteTargetCreate {
                    endpoint_id: right_code.endpoint_id,
                    enabled: true,
                    upstream_model: None,
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
async fn rejects_responses_routed_to_chat_native_target() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::Chat;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "chat-upstream".to_string(),
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
            "input": [{"role":"user","content":"hello"}],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.json::<Value>().await?;
    assert_eq!(
        body["error"]["code"].as_str(),
        Some("responses_cross_protocol_unsupported")
    );

    assert!(
        upstream_log.bodies.lock().await.is_empty(),
        "rejected cross-protocol responses must not reach the chat upstream"
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn rejects_responses_routed_to_anthropic_native_target() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    enable_prompt_logging(&schema).await?;

    let upstream_log = Arc::new(ChatRequestLog::default());
    let upstream_addr = spawn_replay_upstream(upstream_log.clone()).await;
    let (relay_addr, worker_addr, relay_handle) = spawn_relay().await;
    let mut config = worker_config(worker_addr, upstream_addr, &worker_database_url(&schema)?);
    config.upstream_native_api = prompt_ferry::config::NativeApi::AnthropicMessages;
    let endpoint = db::create_endpoint(
        &schema.pool,
        db::EndpointCreate {
            scope: "admin".to_string(),
            owner_user_id: None,
            name: "anthropic-upstream".to_string(),
            provider: db::EndpointProvider::Generic,
            provider_region: None,
            service_tier: Default::default(),
            base_url: format!("http://{upstream_addr}"),
            native_api: prompt_ferry::config::NativeApi::AnthropicMessages,
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
            "conversation": "conv_cross_protocol",
            "input": [{"role":"user","content":"hello"}],
            "stream": false
        }))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.json::<Value>().await?;
    assert_eq!(
        body["error"]["code"].as_str(),
        Some("responses_cross_protocol_unsupported")
    );

    worker_handle.abort();
    schema.cleanup().await?;
    Ok(())
}
