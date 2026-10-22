// Issue #1032 P1: the embeddings protocol and the per-target dimension
// override.
//
// The persistence contract is the subject here: the new `embeddings` native
// API and the nullable positive `embedding_dimensions_override` must
// round-trip through the PostgreSQL route repository, the admin request
// contract must distinguish an omitted field from an explicit `null`, and the
// schema must reject a non-positive dimension. No upstream request is made and
// no runtime forwarding behavior is asserted — that is P2.

use prompt_ferry::config::NativeApi;
use prompt_ferry::db::{
    self, CompactMode, ModelEndpointRuleCreate, ModelRouteRoutingStrategy, ModelRouteTargetCreate,
    normalize_embedding_dimensions_override,
};
use prompt_ferry::worker_admin_types::ModelRouteTargetRequest;
use uuid::Uuid;

#[test]
fn reviewer_embeddings_reject_compact_even_with_stored_self_summarize() {
    let result = prompt_ferry::upstream_adapter::prepare_upstream_request_with_compact(
        "/v1/responses/compact",
        br#"{"model":"m","input":"hello"}"#,
        NativeApi::Embeddings,
        false,
        None,
        CompactMode::SelfSummarize,
    );
    assert!(
        result.is_err(),
        "embeddings must reject compact, got {result:?}"
    );
}

fn capture_embedding_body(body: &str) -> prompt_ferry::usage::TokenUsage {
    let mut capture = prompt_ferry::usage::UsageCapture::new(false, None);
    capture.set_embedding_response();
    capture.observe_embedding_body(body.as_bytes());
    capture.finish();
    capture.usage
}

