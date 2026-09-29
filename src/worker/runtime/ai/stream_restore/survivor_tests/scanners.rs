//! Pure-helper assertions: the snapshot-field allowlist, stream-key identity,
//! the token-path scanner, the token log prefix, and terminal detection.

use serde_json::json;

use super::super::{
    is_terminal_event, response_stream_key, snapshot_field_is_text, token_bearing_paths,
    token_prefix,
};

#[test]
fn snapshot_field_allowlist_is_exhaustive_over_the_restored_fields() {
    for field in [
        "text",
        "arguments",
        "output",
        "summary",
        "content",
        "reasoning_content",
        "reasoning",
        "reasoning_text",
        "thinking",
        "chain_of_thought",
        "refusal",
    ] {
        assert!(snapshot_field_is_text(field), "{field} must be restorable");
    }
    for field in [
        "id",
        "type",
        "status",
        "call_id",
        "name",
        "annotations",
        "role",
    ] {
        assert!(
            !snapshot_field_is_text(field),
            "{field} must never be restored as a text field"
        );
    }
}

#[test]
fn response_stream_keys_distinguish_item_and_indexes() {
    let event_type = "response.output_text.delta";
    let base =
        json!({"item_id": "msg_1", "output_index": 1, "content_index": 2, "summary_index": 3});
    assert_eq!(
        response_stream_key(event_type, &base),
        "responses:response.output_text.delta:msg_1:1:2:3"
    );

    let other_item =
        json!({"item_id": "msg_2", "output_index": 1, "content_index": 2, "summary_index": 3});
    assert_ne!(
        response_stream_key(event_type, &base),
        response_stream_key(event_type, &other_item),
        "different items must not share a stream"
    );
    let other_content =
        json!({"item_id": "msg_1", "output_index": 1, "content_index": 5, "summary_index": 3});
    assert_ne!(
        response_stream_key(event_type, &base),
        response_stream_key(event_type, &other_content),
        "different content indexes must not share a stream"
    );
    let other_summary =
        json!({"item_id": "msg_1", "output_index": 1, "content_index": 2, "summary_index": 9});
    assert_ne!(
        response_stream_key(event_type, &base),
        response_stream_key(event_type, &other_summary),
        "different summary indexes must not share a stream"
    );
    assert_ne!(
        response_stream_key(event_type, &base),
        response_stream_key("response.reasoning_summary_text.delta", &base),
        "different event families must not share a stream"
    );

    let empty = json!({});
    assert_eq!(
        response_stream_key(event_type, &empty),
        "responses:response.output_text.delta::0:0:0"
    );
}

#[test]
fn token_paths_traverse_arrays_and_objects() {
    let token = "[[RDX:v2:text:domain:001:cafebabe]]";
    let value = json!({
        "plain": "clean",
        "list": ["clean", {"nested": token}],
        "direct": token,
    });
    assert_eq!(
        token_bearing_paths(&value),
        vec!["/direct".to_string(), "/list/1/nested".to_string()]
    );

    let deep = json!({"a": [{"b": [{"c": token}]}]});
    assert_eq!(token_bearing_paths(&deep), vec!["/a/0/b/0/c".to_string()]);
    assert!(token_bearing_paths(&json!("no tokens here")).is_empty());
    assert!(token_bearing_paths(&json!({})).is_empty());
}

#[test]
fn token_prefix_is_the_marker_plus_a_bounded_slice() {
    assert_eq!(token_prefix("no marker"), "[[RDX:v2:");
    let token = "[[RDX:v2:text:domain:001:cafebabe]] tail";
    assert_eq!(token_prefix(token), &token[.."[[RDX:v2:".len() + 8]);
    let bare = "[[RDX:v2:";
    assert_eq!(token_prefix(bare), bare);
}

#[test]
fn terminal_event_detection_covers_all_five_families() {
    for event_type in [
        "response.completed",
        "response.failed",
        "response.incomplete",
        "error",
        "message_stop",
    ] {
        assert!(
            is_terminal_event(&json!({"type": event_type})),
            "{event_type} must be terminal"
        );
    }
    for event_type in [
        "response.output_text.delta",
        "content_block_delta",
        "response.output_item.done",
        "chat",
    ] {
        assert!(
            !is_terminal_event(&json!({"type": event_type})),
            "{event_type} must not be terminal"
        );
    }
    assert!(!is_terminal_event(&json!({})));
}
