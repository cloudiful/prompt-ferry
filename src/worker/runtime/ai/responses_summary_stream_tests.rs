use super::responses_summary_stream::ResponsesReasoningSummarySseFilter;
use serde_json::Value;

fn parse_events(chunks: &[Vec<u8>]) -> Vec<Value> {
    chunks
        .iter()
        .flat_map(|chunk| {
            String::from_utf8_lossy(chunk)
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn mirrors_reasoning_deltas_as_summary_deltas() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(false);
    let output = filter
        .push_chunk(
            br#"event: response.reasoning_text.delta
data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"think"}

"#
            .to_vec(),
        )
        .unwrap();

    assert_eq!(output.len(), 3);
    assert!(String::from_utf8_lossy(&output[0]).contains("response.reasoning_summary_part.added"));
    let summary: Value = serde_json::from_slice(
        String::from_utf8_lossy(&output[2])
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(summary["delta"], "think");
}

#[test]
fn fills_missing_summary_on_completed_response() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(false);
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.completed","response":{"output":[{"type":"reasoning","content":[{"type":"reasoning_text","text":"complete"}]}]}}

"#
            .to_vec(),
        )
        .unwrap();
    let text = String::from_utf8_lossy(&output[0]);
    assert!(text.contains("summary_text"));
    assert!(text.contains("complete"));
}

#[test]
fn does_not_duplicate_an_upstream_summary_delta() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let summary = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_summary_text.delta","output_index":0,"item_id":"r1","delta":"short"}

"#
            .to_vec(),
        )
        .unwrap();
    // Upstream summary already announced the item once with the minted token.
    let summary_events = parse_events(&summary);
    assert_eq!(
        summary_events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .count(),
        1
    );
    assert_eq!(summary_events[0]["item"]["encrypted_content"], "minimax-r1");

    let reasoning = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"complete"}

"#
            .to_vec(),
        )
        .unwrap();

    let events = parse_events(&reasoning);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "response.reasoning_text.delta");
}

#[test]
fn does_not_repeat_full_reasoning_when_output_item_completes() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(false);
    filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"complete"}

"#
            .to_vec(),
        )
        .unwrap();
    filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.done","output_index":0,"item_id":"r1","text":"complete"}

"#
            .to_vec(),
        )
        .unwrap();

    let output = filter
        .push_chunk(
            br#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"r1","type":"reasoning","content":[{"type":"reasoning_text","text":"complete"}]}}

"#
            .to_vec(),
        )
        .unwrap();

    assert_eq!(output.len(), 1);
    assert!(String::from_utf8_lossy(&output[0]).contains("summary_text"));
}

#[test]
fn mints_minimax_echo_token_on_added_and_done_items() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let added = filter
        .push_chunk(
            br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"resp_1_rs","type":"reasoning","status":"in_progress","summary":[],"content":[]}}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&added);
    assert_eq!(events[0]["item"]["encrypted_content"], "minimax-resp_1_rs");

    let done = filter
        .push_chunk(
            br#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"resp_1_rs","type":"reasoning","status":"completed","summary":[{"type":"summary_text","text":"t"}],"content":[{"type":"reasoning_text","text":"t"}]}}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&done);
    assert_eq!(events[0]["item"]["encrypted_content"], "minimax-resp_1_rs");
}

#[test]
fn does_not_mint_when_disabled_or_token_already_present() {
    let mut disabled = ResponsesReasoningSummarySseFilter::new(false);
    let output = disabled
        .push_chunk(
            br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"resp_1_rs","type":"reasoning","summary":[]}}

"#
            .to_vec(),
        )
        .unwrap();
    assert!(
        parse_events(&output)[0]["item"]
            .get("encrypted_content")
            .is_none()
    );

    let mut enabled = ResponsesReasoningSummarySseFilter::new(true);
    let body = br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"resp_1_rs","type":"reasoning","encrypted_content":"upstream-opaque","summary":[]}}

