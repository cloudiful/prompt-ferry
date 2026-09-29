//! Chunk-batch equivalence and terminal-lifecycle assertions.

use serde_json::Value;

use super::{SseRestoreFilter, data_json, delta_event, session};
use crate::worker::runtime::error_handling::ResponsesSseTerminal;

#[test]
fn push_chunks_equals_sequential_push_chunk_outputs() {
    let (session, token) = session("a.example.com");
    let split = token.len() / 2;

    let mut batched_filter = SseRestoreFilter::new(&session);
    let batched = batched_filter
        .push_chunks(vec![
            delta_event("msg", 0, &token[..split]).into_bytes(),
            delta_event("msg", 0, &token[split..]).into_bytes(),
        ])
        .expect("batched restore");

    let mut sequential_filter = SseRestoreFilter::new(&session);
    let mut sequential = sequential_filter
        .push_chunk(delta_event("msg", 0, &token[..split]).as_bytes())
        .expect("first");
    sequential.extend(
        sequential_filter
            .push_chunk(delta_event("msg", 0, &token[split..]).as_bytes())
            .expect("second"),
    );

    assert_eq!(batched.len(), 2, "batched run restores the same events");
    assert_eq!(batched, sequential, "chunk batching must be transparent");
    assert_eq!(data_json(&batched[0])["delta"], "");
    assert_eq!(data_json(&batched[1])["delta"], "a.example.com");
}

#[test]
fn push_chunks_of_many_chunks_restores_a_token_split_three_ways() {
    let (session, token) = session("a.example.com");
    let third = token.len() / 3;
    let mut filter = SseRestoreFilter::new(&session);

    let output = filter
        .push_chunks(vec![
            delta_event("msg", 0, &token[..third]).into_bytes(),
            delta_event("msg", 0, &token[third..2 * third]).into_bytes(),
            delta_event("msg", 0, &token[2 * third..]).into_bytes(),
        ])
        .expect("restore");

    let restored: Vec<Value> = output.iter().map(|event| data_json(event)).collect();
    assert_eq!(restored[0]["delta"], "");
    assert_eq!(restored[1]["delta"], "");
    assert_eq!(restored[2]["delta"], "a.example.com");
}

#[test]
fn push_chunks_with_no_chunks_yields_no_output() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    assert!(filter.push_chunks(Vec::new()).expect("empty").is_empty());
    assert!(!filter.is_done());
}

#[test]
fn is_done_flips_only_after_a_terminal_event() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new_responses(&session);
    assert!(!filter.is_done(), "fresh filter is not done");

    filter
        .push_chunk(delta_event("msg", 0, "plain").as_bytes())
        .expect("delta");
    assert!(!filter.is_done(), "non-terminal event is not done");

    filter
        .push_chunk(b"data: {\"type\":\"response.completed\"}\n\n")
        .expect("terminal");
    assert!(filter.is_done(), "responses terminal marks the filter done");
    assert_eq!(
        filter.responses_terminal(),
        Some(ResponsesSseTerminal::Completed)
    );
    // Events after the terminal produce no further output.
    assert!(
        filter
            .push_chunk(delta_event("msg", 0, "late").as_bytes())
            .expect("post-terminal")
            .is_empty()
    );
}

#[test]
fn legacy_done_marker_marks_the_filter_done() {
    let (session, _) = session("a.example.com");
    let mut filter = SseRestoreFilter::new(&session);
    assert!(!filter.is_done());
    filter.push_chunk(b"data: [DONE]\n\n").expect("done");
    assert!(filter.is_done());
    assert_eq!(filter.responses_terminal(), None);
}
