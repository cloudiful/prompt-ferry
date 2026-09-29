//! Phase 1 property tests for the SSE restore filter (issue #569): the
//! `redact → split at any point → restore` round trip (`p1`) and the visible
//! text of an arbitrary Responses event sequence (`p4`).

use proptest::prelude::*;
use redactor::InputKind;
use serde_json::{Value, json};

use super::stream_restore::SseRestoreFilter;
use crate::redact_test_support::domain_redaction;
use crate::redact_upstream::{UpstreamRedactionSession, redact_text_with_stateful_session};

const TOKEN_PREFIX: &str = "[[RDX:v2:";
const PAYLOAD_TAG: &str = "payload";
const DOMAIN_NAMES: &[&str] = &["alpha", "bravo", "charlie", "delta", "echo", "foxtrot"];
const FILLERS: &[&str] = &[
    "",
    "note",
    "[",
    "[[RDX:v2",
    "[[RDX:v2:unknown:001:deadbeef]]",
    "quoted \"value\"",
    "日本語",
    "line\nbreak",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    OutputDelta,
    SummaryDelta,
    ContentPart,
    OutputItem,
    Completed,
}

const FAMILIES: &[Family] = &[
    Family::OutputDelta,
    Family::SummaryDelta,
    Family::ContentPart,
    Family::OutputItem,
    Family::Completed,
];

impl Family {
    fn event_type(self) -> &'static str {
        match self {
            Self::OutputDelta => "response.output_text.delta",
            Self::SummaryDelta => "response.reasoning_summary_text.delta",
            Self::ContentPart => "response.content_part.done",
            Self::OutputItem => "response.output_item.done",
            Self::Completed => "response.completed",
        }
    }

    fn is_delta(self) -> bool {
        matches!(self, Self::OutputDelta | Self::SummaryDelta)
    }

    /// The client-visible payload text, or `None` for noise events.
    fn payload_text(self, value: &Value) -> Option<&str> {
        let pointer = match self {
            Self::OutputDelta | Self::SummaryDelta => "/delta",
            Self::ContentPart => "/part/text",
            Self::OutputItem => "/item/content/0/text",
            Self::Completed => "/response/output/0/content/0/text",
        };
        let tagged = match self {
            Self::OutputDelta | Self::SummaryDelta | Self::ContentPart => {
                value.get("item_id").and_then(Value::as_str) == Some(PAYLOAD_TAG)
            }
            Self::OutputItem => value.get("output_index").and_then(Value::as_u64) == Some(0),
            Self::Completed => {
                value.pointer("/response/id").and_then(Value::as_str) == Some(PAYLOAD_TAG)
            }
        };
        if !tagged {
            return None;
        }
        value.pointer(pointer).and_then(Value::as_str)
    }
}

fn domain(name: &str) -> String {
    format!("{name}.example.com")
}

/// Domains sit alone on their own lines so filler tokens (brackets, quotes,
/// marker look-alikes) cannot trip the redactor's code-context validator; every
/// case still mints at least one real-scope token.
fn arbitrary_text() -> impl Strategy<Value = String> {
    (
        prop::sample::subsequence(DOMAIN_NAMES.to_vec(), 1..=3),
        prop::sample::select(FILLERS),
        prop::sample::select(FILLERS),
        prop::sample::select(FILLERS),
    )
        .prop_map(|(names, lead, joiner, tail)| {
            let mut text = String::from(lead);
            text.push('\n');
            for (index, name) in names.iter().enumerate() {
                if index > 0 {
                    text.push_str(joiner);
                    text.push('\n');
                }
                text.push_str(&domain(name));
                text.push('\n');
            }
            text.push_str(tail);
            text
        })
}

fn redact(text: &str, external_id: &str) -> (UpstreamRedactionSession, String) {
    let result =
        redact_text_with_stateful_session(text, InputKind::Text, None, Some(external_id), None)
            .expect("redact");
    (
        result.session.expect("redaction minted no session"),
        result.redacted_text,
    )
}

fn event_data(event: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(event).ok()?;
    let payload = text.lines().find_map(|line| line.strip_prefix("data: "))?;
    serde_json::from_str(payload).ok()
}

fn visible_text(events: &[Vec<u8>], family: Family) -> String {
    let mut visible = String::new();
    for event in events {
        let Some(value) = event_data(event) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) == Some(family.event_type())
            && let Some(text) = family.payload_text(&value)
        {
            visible.push_str(text);
        }
    }
    visible
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Split at the given percentages, clamped to char boundaries and deduped, so
/// the pieces concatenate back to the whole text.
fn split_parts(text: &str, ratios: &[u32]) -> Vec<String> {
    let mut bounds: Vec<usize> = ratios
        .iter()
        .map(|ratio| floor_char_boundary(text, text.len() * (*ratio as usize) / 100))
        .collect();
    bounds.extend([0, text.len()]);
    bounds.sort_unstable();
    bounds.dedup();
    bounds
        .windows(2)
        .map(|window| text[window[0]..window[1]].to_string())
        .collect()
}