"#
        .to_vec();
    let output = enabled.push_chunk(body.clone()).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0], body);
    assert_eq!(
        parse_events(&output)[0]["item"]["encrypted_content"],
        "upstream-opaque"
    );
}

#[test]
fn mints_minimax_echo_token_on_completed_output() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.completed","response":{"output":[{"id":"resp_2_rs","type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":"think"}]}]}}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&output);
    let item = &events[0]["response"]["output"][0];
    assert_eq!(item["encrypted_content"], "minimax-resp_2_rs");
    assert_eq!(item["summary"][0]["text"], "think");
}

#[test]
fn does_not_mint_on_failed_or_incomplete_completed_payload() {
    // A non-reasoning item or an already-tokenized item in the completed
    // payload must not be rewritten.
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let body = br#"data: {"type":"response.completed","response":{"output":[{"id":"msg_1","type":"message","content":[{"type":"output_text","text":"hi"}]},{"id":"resp_3_rs","type":"reasoning","encrypted_content":"upstream-opaque","summary":[{"type":"summary_text","text":"t"}]}]}}

"#
        .to_vec();
    let output = filter.push_chunk(body.clone()).unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0], body);
}

#[test]
fn synthesizes_missing_output_item_added_before_reasoning_delta() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"resp_1_rs","delta":"think"}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&output);
    // added, summary_part.added, original reasoning_text.delta, summary delta
    assert_eq!(events[0]["type"], "response.output_item.added");
    assert_eq!(events[0]["item"]["id"], "resp_1_rs");
    assert_eq!(events[0]["item"]["encrypted_content"], "minimax-resp_1_rs");
    assert_eq!(events[1]["type"], "response.reasoning_summary_part.added");
    assert_eq!(events[2]["type"], "response.reasoning_text.delta");
    assert_eq!(events[3]["type"], "response.reasoning_summary_text.delta");
}

#[test]
fn does_not_synthesize_added_on_non_minimax_passthrough() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(false);
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"think"}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&output);
    assert!(
        events
            .iter()
            .all(|event| event["type"] != "response.output_item.added"),
        "non-MiniMax passthrough must not gain synthesized items"
    );
}

#[test]
fn synthesizes_output_item_added_only_once_across_deltas() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let mut all = Vec::new();
    for _ in 0..2 {
        all.extend(
            filter
                .push_chunk(
                    br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"x"}

"#
                    .to_vec(),
                )
                .unwrap(),
        );
    }
    all.extend(
        filter
            .push_chunk(
                br#"data: {"type":"response.reasoning_text.done","output_index":0,"item_id":"r1","text":"xx"}

"#
                .to_vec(),
            )
            .unwrap(),
    );
    let events = parse_events(&all);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "response.output_item.added")
            .count(),
        1,
        "only the first reference announces the item"
    );
}

#[test]
fn synthesizes_output_item_added_when_upstream_omits_it_before_done() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(true);
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.done","output_index":3,"item_id":"r7","text":"plan"}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&output);
    assert_eq!(events[0]["type"], "response.output_item.added");
    assert_eq!(events[0]["output_index"], 3);
    assert_eq!(events[0]["item"]["id"], "r7");
    assert_eq!(events[0]["item"]["encrypted_content"], "minimax-r7");
    assert_eq!(events[1]["type"], "response.reasoning_text.done");
}

#[test]
fn does_not_synthesize_added_when_upstream_already_sent_it() {
    let mut filter = ResponsesReasoningSummarySseFilter::new(false);
    filter
        .push_chunk(
            br#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"r1","type":"reasoning","summary":[]}}

"#
            .to_vec(),
        )
        .unwrap();
    let output = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"think"}

"#
            .to_vec(),
        )
        .unwrap();
    let events = parse_events(&output);
    assert!(
        events
            .iter()
            .all(|event| event["type"] != "response.output_item.added"),
        "upstream added event must not be duplicated"
    );
}
