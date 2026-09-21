#[path = "support/db_harness.rs"]
mod db_harness;

use std::sync::{Arc, Mutex};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use db_harness::{TEST_DATABASE_URL_ENV, TestSchema, test_database_configured};
use prompt_ferry::db;
use prompt_ferry::worker::{CacheAlertDependencies, run_cache_alert_check};
use prompt_ferry::worker_admin_types::CacheAlertSettings;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Minimal DingTalk robot stub: records every payload and answers with the
/// DingTalk success envelope.
#[derive(Clone, Default)]
struct MockDingtalk {
    received: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl MockDingtalk {
    fn payloads(&self) -> Vec<serde_json::Value> {
        self.received.lock().expect("mock lock").clone()
    }
}

async fn record_alert(
    State(state): State<MockDingtalk>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    state.received.lock().expect("mock lock").push(payload);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "errcode": 0, "errmsg": "ok" })),
    )
}

async fn start_mock_dingtalk() -> (String, MockDingtalk) {
    let state = MockDingtalk::default();
    let app = Router::new()
        .route("/robot/send", post(record_alert))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock listener");
    let address = listener.local_addr().expect("mock address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}/robot/send"), state)
}

struct Tokens {
    input: i64,
    output: i64,
    total: i64,
    cache_read: i64,
}

const LOW_CACHE: Tokens = Tokens {
    input: 800,
    output: 100,
    total: 1_000,
    cache_read: 100,
};

const HIGH_CACHE: Tokens = Tokens {
    input: 100,
    output: 100,
    total: 1_100,
    cache_read: 900,
};

async fn insert_turn(
    pool: &PgPool,
    conversation_id: Uuid,
    seq: i32,
    state: db::RequestRecordState,
    tokens: &Tokens,
) -> anyhow::Result<()> {
    insert_turn_with_model(pool, conversation_id, seq, state, tokens, "gpt-test").await
}

async fn insert_turn_with_model(
    pool: &PgPool,
    conversation_id: Uuid,
    seq: i32,
    state: db::RequestRecordState,
    tokens: &Tokens,
    model: &str,
) -> anyhow::Result<()> {
    let mut turn = db::RequestRecordCreate::ai_request(Uuid::new_v4(), "/v1/responses");
    turn.request_state = state;
    turn.conversation_id = Some(conversation_id);
    turn.conversation_seq = Some(seq);
    turn.model = Some(model.to_string());
    turn.input_tokens = Some(tokens.input);
    turn.output_tokens = Some(tokens.output);
    turn.total_tokens = Some(tokens.total);
    turn.cache_read_tokens = Some(tokens.cache_read);
    turn.cache_write_tokens = Some(0);
    db::record_request_record(pool, turn).await?;
    Ok(())
}

fn alert_settings(webhook_url: &str) -> CacheAlertSettings {
    CacheAlertSettings {
        enabled: true,
        window_minutes: 30,
        min_turns: 5,
        threshold: 0.2,
        cooldown_minutes: 60,
        dingtalk_webhook_url: webhook_url.to_string(),
        dingtalk_secret: String::new(),
    }
}

async fn dependencies(pool: &PgPool, webhook_url: &str) -> CacheAlertDependencies {
    CacheAlertDependencies {
        pool: pool.clone(),
        settings: Arc::new(RwLock::new(alert_settings(webhook_url))),
        client: reqwest::Client::new(),
    }
}

async fn migrate(pool: &PgPool) -> anyhow::Result<()> {
    db::migrate(pool).await?;
    Ok(())
}

