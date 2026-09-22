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
        UpstreamRedactionSession::current(RestoreState::new(artifact.session).expect("state")),
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
    let session =
        UpstreamRedactionSession::current(RestoreState::new(artifact.session).expect("state"));
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
fn passes_through_truncated_token_at_done_event() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    let partial = &token[..token.len() - 2];
    let chunk = format!(
        "data: {{\"type\":\"response.output_text.delta\",\"item_id\":\"msg\",\"delta\":{:?}}}\n\n",
        partial
    );
    filter.push_chunk(chunk.as_bytes()).expect("partial");

    let output = filter
        .push_chunk(b"data: [DONE]\n\n")
        .expect("truncated token is pass-through");
    assert_eq!(output.len(), 2);
    assert_eq!(data_json(&output[0])["delta"], partial);
    assert_eq!(output[1], b"data: [DONE]\n\n");
}

#[test]
fn passes_through_malformed_tokens_without_aborting_stream() {
    let (session, _) = session("a.example.com");
    let malformed = "before [[RDX:v2:...]] after";
    let mut filter = SseRestoreFilter::new(&session);
    let event = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": malformed}}]
    });

    let output = filter
        .push_chunk(format!("data: {event}\n\n").as_bytes())
        .expect("malformed token is pass-through");

    assert_eq!(
        data_json(&output[0])["choices"][0]["delta"]["content"],
        malformed
    );
}

#[test]
fn preserves_valid_token_when_invalid_tokens_are_mixed_in_stream() {
    let (session, token) = session("a.example.com");
    let mut invalid_checksum = token.clone();
    let checksum_index = invalid_checksum.len() - 3;
    let replacement = if invalid_checksum.as_bytes()[checksum_index] == b'0' {
        '1'
    } else {
        '0'
    };
    invalid_checksum.replace_range(checksum_index..checksum_index + 1, &replacement.to_string());
    let content = format!(
        "valid {token} checksum {invalid_checksum} unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
    );
    let event = serde_json::json!({
        "choices": [{"index": 0, "delta": {"content": content}}]
    });
    let mut filter = SseRestoreFilter::new(&session);

    let output = filter
        .push_chunk(format!("data: {event}\n\n").as_bytes())
        .expect("mixed token restore");

    assert_eq!(
        data_json(&output[0])["choices"][0]["delta"]["content"],
        format!(
            "valid a.example.com checksum {invalid_checksum} unknown [[RDX:v2:scope:unknown:001:deadbeef]]"
        )
    );
}

#[test]
fn invalid_sse_json_still_fails() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);

    let err = filter
        .push_chunk(b"data: {not-json}\n\n")
        .expect_err("invalid SSE JSON");
    assert!(err.to_string().contains("invalid SSE data JSON"));
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

/// Captures `restore_unhandled_field` warning field values for assertions.
#[derive(Default)]
struct WarningCapture {
    events: std::sync::Arc<std::sync::Mutex<Vec<std::collections::BTreeMap<String, String>>>>,
}

impl WarningCapture {
    fn run<R>(&self, body: impl FnOnce() -> R) -> R {
        let subscriber = CaptureSubscriber {
            events: self.events.clone(),
        };
        tracing::subscriber::with_default(subscriber, body)
    }

    fn unhandled(&self) -> Vec<std::collections::BTreeMap<String, String>> {
        self.events
            .lock()
            .expect("warning capture lock")
            .iter()
            .filter(|fields| {
                fields.get("event").map(String::as_str) == Some("restore_unhandled_field")
            })
            .cloned()
            .collect()
    }
}

struct CaptureSubscriber {
    events: std::sync::Arc<std::sync::Mutex<Vec<std::collections::BTreeMap<String, String>>>>,
}

impl tracing::Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("warning capture lock")
            .push(visitor.fields);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

#[derive(Default)]
struct FieldVisitor {
    fields: std::collections::BTreeMap<String, String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

#[test]
fn restores_reasoning_summary_token_split_across_events() {
    let (session, token) = session("a.example.com");
    let split = token.len() / 2;
    let mut filter = SseRestoreFilter::new_responses(&session);
    let first = format!(
        "data: {{\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"delta\":{:?}}}\n\n",
        &token[..split]
    );
    let second = format!(
        "data: {{\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"delta\":{:?}}}\n\n",
        &token[split..]
    );

    let first = filter.push_chunk(first.as_bytes()).expect("first");
    let second = filter.push_chunk(second.as_bytes()).expect("second");
    filter.finish().expect("finish");

    assert_eq!(data_json(&first[0])["delta"], "");
    assert_eq!(data_json(&second[0])["delta"], "a.example.com");
}

#[test]
fn restores_reasoning_summary_part_text() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let added = format!(
        "data: {{\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"part\":{{\"type\":\"summary_text\",\"text\":{token:?}}}}}\n\n"
    );
    let done = format!(
        "data: {{\"type\":\"response.reasoning_summary_part.done\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"part\":{{\"type\":\"summary_text\",\"text\":{token:?}}}}}\n\n"
    );

    let added = filter.push_chunk(added.as_bytes()).expect("added");
    let done = filter.push_chunk(done.as_bytes()).expect("done");
    filter.finish().expect("finish");

    assert_eq!(data_json(&added[0])["part"]["text"], "a.example.com");
    assert_eq!(data_json(&done[0])["part"]["text"], "a.example.com");
}

#[test]
fn restores_reasoning_summary_text_done() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = format!(
        "data: {{\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"text\":{token:?}}}\n\n"
    );

    let output = filter.push_chunk(event.as_bytes()).expect("done");
    filter.finish().expect("finish");

    assert_eq!(data_json(&output[0])["text"], "a.example.com");
}

