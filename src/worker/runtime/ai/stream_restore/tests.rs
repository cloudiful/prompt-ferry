use redactor::{
    CustomStringMatch, CustomStringRule, CustomStringScope, FindingKind, InputKind,
    RedactionPolicy, RedactorBuilder, RestoreState,
};
use serde_json::Value;

use super::SseRestoreFilter;
use crate::redact_upstream::UpstreamRedactionSession;
use crate::worker::runtime::error_handling::ResponsesSseTerminal;

fn session(original: &str) -> (UpstreamRedactionSession, String) {
    let redactor = RedactorBuilder::new()
        .with_redaction_policy(RedactionPolicy::default().with_kind(FindingKind::Domain, true))
        .build();
    let artifact = redactor
        .redact_artifact_with_input_kind_source_and_prior_session(
            original,
            InputKind::Text,
            None,
            None,
            Some("conversation"),
        )
        .expect("redact");
    let token = artifact.session.issued_tokens[0].clone();
    (
        UpstreamRedactionSession {
            restore_state: RestoreState::new(artifact.session).expect("state"),
        },
        token,
    )
}

fn data_json(event: &[u8]) -> Value {
    let line = std::str::from_utf8(event)
        .expect("UTF-8")
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("data line");
    serde_json::from_str(line).expect("JSON")
}

#[test]
fn restores_responses_token_split_across_events() {
    let (session, token) = session("a.example.com");
    let split = token.len() / 2;
    let mut filter = SseRestoreFilter::new(&session);
    let first = format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg\",\"output_index\":0,\"content_index\":0,\"delta\":{:?}}}\n\n",
        &token[..split]
    );
    let second = format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg\",\"output_index\":0,\"content_index\":0,\"delta\":{:?}}}\n\n",
        &token[split..]
    );

    let first = filter.push_chunk(first.as_bytes()).expect("first");
    let second = filter.push_chunk(second.as_bytes()).expect("second");
    filter.finish().expect("finish");

    assert_eq!(data_json(&first[0])["delta"], "");
    assert_eq!(data_json(&second[0])["delta"], "a.example.com");
}

#[test]
fn restores_chat_delta_and_reserializes_special_characters() {
    let original = "private \"value\"\nnext line";
    let redactor = RedactorBuilder::new()
        .with_redaction_policy(RedactionPolicy {
            custom_strings: vec![CustomStringRule {
                pattern: original.to_string(),
                match_type: CustomStringMatch::Exact,
                scope: CustomStringScope::Text,
            }],
            ..RedactionPolicy::default()
        })
        .build();
    let artifact = redactor
        .redact_artifact_with_input_kind_source_and_prior_session(
            original,
            InputKind::Text,
            None,
            None,
            Some("conversation"),
        )
        .expect("redact");
    let token = artifact.session.issued_tokens[0].clone();
    let session = UpstreamRedactionSession {
        restore_state: RestoreState::new(artifact.session).expect("state"),
    };
    let mut filter = SseRestoreFilter::new(&session);
    let event = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": token}}]
    });
    let chunk = format!("data: {event}\n\n");

    let output = filter.push_chunk(chunk.as_bytes()).expect("restore");
    filter.finish().expect("finish");

    assert_eq!(
        data_json(&output[0])["choices"][0]["delta"]["content"],
        original
    );
}

#[test]
fn rejects_truncated_token_at_done_event() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let partial = &token[..token.len() - 2];
    let chunk = format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg\",\"delta\":{:?}}}\n\n",
        partial
    );
    filter.push_chunk(chunk.as_bytes()).expect("partial");

    let err = filter
        .push_chunk(b"data: [DONE]\n\n")
        .expect_err("truncated");
    assert!(err.to_string().contains("truncated token"));
}

#[test]
fn flushes_plain_marker_prefix_before_done_event() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let chunk = concat!(
        "data: {\"type\":\"response.output_text.delta\",",
        "\"item_id\":\"msg\",\"delta\":\"tail [[\"}\n\n"
    );
    let first = filter.push_chunk(chunk.as_bytes()).expect("partial prefix");
    assert_eq!(data_json(&first[0])["delta"], "tail ");

    let done = filter.push_chunk(b"data: [DONE]\n\n").expect("done");
    assert_eq!(done.len(), 2);
    assert_eq!(data_json(&done[0])["delta"], "[[");
    assert_eq!(done[1], b"data: [DONE]\n\n");
}

#[test]
fn flushes_plain_marker_prefix_before_responses_terminal_event() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let chunk = concat!(
        "data: {\"type\":\"response.output_text.delta\",",
        "\"item_id\":\"msg\",\"delta\":\"tail [\"}\n\n"
    );
    filter.push_chunk(chunk.as_bytes()).expect("partial prefix");

    let completed = filter
        .push_chunk(b"data: {\"type\":\"response.completed\"}\n\n")
        .expect("completed");
    assert_eq!(completed.len(), 2);
    assert_eq!(data_json(&completed[0])["delta"], "[");
    assert_eq!(data_json(&completed[1])["type"], "response.completed");
    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Completed)
    );
}

#[test]
fn exposes_responses_failure_terminals_after_restore() {
    for (event_type, expected) in [
        ("response.failed", ResponsesSseTerminal::Failed),
        ("response.incomplete", ResponsesSseTerminal::Incomplete),
        ("error", ResponsesSseTerminal::Error),
    ] {
        let (session, _) = session("a.example.com");
        let mut filter = SseRestoreFilter::new_responses(&session);
        let event = format!("data: {{\"type\":\"{event_type}\"}}\n\n");
        assert_eq!(filter.push_chunk(event.as_bytes()).unwrap().len(), 1);
        assert_eq!(filter.responses_terminal(), Some(expected));
    }
}
