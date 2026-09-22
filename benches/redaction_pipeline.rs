use std::hint::black_box;
use std::time::Instant;

use anyhow::Result;
use prompt_ferry_redact::{RedactionConfig, apply_config};
use prompt_ferry_redact_upstream::UpstreamRedactionProcessor;
use redactor::RedactionRules;
use serde_json::{Value, json};

fn timed(label: &str, iterations: usize, mut run: impl FnMut()) {
    let started = Instant::now();
    for _ in 0..iterations {
        run();
    }
    let elapsed = started.elapsed();
    println!(
        "{label}: {iterations} iterations in {elapsed:?} ({:.2} us/iter)",
        elapsed.as_secs_f64() * 1_000_000.0 / iterations as f64
    );
}

/// Local mirror of `worker::runtime::json_walker::walk_json_strings`, which is
/// `pub(crate)`: benches cannot import it, so replicate the same traversal to
/// keep the timed path identical to `redact_ai_request_json`.
fn walk_json_strings(
    value: &mut Value,
    visitor: &mut impl FnMut(&str, Option<&str>, Option<&str>, &str) -> Result<Option<String>>,
) -> Result<()> {
    let mut json_path = String::new();
    walk_value(value, &mut json_path, None, None, visitor)
}

fn walk_value(
    value: &mut Value,
    json_path: &mut String,
    field_name: Option<&str>,
    object_type: Option<&str>,
    visitor: &mut impl FnMut(&str, Option<&str>, Option<&str>, &str) -> Result<Option<String>>,
) -> Result<()> {
    match value {
        Value::String(text) => {
            if let Some(replacement) = visitor(json_path, field_name, object_type, text)? {
                *text = replacement;
            }
        }
        Value::Array(items) => {
            let base_len = json_path.len();
            for (index, item) in items.iter_mut().enumerate() {
                json_path.push('/');
                json_path.push_str(&index.to_string());
                walk_value(item, json_path, None, None, visitor)?;
                json_path.truncate(base_len);
            }
        }
        Value::Object(object) => {
            let object_type = object
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_owned);
            let base_len = json_path.len();
            for (key, child) in object.iter_mut() {
                json_path.push('/');
                json_path.push_str(key);
                walk_value(child, json_path, Some(key), object_type.as_deref(), visitor)?;
                json_path.truncate(base_len);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Mirrors `should_process_chat_string_field` for the `/v1/chat/completions`
/// path used by the reasoning and large-payload scenarios.
fn should_process_chat_string_field(json_path: &str, key: &str) -> bool {
    let is_reasoning_field = matches!(
        key,
        "reasoning_content"
            | "reasoning"
            | "reasoning_text"
            | "reasoning_details"
            | "thinking"
            | "chain_of_thought"
            | "summary"
    );
    (key == "content" && json_path.contains("/messages/"))
        || (key == "text" && json_path.contains("/messages/") && json_path.contains("/content/"))
        || (key == "arguments"
            && (json_path.ends_with("/function")
                || json_path.ends_with("/function_call")
                || json_path.contains("/tool_calls/")))
        || (key == "output" && json_path.contains("/messages/"))
        || is_reasoning_field
        || (key == "text" && json_path.contains("/reasoning_details/"))
}

fn main() {
    apply_config(&RedactionConfig {
        enabled: true,
        rules: RedactionRules {
            domain: true,
            ..RedactionRules::default()
        },
        custom_strings: Vec::new(),
    })
    .expect("redaction config");

    let fields = (0..512)
        .map(|index| format!("service-{index}.example.com"))
        .collect::<Vec<_>>();
    timed("prompt_ferry_session_128_fields", 100, || {
        let mut processor = UpstreamRedactionProcessor::new(None, Some("bench"), None)
            .expect("redaction processor");
        let redacted = fields
            .iter()
            .take(128)
            .map(|field| {
                processor
                    .redact_fragment(field, redactor::InputKind::Text)
                    .expect("redact")
            })
            .collect::<Vec<_>>();
        black_box(
            processor
                .finish_state(&fields[..128].join("\n"), &redacted.join("\n"))
                .expect("finish state"),
        );
    });

    let mut processor =
        UpstreamRedactionProcessor::new(None, Some("bench"), None).expect("redaction processor");
    let redacted = fields
        .iter()
        .map(|field| {
            processor
                .redact_fragment(field, redactor::InputKind::Text)
                .expect("redact")
        })
        .collect::<Vec<_>>();
    let session = processor
        .finish_state(&fields.join("\n"), &redacted.join("\n"))
        .expect("finish state")
        .expect("session");
    timed("prompt_ferry_restore_512_entries_128_fields", 100, || {
        let context = session
            .restore_state
            .restore_context()
            .expect("restore context");
        for field in redacted.iter().take(128) {
            black_box(context.restore_text(black_box(field)));
        }
    });

    // 2000 turn-advances (each turn redacts one fresh domain and advances the
    // prior restore state), then time one incremental redact_fragment on top.
    // Setup advances the raw session exactly like UpstreamRedactionProcessor
    // (finish_session -> RestoreState::advance) but skips the exceeds_budget
    // gate: a full 2000-entry state cannot fit the 256KiB envelope, so the
    // gate would abort long before 2000 turns.
    let turns = 2000;
    let turn_fields = (0..turns)
        .map(|turn| format!("turn-{turn}.example.com"))
        .collect::<Vec<_>>();
    let redactor_snapshot =
        prompt_ferry_redact::redactor_snapshot_for_user(None).expect("redactor snapshot");
    let mut session_redactor =
        redactor::SessionRedactor::with_prior_session(None, Some("bench-long"))
            .expect("long session redactor");
    let mut state: Option<redactor::RestoreState> = None;
    for field in &turn_fields {
        let redacted = session_redactor
            .redact_fragment_with_input_kind(&redactor_snapshot, field, redactor::InputKind::Text)
            .expect("advance redact");
        let session = session_redactor.finish_session(field, &redacted, redactor_snapshot.policy());
        state = Some(
            match &state {
                Some(prior) => prior.advance(session),
                None => redactor::RestoreState::new(session),
            }
            .expect("advance state"),
        );
    }
    let state = state.expect("advance state");
    assert_eq!(state.session().entries.len(), turns);
    let prior = prompt_ferry_redact_upstream::UpstreamRedactionSession::current(state);
    let mut processor = UpstreamRedactionProcessor::new(None, Some("bench-long"), Some(&prior))
        .expect("long session processor");
    let mut turn = 0usize;
    timed("prompt_ferry_long_session_advance_2000_entries", 50, || {
        let field = format!("tail-{turn}.example.com");
        turn += 1;
        let redacted = processor
            .redact_fragment(black_box(&field), redactor::InputKind::Text)
            .expect("advance redact");
        black_box(redacted);
    });
    drop(processor);

    // 2100 entries over the MAX_ENTRIES=2000 ceiling: finish_state must hit
    // the exceeds_budget -> Ok(None) degrade path.
    let overflow_fields = (0..2100)
        .map(|index| format!("budget{index}.example.com"))
        .collect::<Vec<_>>();
    timed("prompt_ferry_budget_overflow_path", 10, || {
        let mut processor = UpstreamRedactionProcessor::new(None, Some("bench-budget"), None)
            .expect("budget processor");
        let redacted = overflow_fields
            .iter()
            .map(|field| {
                processor
                    .redact_fragment(field, redactor::InputKind::Text)
                    .expect("budget redact")
            })
            .collect::<Vec<_>>();
        let finalized = processor
            .finish_state(&overflow_fields.join(" "), &redacted.join(" "))
            .expect("budget finish");
        assert!(finalized.is_none(), "2100 entries must exceed budget");
    });

    // 500 permits (MAX_PERMITS ceiling): 500 turn-advances each issuing one
    // permit, then time a single-token restore against that state. Setup uses
    // the raw SessionRedactor advance chain, as in the long-session scenario.
    let permit_turns = 500;
    let permit_fields = (0..permit_turns)
        .map(|turn| format!("permit-{turn}.example.com"))
        .collect::<Vec<_>>();
    let mut session_redactor =
        redactor::SessionRedactor::with_prior_session(None, Some("bench-permits"))
            .expect("permit redactor");
    let mut state: Option<redactor::RestoreState> = None;
    for field in &permit_fields {
        let redacted = session_redactor
            .redact_fragment_with_input_kind(&redactor_snapshot, field, redactor::InputKind::Text)
            .expect("permit redact");
        let session = session_redactor.finish_session(field, &redacted, redactor_snapshot.policy());
        state = Some(
            match &state {
                Some(prior) => prior.advance(session),
                None => redactor::RestoreState::new(session),
            }
            .expect("permit state"),
        );
    }
    let state = state.expect("permit bench state");
    assert_eq!(state.permits().len(), permit_turns);
    let permit_token = state.session().entries[permit_turns - 1].token.clone();
    timed("prompt_ferry_restore_500_permits", 100, || {
        let context = state.restore_context().expect("permit restore context");
        black_box(context.restore_text(black_box(&permit_token)));
    });

    // Chat-completions body heavy in reasoning fields: walk mirrors
    // redact_ai_request_json with the chat field filter so reasoning fields
    // redact through the same production path.
    let reasoning_message = || {
        json!({
            "role": "assistant",
            "content": "answer from server-answer.example.com",
            "reasoning_content": "thought chain about origin-plan.example.com ".repeat(8),
            "reasoning_details": [{
                "text": "detail about source-data.example.com ".repeat(4),
                "signature": "sig"
            }],
            "tool_calls": [{
                "id": "call_bench",
                "type": "function",
                "function": {
                    "name": "lookup",
                    "arguments": r#"{"host":"call-bench.example.com"}"#,
                }
            }]
        })
    };
    let reasoning_body = json!({
        "model": "gpt-bench",
        "messages": (0..50).map(|_| reasoning_message()).collect::<Vec<_>>(),
    });
    timed("prompt_ferry_reasoning_heavy_payload", 50, || {
        // Re-seed per iteration: the walk mutates the body, so reusing it would
        // measure token-stable text after the first pass.
        let mut body = reasoning_body.clone();
        let mut processor = UpstreamRedactionProcessor::new(None, Some("bench-reasoning"), None)
            .expect("reasoning processor");
        walk_json_strings(
            &mut body,
            &mut |json_path, field_name, _object_type, text| {
                let field_name = field_name.unwrap_or_default();
                if !should_process_chat_string_field(json_path, field_name) {
                    return Ok(None);
                }
                processor
                    .redact_fragment(text, redactor::InputKind::Text)
                    .map(Some)
                    .map_err(anyhow::Error::new)
            },
        )
        .expect("reasoning walk");
        black_box(body);
    });

    // ~1MB chat body end-to-end redact: parse -> walk+redact -> serialize ->
    // finish_state, mirroring redact_ai_request_json cost shape.
    let filler_template = "filler text with service-example.example.com inside. ".repeat(9);
    let large_body = json!({
        "model": "gpt-bench",
        "messages": (0..2000)
            .map(|index| {
                json!({
                    "role": "user",
                    "content": format!("{filler_template}unique-{index}.example.com"),
                })
            })
            .collect::<Vec<_>>(),
    })
    .to_string();
    assert!(large_body.len() > 1024 * 1024, "payload must exceed 1MiB");
    timed("prompt_ferry_large_payload_walk", 20, || {
        let mut value: Value = serde_json::from_str(&large_body).expect("large payload json");
        let mut processor = UpstreamRedactionProcessor::new(None, Some("bench-large"), None)
            .expect("large payload processor");
        walk_json_strings(
            &mut value,
            &mut |json_path, field_name, _object_type, text| {
                let field_name = field_name.unwrap_or_default();
                if !should_process_chat_string_field(json_path, field_name) {
                    return Ok(None);
                }
                processor
                    .redact_fragment(text, redactor::InputKind::Text)
                    .map(Some)
                    .map_err(anyhow::Error::new)
            },
        )
        .expect("large payload walk");
        let redacted_body = serde_json::to_vec(&value).expect("large payload serialize");
        let redacted_text = std::str::from_utf8(&redacted_body).expect("UTF-8 JSON");
        black_box(
            processor
                .finish_state(&large_body, redacted_text)
                .expect("large payload finish"),
        );
    });
}
