use super::responses_summary_stream::ResponsesReasoningSummarySseFilter;
use serde_json::Value;

#[test]
fn mirrors_reasoning_deltas_as_summary_deltas() {
    let mut filter = ResponsesReasoningSummarySseFilter::new();
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
    let mut filter = ResponsesReasoningSummarySseFilter::new();
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
    let mut filter = ResponsesReasoningSummarySseFilter::new();
    let summary = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_summary_text.delta","output_index":0,"item_id":"r1","delta":"short"}

"#
            .to_vec(),
        )
        .unwrap();
    let reasoning = filter
        .push_chunk(
            br#"data: {"type":"response.reasoning_text.delta","output_index":0,"item_id":"r1","delta":"complete"}

"#
            .to_vec(),
        )
        .unwrap();

    assert_eq!(summary.len(), 1);
    assert_eq!(reasoning.len(), 1);
}

#[test]
fn does_not_repeat_full_reasoning_when_output_item_completes() {
    let mut filter = ResponsesReasoningSummarySseFilter::new();
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