#[test]
fn restores_terminal_snapshot_reasoning_summary_and_output_text() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let completed = format!(
        "data: {{\"type\":\"response.completed\",\"response\":{{\"output\":[{{\"id\":\"r1\",\"type\":\"reasoning\",\"summary\":[{{\"type\":\"summary_text\",\"text\":{token:?}}}],\"content\":[{{\"type\":\"reasoning_text\",\"text\":{token:?}}}]}},{{\"id\":\"m1\",\"type\":\"message\",\"content\":[{{\"type\":\"output_text\",\"text\":{token:?}}}]}}]}}}}\n\n"
    );

    let output = filter
        .push_chunk(completed.as_bytes())
        .expect("completed snapshot");
    filter.finish().expect("finish");

    let value = data_json(&output[0]);
    let output_items = value["response"]["output"]
        .as_array()
        .expect("output items");
    assert_eq!(output_items[0]["summary"][0]["text"], "a.example.com");
    assert_eq!(output_items[0]["content"][0]["text"], "a.example.com");
    assert_eq!(output_items[1]["content"][0]["text"], "a.example.com");
}

#[test]
fn restores_incomplete_and_failed_snapshots() {
    let (session, token) = session("a.example.com");
    for event_type in ["response.incomplete", "response.failed"] {
        let mut filter = SseRestoreFilter::new_responses(&session);
        let event = format!(
            "data: {{\"type\":\"{event_type}\",\"response\":{{\"output\":[{{\"id\":\"r1\",\"type\":\"reasoning\",\"summary\":[{{\"type\":\"summary_text\",\"text\":{token:?}}}]}}]}}}}\n\n"
        );
        let output = filter.push_chunk(event.as_bytes()).expect("snapshot");
        assert_eq!(output.len(), 1, "{event_type} snapshot must pass through");
        assert_eq!(
            data_json(&output[0])["response"]["output"][0]["summary"][0]["text"],
            "a.example.com",
            "{event_type} summary must be restored"
        );
    }
}

#[test]
fn restores_terminal_snapshot_function_call_arguments() {
    let (session, token) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let completed = format!(
        "data: {{\"type\":\"response.completed\",\"response\":{{\"output\":[{{\"id\":\"fc1\",\"type\":\"function_call\",\"call_id\":\"fc1\",\"name\":\"lookup\",\"arguments\":{token:?}}},{{\"id\":\"fc1\",\"type\":\"function_call_output\",\"call_id\":\"fc1\",\"output\":{token:?}}}]}}}}\n\n"
    );

    let output = filter.push_chunk(completed.as_bytes()).expect("snapshot");
    filter.finish().expect("finish");

    let value = data_json(&output[0]);
    assert_eq!(value["response"]["output"][0]["arguments"], "a.example.com");
    assert_eq!(value["response"]["output"][1]["output"], "a.example.com");
}

#[test]
fn token_free_terminal_event_passes_through_byte_identical() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = "event: response.completed\r\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"status\":\"completed\",\"output\":[]}}\r\n\r\n";

    let output = filter.push_chunk(event.as_bytes()).expect("terminal");

    assert_eq!(output.len(), 1);
    assert_eq!(output[0], event.as_bytes());
}

#[test]
fn passes_through_terminal_snapshot_token_outside_text_fields_and_warns() {
    let (session, token) = session("a.example.com");
    let capture = WarningCapture::default();
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = format!(
        "data: {{\"type\":\"response.completed\",\"response\":{{\"id\":{token:?},\"output\":[]}}}}\n\n"
    );

    let output = capture.run(|| filter.push_chunk(event.as_bytes()).expect("snapshot"));

    assert_eq!(data_json(&output[0])["response"]["id"], token);
    let warnings = capture.unhandled();
    assert_eq!(warnings.len(), 1, "expected one unhandled-field warning");
    assert_eq!(
        warnings[0].get("event_type").map(String::as_str),
        Some("response.completed")
    );
    assert_eq!(
        warnings[0].get("field_path").map(String::as_str),
        Some("/response/id")
    );
}

#[test]
fn passes_through_token_in_unknown_event_and_warns() {
    let (session, token) = session("a.example.com");
    let capture = WarningCapture::default();
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = format!(
        "data: {{\"type\":\"response.unknown.event\",\"payload\":{{\"note\":{token:?}}}}}\n\n"
    );

    let output = capture.run(|| filter.push_chunk(event.as_bytes()).expect("pass-through"));
    filter.finish().expect("finish");

    assert_eq!(data_json(&output[0])["payload"]["note"], token);
    let warnings = capture.unhandled();
    assert_eq!(warnings.len(), 1, "expected one unhandled-field warning");
    assert_eq!(
        warnings[0].get("event_type").map(String::as_str),
        Some("response.unknown.event")
    );
    assert_eq!(
        warnings[0].get("field_path").map(String::as_str),
        Some("/payload/note")
    );
    assert_eq!(
        warnings[0].get("token_prefix").map(String::as_str),
        Some(&token[.."[[RDX:v2:".len() + 8])
    );
}

#[test]
fn handled_events_do_not_warn() {
    let (session, token) = session("a.example.com");
    let capture = WarningCapture::default();
    let mut filter = SseRestoreFilter::new_responses(&session);
    let event = format!(
        "data: {{\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"r1\",\"output_index\":0,\"summary_index\":0,\"delta\":{token:?}}}\n\n"
    );

    let output = capture.run(|| filter.push_chunk(event.as_bytes()).expect("restore"));
    filter.finish().expect("finish");

    assert_eq!(data_json(&output[0])["delta"], "a.example.com");
    assert!(capture.unhandled().is_empty());
}