fn payload_events(family: Family, parts: &[String]) -> Vec<Value> {
    let text = parts.concat();
    match family {
        Family::OutputDelta | Family::SummaryDelta => parts.iter().map(|part| json!({"type": family.event_type(), "item_id": PAYLOAD_TAG, "output_index": 0, "content_index": 0, "summary_index": 0, "delta": part})).collect(),
        Family::ContentPart => vec![json!({"type": family.event_type(), "item_id": PAYLOAD_TAG, "output_index": 0, "content_index": 0, "part": {"type": "output_text", "text": text}})],
        Family::OutputItem => vec![json!({"type": family.event_type(), "output_index": 0, "item": {"id": PAYLOAD_TAG, "type": "message", "role": "assistant", "content": [{"type": "output_text", "text": text}]}})],
        Family::Completed => vec![json!({"type": family.event_type(), "response": {"id": PAYLOAD_TAG, "output": [{"id": PAYLOAD_TAG, "type": "message", "content": [{"type": "output_text", "text": text}]}]}})],
    }
}

/// Token-free events from families other than the payload's; they must flow
/// through without contributing or corrupting visible text.
fn noise_event(kind: u8) -> Value {
    match kind {
        0 => json!({"type": "response.unknown_family_noise", "payload": {"note": "noise"}}),
        1 => {
            json!({"type": "response.content_part.done", "item_id": "noise", "output_index": 99, "content_index": 0, "part": {"type": "output_text", "text": "noise-part"}})
        }
        _ => {
            json!({"type": "response.output_item.done", "output_index": 99, "item": {"id": "noise", "type": "message", "content": [{"type": "output_text", "text": "noise-item"}]}})
        }
    }
}

fn terminal_noise() -> Value {
    json!({"type": "response.completed", "response": {"id": "noise", "output": []}})
}

fn push_events(filter: &mut SseRestoreFilter<'_>, events: &[Value]) -> Vec<Vec<u8>> {
    let mut output = Vec::new();
    for event in events {
        let chunk = format!("data: {event}\n\n");
        output.extend(filter.push_chunk(chunk.as_bytes()).expect("push chunk"));
    }
    output.extend(filter.finish().expect("finish"));
    output
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn p1_redacted_text_round_trips_across_arbitrary_splits(
        (text, ratios) in (arbitrary_text(), prop::collection::vec(0u32..=100, 1..=3))
    ) {
        let _guard = domain_redaction();
        let (session, redacted) = redact(&text, "conv-p1");
        prop_assert!(redacted.contains(TOKEN_PREFIX), "no token minted for {text:?}");

        let parts = split_parts(&redacted, &ratios);
        let mut filter = SseRestoreFilter::new_responses(&session);
        let output = push_events(&mut filter, &payload_events(Family::OutputDelta, &parts));

        prop_assert_eq!(visible_text(&output, Family::OutputDelta), text);
    }

    #[test]
    fn p4_event_sequence_visible_text_round_trips(
        (text, ratios, family_index, noise) in (
            arbitrary_text(),
            prop::collection::vec(0u32..=100, 1..=3),
            0usize..FAMILIES.len(),
            prop::collection::vec(0u8..3, 0..=3),
        )
    ) {
        let _guard = domain_redaction();
        let family = FAMILIES[family_index];
        let (session, redacted) = redact(&text, "conv-p4");
        prop_assert!(redacted.contains(TOKEN_PREFIX), "no token minted for {text:?}");

        let parts = if family.is_delta() {
            split_parts(&redacted, &ratios)
        } else {
            vec![redacted]
        };
        let mut events = Vec::new();
        for (_, kind) in noise.iter().enumerate().filter(|(index, _)| index % 2 == 0) {
            events.push(noise_event(*kind));
        }
        events.extend(payload_events(family, &parts));
        for (_, kind) in noise.iter().enumerate().filter(|(index, _)| index % 2 == 1) {
            events.push(noise_event(*kind));
        }
        if family != Family::Completed {
            events.push(terminal_noise());
        }

        let mut filter = SseRestoreFilter::new_responses(&session);
        let output = push_events(&mut filter, &events);

        prop_assert_eq!(visible_text(&output, family), text);
    }
}