fn embedding_capture(usage: &str) -> prompt_ferry::usage::TokenUsage {
    let body = format!(r#"{{"data":[{{"embedding":[0.1]}}],"usage":{usage}}}"#);
    let mut capture = prompt_ferry::usage::UsageCapture::new(false, None);
    capture.set_embedding_response();
    capture.observe_embedding_body(body.as_bytes());
    capture.finish();
    capture.usage
}

fn reviewer_large_embedding_body(prefix: &str) -> String {
    format!(
        "{prefix}\"data\":[{{\"object\":\"embedding\",\"index\":0,\"embedding\":[{}0.125]}}],\"usage\":{{\"prompt_tokens\":11,\"total_tokens\":11}}}}",
        "0.125,".repeat(180_000)
    )
}

fn reviewer_capture_embedding_body(body: &str) -> prompt_ferry::usage::TokenUsage {
    let parsed: serde_json::Value = serde_json::from_str(body).expect("valid embeddings JSON");
    assert_eq!(parsed["usage"]["prompt_tokens"], 11);
    let mut capture = prompt_ferry::usage::UsageCapture::new(false, None);
    capture.set_embedding_response();
    capture.observe_embedding_body(body.as_bytes());
    capture.finish();
    capture.usage
}

#[test]
fn reviewer_large_embedding_usage_ignores_model_string_values() {
    let control = reviewer_large_embedding_body(r#"{"object":"list","model":"embedding-model","#);
    let body = reviewer_large_embedding_body(r#"{"object":"list","model":"usage","#);
    assert!(body.len() > 1024 * 1024);
    assert_eq!(
        reviewer_capture_embedding_body(&control).input_tokens,
        Some(11)
    );
    assert_eq!(
        reviewer_capture_embedding_body(&body).input_tokens,
        Some(11)
    );
}

#[test]
fn reviewer_large_embedding_usage_ignores_nested_extension_objects() {
    let body = reviewer_large_embedding_body(
        r#"{"object":"list","model":"embedding-model","metadata":{"usage":{"prompt_tokens":999,"total_tokens":999}},"#,
    );
    assert!(body.len() > 1024 * 1024);
    assert_eq!(
        reviewer_capture_embedding_body(&body).input_tokens,
        Some(11)
    );
}

#[test]
fn reviewer_short_embedding_usage_ignores_model_strings_and_nested_objects() {
    // The same provenance rule as the reviewer's large-body cases, exercised on
    // a short body: a model string equal to the key name and a nested extension
    // object never divert the root usage.
    let body = r#"{"object":"list","model":"usage","metadata":{"usage":{"prompt_tokens":999,"total_tokens":999}},"data":[{"object":"embedding","embedding":[0.1]}],"usage":{"prompt_tokens":11,"total_tokens":11}}"#;
    let usage = reviewer_capture_embedding_body(body);
    assert_eq!(usage.input_tokens, Some(11));
    assert_eq!(usage.total_tokens, Some(11));
    assert_eq!(usage.output_tokens, Some(0));

    // A present-but-unusable usage stays unknown instead of falling through.
    for shape in ["null", "7", "\"usage\"", "[]"] {
        let body = format!(r#"{{"usage":{shape},"data":[{{"usage":{{"prompt_tokens":999}}}}]}}"#);
        let usage = capture_embedding_body(&body);
        assert_eq!(usage.input_tokens, None, "shape {shape}");
        assert_eq!(usage.total_tokens, None, "shape {shape}");
    }
}

#[test]
fn reviewer_embedding_wrong_typed_string_maps_preserve_usage() {
    for field in ["model", "id", "conversation"] {
        let body = format!(
            r#"{{"{field}":{{"extension":1}},"usage":{{"prompt_tokens":11,"total_tokens":11}}}}"#
        );
        let _: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        assert_eq!(
            capture_embedding_body(&body).input_tokens,
            Some(11),
            "{field}"
        );
    }
}

#[test]
fn reviewer_embedding_sse_never_retains_response_text() {
    let mut capture = prompt_ferry::usage::UsageCapture::new(true, None);
    capture.set_embedding_response();
    capture.observe_chunk(
        b"data: {\"text\":\"embedding-sensitive-text\",\"data\":[{\"embedding\":[0.125]}],\"usage\":{\"prompt_tokens\":11}}\n\n",
    );
    capture.finish();
    assert_eq!(capture.usage.input_tokens, Some(11));
    assert!(
        capture.response_text.is_empty(),
        "retained {:?}",
        capture.response_text
    );
}

#[test]
fn reviewer_embedding_wrong_typed_string_maps_keep_typed_siblings() {
    // A wrong-typed string metadata map is consumed structurally (including
    // nested objects and arrays) so the rest of the object still parses.
    for field in ["model", "id", "conversation"] {
        let body = format!(
            r#"{{"{field}":{{"nested":{{"usage":{{"prompt_tokens":999}}}},"list":[1,2]}},"usage":{{"prompt_tokens":11,"total_tokens":11}}}}"#
        );
        let _: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
        let usage = capture_embedding_body(&body);
        assert_eq!(usage.input_tokens, Some(11), "{field}");
        assert_eq!(usage.total_tokens, Some(11), "{field}");
    }

    // An unusable map field does not hide a valid sibling metadata string.
    let body = r#"{"id":[1,2],"model":{"a":1},"conversation":"conversation-1","usage":{"prompt_tokens":11}}"#;
    let mut capture = prompt_ferry::usage::UsageCapture::new(false, None);
    capture.set_embedding_response();
    capture.observe_embedding_body(body.as_bytes());
    capture.finish();
    assert_eq!(capture.usage.input_tokens, Some(11));
    assert_eq!(
        capture.provider_conversation_key.as_deref(),
        Some("conversation-1")
    );
}

#[test]
fn reviewer_embedding_sse_keeps_typed_metadata_without_generic_capture() {
    let mut capture = prompt_ferry::usage::UsageCapture::new(true, None);
    capture.set_embedding_response();
    capture.observe_chunk(
        b"data: {\"id\":\"emb-1\",\"model\":\"m\",\"text\":\"embedding-sensitive-text\",\"data\":[{\"embedding\":[0.125]}],\"usage\":{\"prompt_tokens\":11}}\n\n",
    );
    capture.finish();
    assert_eq!(capture.usage.input_tokens, Some(11));
    assert_eq!(capture.response_id.as_deref(), Some("emb-1"));
    assert_eq!(capture.model.as_deref(), Some("m"));
    assert!(
        capture.response_text.is_empty(),
        "retained {:?}",
        capture.response_text
    );
}

#[test]
fn reviewer_embedding_wrapper_presence_and_size_preserve_provenance() {
    for (fields, expected) in [
        (
            r#""response":{"usage":{"prompt_tokens":3}},"message":{"usage":{"prompt_tokens":5}},"usage":{"prompt_tokens":11}"#,
            Some(3),
        ),
        (
            r#""response":null,"message":{"usage":{"prompt_tokens":5}},"usage":{"prompt_tokens":11}"#,
            Some(11),
        ),
        (
            r#""response":7,"message":{"usage":{"prompt_tokens":5}},"usage":{"prompt_tokens":11}"#,
            Some(11),
        ),
        (
            r#""response":{"usage":null},"usage":{"prompt_tokens":11}"#,
            None,
        ),
        (
            r#""response":{"usage":[]},"usage":{"prompt_tokens":11}"#,
            None,
        ),
        (
            r#""response":{"id":"response-id"},"usage":{"prompt_tokens":11}"#,
            Some(11),
        ),
        (
            r#""message":{"usage":{"prompt_tokens":5}},"usage":{"prompt_tokens":11}"#,
            Some(5),
        ),
        (
            r#""message":{"usage":null},"usage":{"prompt_tokens":11}"#,
            None,
        ),
        (r#""u\u0073age":{"prompt_tokens":11}"#, Some(11)),
    ] {
        for padding in [String::new(), "0.125,".repeat(180_000)] {
            let body = format!(r#"{{"data":[{{"embedding":[{padding}0.125]}}],{fields}}}"#);
            let _: serde_json::Value = serde_json::from_str(&body).expect("valid JSON");
            assert_eq!(
                capture_embedding_body(&body).input_tokens,
                expected,
                "{fields}"
            );
        }
    }
}

#[test]
fn reviewer_embedding_total_without_explicit_total_uses_only_actual_input() {
    let usage = embedding_capture(r#"{"prompt_tokens":4,"completion_tokens":3}"#);
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.output_tokens, Some(0));
    assert_eq!(usage.total_tokens, Some(4));
}

#[test]
fn reviewer_missing_embedding_input_never_creates_a_positive_output_meter() {
    let usage = embedding_capture(r#"{"completion_tokens":3}"#);
    assert_eq!(usage.input_tokens, None);
    let billed = db::NormalizedBillingUsage::from_usage(&usage);
    assert!(
        billed.is_none_or(|usage| usage.output_tokens == 0),
        "missing input must not create positive output billing: {billed:?}"
    );
}

#[test]
fn reviewer_embeddings_never_bill_reported_completion_tokens() {
    let usage = embedding_capture(r#"{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}"#);
    assert_eq!(usage.output_tokens, Some(0));
    assert_eq!(usage.input_tokens, Some(4));
    assert_eq!(usage.total_tokens, Some(7));
}

#[test]
fn embeddings_usage_zeroes_every_reported_output_shape() {
    // Issue #1032: `output_tokens`/`completion_tokens` are never billed for an
    // embedding, and only the raw input and an explicit upstream total survive.
    for usage in [
        r#"{"prompt_tokens":4,"output_tokens":3,"total_tokens":7}"#,
        r#"{"prompt_tokens":4,"completion_tokens":3,"output_tokens":5,"total_tokens":9}"#,
        r#"{"prompt_tokens":4,"completion_tokens":3}"#,
    ] {
        let captured = embedding_capture(usage);
        assert_eq!(captured.output_tokens, Some(0), "usage {usage}");
        assert_eq!(captured.input_tokens, Some(4), "usage {usage}");
    }
    // An explicit upstream total is preserved; without one the total is the
    // actual input, never a chat-style input+completion arithmetic total.
    assert_eq!(
        embedding_capture(r#"{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}"#)
            .total_tokens,
        Some(7)
    );
    assert_eq!(
        embedding_capture(r#"{"prompt_tokens":4,"completion_tokens":3}"#).total_tokens,
        Some(4)
    );
    // Without a reported input there is no billable embedding usage: an
    // output/completion value never becomes a meter and no input is inferred.
    let unknown = embedding_capture(r#"{"completion_tokens":3,"output_tokens":3}"#);
    assert_eq!(unknown.input_tokens, None);
    assert_eq!(unknown.output_tokens, None);
    assert_eq!(unknown.total_tokens, None);
    assert!(db::NormalizedBillingUsage::from_usage(&unknown).is_none());
    // An explicit total without input stays factual metadata with no output.
    let total_only = embedding_capture(r#"{"total_tokens":9}"#);
    assert_eq!(total_only.input_tokens, None);
    assert_eq!(total_only.output_tokens, None);
    assert_eq!(total_only.total_tokens, Some(9));
    // A response with no usage object at all stays unknown.
    let mut capture = prompt_ferry::usage::UsageCapture::new(false, None);
    capture.set_embedding_response();
    capture.observe_chunk(br#"{"object":"list","data":[]}"#);
    capture.finish();
    assert_eq!(capture.usage.input_tokens, None);
    assert_eq!(capture.usage.total_tokens, None);
}

#[test]
fn embeddings_reject_every_chat_generation_path() {
    // Issue #1032: a stored chat option (here self_summarize) never activates a
    // chat pipeline for an embeddings target.
    for path in [
        "/v1/chat/completions",
        "/v1/responses",
        "/v1/responses/compact",
        "/v1/messages",
    ] {
        let result = prompt_ferry::upstream_adapter::prepare_upstream_request_with_compact(
            path,
            br#"{"model":"m","input":"hello"}"#,
            NativeApi::Embeddings,
            false,
            None,
            CompactMode::SelfSummarize,
        );
        assert!(result.is_err(), "{path} must be rejected, got {result:?}");
    }
    // The chat protocols keep their self_summarize compact behavior.
    let chat = prompt_ferry::upstream_adapter::prepare_upstream_request_with_compact(
        "/v1/responses/compact",
        br#"{"model":"m","input":"hello"}"#,
        NativeApi::Chat,
        false,
        None,
        CompactMode::SelfSummarize,
    )
    .expect("chat compact self_summarize stays available");
    assert_eq!(
        chat.response_adapter,
        prompt_ferry::upstream_adapter::ResponseAdapter::SelfSummarizeLocal
    );
}

#[path = "support/embedding_capability_fixtures.rs"]
mod fixtures;
use fixtures::{insert_endpoint, insert_rule, insert_target, isolated_pool};

#[tokio::test]
async fn the_embeddings_protocol_and_dimension_round_trip_through_the_repository() {
    let pool = isolated_pool().await;
    let mut transaction = pool.begin().await.expect("begin fixture transaction");

    let endpoint_id = Uuid::new_v4();
    insert_endpoint(
        &mut transaction,
        endpoint_id,
        "embedding-endpoint",
        NativeApi::Embeddings.as_str(),
    )
    .await;
    let rule_id = Uuid::new_v4();
    insert_rule(&mut transaction, rule_id, endpoint_id, "qwen/*").await;
    let target_id = Uuid::new_v4();
    insert_target(
        &mut transaction,
        target_id,
        rule_id,
        endpoint_id,
        0,
        Some(1536),
    )
    .await
    .expect("a positive dimension is accepted");
    // A target without a dimension inherits the caller's dimensions.
    let inherited_rule_id = Uuid::new_v4();
    insert_rule(
        &mut transaction,
        inherited_rule_id,
        endpoint_id,
        "inherit/*",
    )
    .await;
    let inherited_target_id = Uuid::new_v4();
    insert_target(
        &mut transaction,
        inherited_target_id,
        inherited_rule_id,
        endpoint_id,
        0,
        None,
    )
    .await
    .expect("a NULL dimension is accepted");
    transaction.commit().await.expect("commit fixture");

    // The rule read path must carry the dimension and the resolved protocol.
    let rule = db::get_model_endpoint_rule(&pool, rule_id)
        .await
        .expect("read the rule")
        .expect("the rule exists");
    assert_eq!(rule.targets.len(), 1);
    assert_eq!(
        rule.targets[0].embedding_dimensions_override,
        Some(1536),
        "the configured dimension round-trips through the read path"
    );
    assert_eq!(
        rule.targets[0].native_api,
        NativeApi::Embeddings,
        "the explicit embeddings protocol round-trips"
    );
    assert_eq!(
        rule.targets[0].target_id, target_id,
        "the read path returns the fixture target"
    );
    let inherited = db::get_model_endpoint_rule(&pool, inherited_rule_id)
        .await
        .expect("read the inherited rule")
        .expect("the inherited rule exists");
    assert_eq!(
        inherited.targets[0].embedding_dimensions_override, None,
        "a target without a dimension inherits"
    );

    // The candidate (routing) read path must copy the same value.
    let candidate = db::get_model_route_candidate(&pool, rule_id)
        .await
        .expect("read the candidate")
        .expect("the candidate exists");
    assert_eq!(candidate.targets.len(), 1);
    assert_eq!(
        candidate.targets[0].embedding_dimensions_override,
        Some(1536),
        "the candidate materialization copies the target dimension"
    );
    assert_eq!(
        candidate.targets[0].native_api,
        NativeApi::Embeddings,
        "the candidate carries the resolved embeddings protocol"
    );

    // The list read path must agree.
    let rules = db::list_model_endpoint_rules(&pool)
        .await
        .expect("list the rules");
    let listed = rules
        .iter()
        .find(|rule| rule.rule_id == rule_id)
        .expect("the fixture rule is listed");
    assert_eq!(listed.targets[0].embedding_dimensions_override, Some(1536));

    // Clean up the fixture rows (the test target is the shared dedicated
    // database, not a private schema).
    db::delete_model_endpoint_rule(&pool, rule_id)
        .await
        .expect("delete the fixture rule");
    db::delete_model_endpoint_rule(&pool, inherited_rule_id)
        .await
        .expect("delete the inherited fixture rule");
    let _ = db::delete_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn the_schema_rejects_a_non_positive_dimension() {
    let pool = isolated_pool().await;

    let endpoint_id = Uuid::new_v4();
    let setup = pool.begin().await.expect("begin fixture transaction");
    let mut setup = setup;
    insert_endpoint(
        &mut setup,
        endpoint_id,
        "embedding-check-endpoint",
        NativeApi::Embeddings.as_str(),
    )
    .await;
    let rule_id = Uuid::new_v4();
    insert_rule(&mut setup, rule_id, endpoint_id, "check/*").await;
    setup.commit().await.expect("commit fixture");

    // Each invalid insert needs its own transaction: PostgreSQL aborts the
    // current transaction after a failed statement, so the second attempt
    // could not run in the same block.
    for invalid in [0_i32, -1] {
        let mut transaction = pool.begin().await.expect("begin fixture transaction");
        let error = insert_target(
            &mut transaction,
            Uuid::new_v4(),
            rule_id,
            endpoint_id,
            0,
            Some(invalid),
        )
        .await
        .expect_err("a non-positive dimension must be rejected");
        assert!(
            error
                .to_string()
                .contains("ck_model_route_targets_embedding_dimensions_override"),
            "unexpected error for {invalid}: {error}"
        );
        transaction.rollback().await.expect("rollback fixture");
    }

    db::delete_model_endpoint_rule(&pool, rule_id)
        .await
        .expect("delete the fixture rule");
    let _ = db::delete_endpoint(&pool, endpoint_id).await;
}

#[tokio::test]
async fn a_rule_written_through_the_repository_keeps_its_dimension() {
    let pool = isolated_pool().await;
    let mut transaction = pool.begin().await.expect("begin fixture transaction");

    let endpoint_id = Uuid::new_v4();
    insert_endpoint(
        &mut transaction,
        endpoint_id,
        "embedding-write-endpoint",
        NativeApi::Embeddings.as_str(),
    )
    .await;
    transaction.commit().await.expect("commit fixture");

    let input = ModelEndpointRuleCreate {
        scope: "admin".to_string(),
        owner_user_id: None,
        model_pattern: format!("write-{}", Uuid::new_v4()),
        routing_strategy: ModelRouteRoutingStrategy::ResponsesSessionAffinity,
        enabled: true,
        targets: vec![ModelRouteTargetCreate {
            endpoint_id,
            enabled: true,
            upstream_model: Some("qwen/qwen3-embedding-8b".to_string()),
            native_api: NativeApi::Embeddings,
            proxy_url_override: None,
            active_windows: None,
            dev_system_normalize: false,
            thinking_downgrade_enabled: false,
            thinking_effort_override: None,
            compact_mode: CompactMode::Passthrough,
            service_tier: None,
            embedding_dimensions_override: Some(768),
        }],
    };
    let created = db::create_model_endpoint_rule(&pool, input)
        .await
        .expect("create the rule");
    assert_eq!(
        created.targets[0].embedding_dimensions_override,
        Some(768),
        "the write path persists the dimension"
    );

    db::delete_model_endpoint_rule(&pool, created.rule_id)
        .await
        .expect("delete the fixture rule");
    let _ = db::delete_endpoint(&pool, endpoint_id).await;
}

#[test]
fn a_non_positive_dimension_is_rejected_and_never_clamped() {
    assert_eq!(
        normalize_embedding_dimensions_override(None).expect("inherit"),
        None
    );
    assert_eq!(
        normalize_embedding_dimensions_override(Some(1)).expect("smallest positive"),
        Some(1)
    );
    assert_eq!(
        normalize_embedding_dimensions_override(Some(2147483647)).expect("i32 maximum"),
        Some(2147483647)
    );
    for invalid in [0, -1, i32::MIN] {
        assert!(
            normalize_embedding_dimensions_override(Some(invalid)).is_err(),
            "{invalid} must be rejected"
        );
    }
}

#[test]
fn the_admin_request_distinguishes_omitted_null_and_value() {
    let omitted: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
        "endpoint_id": "00000000-0000-0000-0000-000000000000"
    }))
    .expect("an omitted field deserializes");
    assert_eq!(
        omitted.embedding_dimensions_override, None,
        "an omitted field keeps the stored value"
    );

    let cleared: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
        "endpoint_id": "00000000-0000-0000-0000-000000000000",
        "embedding_dimensions_override": null
    }))
    .expect("an explicit null deserializes");
    assert_eq!(
        cleared.embedding_dimensions_override,
        Some(None),
        "an explicit null clears the stored value"
    );

    let set: ModelRouteTargetRequest = serde_json::from_value(serde_json::json!({
        "endpoint_id": "00000000-0000-0000-0000-000000000000",
        "embedding_dimensions_override": 1536
    }))
    .expect("an integer deserializes");
    assert_eq!(
        set.embedding_dimensions_override,
        Some(Some(1536)),
        "an integer sets the forced dimension"
    );
}

#[test]
fn a_legacy_target_payload_without_the_field_stays_compatible() {
    // The standalone archive target snapshot is the compatibility contract:
    // a payload written before issue #1032 has no dimension field at all and
    // must deserialize as inherit instead of failing.
    let snapshot: db::config_repository::ModelRouteTargetSnapshot =
        serde_json::from_value(serde_json::json!({
            "target_id": "00000000-0000-0000-0000-000000000004",
            "endpoint_id": "00000000-0000-0000-0000-000000000005",
            "position": 0,
            "enabled": true,
            "upstream_model": null,
            "native_api": "auto",
            "proxy_url_override": null,
            "active_windows": null,
            "dev_system_normalize": false,
            "thinking_downgrade_enabled": false,
            "thinking_effort_override": null,
            "compact_mode": "passthrough",
            "service_tier": null
        }))
        .expect("a legacy archive target deserializes");
    assert_eq!(
        snapshot.embedding_dimensions_override, None,
        "a legacy archive target inherits the caller's dimensions"
    );

    // The same payload with the field set round-trips.
    let current: db::config_repository::ModelRouteTargetSnapshot =
        serde_json::from_value(serde_json::json!({
            "target_id": "00000000-0000-0000-0000-000000000006",
            "endpoint_id": "00000000-0000-0000-0000-000000000007",
            "position": 0,
            "enabled": true,
            "upstream_model": null,
            "native_api": "embeddings",
            "proxy_url_override": null,
            "active_windows": null,
            "dev_system_normalize": false,
            "thinking_downgrade_enabled": false,
            "thinking_effort_override": null,
            "compact_mode": "passthrough",
            "service_tier": null,
            "embedding_dimensions_override": 1536
        }))
        .expect("a current archive target deserializes");
    assert_eq!(current.embedding_dimensions_override, Some(1536));
}

#[test]
fn the_embeddings_protocol_names_its_wire_value_and_path() {
    assert_eq!(NativeApi::Embeddings.as_str(), "embeddings");
    assert_eq!(NativeApi::Embeddings.path(), "/v1/embeddings");
    // The protocol is not a chat conversion: the chat path and the embeddings
    // path stay distinct.
    assert_ne!(NativeApi::Embeddings.path(), NativeApi::Chat.path());
}

#[test]
fn embedding_usage_is_priced_on_input_only() {
    // Issue #1032: the canonical embeddings usage (input only, output zero)
    // reaches the existing billing pipeline with no output tokens, so only the
    // input meter contributes; the dimension count is never converted into
    // tokens.
    use prompt_ferry::db::NormalizedBillingUsage;
    use prompt_ferry::usage::TokenUsage;

    let usage = TokenUsage {
        input_tokens: Some(4096),
        output_tokens: Some(0),
        total_tokens: Some(4096),
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    let billed = NormalizedBillingUsage::from_usage(&usage).expect("billed usage");
    assert_eq!(billed.input_tokens, 4096);
    assert_eq!(billed.output_tokens, 0, "embeddings never bill output");
    assert_eq!(billed.cache_read_tokens, 0);
    assert_eq!(billed.cache_write_tokens, 0);

    // A response that reports no upstream usage stays unbilled instead of
    // being guessed from the vector length.
    let unknown = TokenUsage {
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        cached_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    assert!(NormalizedBillingUsage::from_usage(&unknown).is_none());
}