#[tokio::test]
async fn low_cache_conversation_alerts_dingtalk_once() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=5 {
        insert_turn(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
        )
        .await?;
    }

    let deps = dependencies(&schema.pool, &webhook_url).await;
    let delivered = run_cache_alert_check(&deps).await?;

    assert_eq!(delivered, 1);
    let payloads = mock.payloads();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0]["msgtype"], "text");
    let content = payloads[0]["text"]["content"]
        .as_str()
        .expect("text content");
    assert!(content.contains("缓存率异常告警"), "title: {content}");
    assert!(content.contains(&conversation_id.to_string()), "{content}");
    assert!(content.contains("model: gpt-test"), "{content}");
    assert!(content.contains("window: 30m"), "{content}");
    assert!(content.contains("turns: 5"), "{content}");
    assert!(content.contains("cache_rate: 11.1%"), "{content}");
    assert!(content.contains("threshold: 20.0%"), "{content}");

    // The alert fingerprint becomes the cooldown clock for this conversation.
    assert!(
        db::last_cache_alert_at(&schema.pool, conversation_id)
            .await?
            .is_some()
    );

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn healthy_conversation_sends_nothing() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=5 {
        insert_turn(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &HIGH_CACHE,
        )
        .await?;
    }

    let deps = dependencies(&schema.pool, &webhook_url).await;
    let delivered = run_cache_alert_check(&deps).await?;

    assert_eq!(delivered, 0);
    assert!(mock.payloads().is_empty());

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn cooldown_suppresses_repeat_alert() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=5 {
        insert_turn(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
        )
        .await?;
    }

    let deps = dependencies(&schema.pool, &webhook_url).await;
    assert_eq!(run_cache_alert_check(&deps).await?, 1);
    assert_eq!(run_cache_alert_check(&deps).await?, 0);
    assert_eq!(mock.payloads().len(), 1);

    schema.cleanup().await?;
    Ok(())
}

#[tokio::test]
async fn disabled_policy_sends_nothing() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=5 {
        insert_turn(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
        )
        .await?;
    }

    let deps = dependencies(&schema.pool, &webhook_url).await;
    deps.settings.write().await.enabled = false;
    let delivered = run_cache_alert_check(&deps).await?;

    assert_eq!(delivered, 0);
    assert!(mock.payloads().is_empty());

    schema.cleanup().await?;
    Ok(())
}

/// Four completed turns plus a failed and an in-flight turn stay below
/// `min_turns`: only `request_state = 'completed'` rows count as turns.
#[tokio::test]
async fn incomplete_and_failed_turns_do_not_reach_the_threshold() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=4 {
        insert_turn(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
        )
        .await?;
    }
    insert_turn(
        &schema.pool,
        conversation_id,
        5,
        db::RequestRecordState::Failed,
        &LOW_CACHE,
    )
    .await?;
    insert_turn(
        &schema.pool,
        conversation_id,
        6,
        db::RequestRecordState::UpstreamProcessing,
        &LOW_CACHE,
    )
    .await?;

    let deps = dependencies(&schema.pool, &webhook_url).await;
    let delivered = run_cache_alert_check(&deps).await?;

    assert_eq!(delivered, 0);
    assert!(mock.payloads().is_empty());

    schema.cleanup().await?;
    Ok(())
}

/// A mid-conversation model switch must not split one conversation into two
/// candidates: the alert still reflects every completed turn and reports the
/// most recent model once.
#[tokio::test]
async fn model_switch_still_aggregates_one_alert_per_conversation() -> anyhow::Result<()> {
    if !test_database_configured() {
        eprintln!("skipping database integration test: {TEST_DATABASE_URL_ENV} is not set");
        return Ok(());
    }
    let schema = TestSchema::new().await?;
    migrate(&schema.pool).await?;
    let (webhook_url, mock) = start_mock_dingtalk().await;
    let conversation_id = Uuid::new_v4();
    for seq in 1..=3 {
        insert_turn_with_model(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
            "gpt-old",
        )
        .await?;
    }
    for seq in 4..=5 {
        insert_turn_with_model(
            &schema.pool,
            conversation_id,
            seq,
            db::RequestRecordState::Completed,
            &LOW_CACHE,
            "gpt-new",
        )
        .await?;
    }

    let deps = dependencies(&schema.pool, &webhook_url).await;
    let delivered = run_cache_alert_check(&deps).await?;

    assert_eq!(delivered, 1);
    let payloads = mock.payloads();
    assert_eq!(payloads.len(), 1);
    let content = payloads[0]["text"]["content"]
        .as_str()
        .expect("text content");
    assert!(content.contains("turns: 5"), "{content}");
    assert!(content.contains("model: gpt-new"), "{content}");

    schema.cleanup().await?;
    Ok(())
}
