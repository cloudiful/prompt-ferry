use crate::{
    db,
    openai_compat::{extract_text, persisted_assistant_message},
    worker_admin::AdminState,
};
use serde_json::Value;
use tracing::warn;

pub(super) async fn load_latest_assistant_artifact(
    state: Option<&AdminState>,
    parent_event_id: Option<i64>,
    input: &[Value],
) -> Option<(usize, Value)> {
    let parent_event_id = parent_event_id?;
    let (assistant_index, assistant_item) = input
        .iter()
        .enumerate()
        .rev()
        .find(|(_, item)| item.get("role").and_then(Value::as_str) == Some("assistant"))?;
    if has_reasoning_before(input, assistant_index) {
        return None;
    }
    let state = state?;
    let artifacts = match db::get_usage_assistant_artifacts(&state.pool, &[parent_event_id]).await {
        Ok(artifacts) => artifacts,
        Err(error) => {
            warn!(
                parent_event_id,
                error = %error,
                "failed to load the previous MiniMax assistant artifact for Responses replay"
            );
            return None;
        }
    };
    let artifact = artifacts.into_iter().next()?;
    let stored_message = persisted_assistant_message(&artifact.message_json).ok()?;
    if !assistant_message_matches(assistant_item, &stored_message) {
        return None;
    }
    Some((assistant_index, artifact.message_json))
}

fn has_reasoning_before(input: &[Value], assistant_index: usize) -> bool {
    let mut index = assistant_index;
    while index > 0 {
        let item = &input[index - 1];
        if item.get("type").and_then(Value::as_str) != Some("reasoning") {
            break;
        }
        if item
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|part| {
                    part.get("type").and_then(Value::as_str) == Some("reasoning_text")
                        && part
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty())
                })
            })
        {
            return true;
        }
        index -= 1;
    }
    false
}

fn assistant_message_matches(input: &Value, stored: &Value) -> bool {
    let input_text = input.get("content").map(extract_text).unwrap_or_default();
    let stored_text = stored.get("content").map(extract_text).unwrap_or_default();
    stored_text.trim().is_empty() || input_text == stored_text
}

#[cfg(test)]
mod tests {
    use super::assistant_message_matches;
    use serde_json::json;

    #[test]
    fn matches_responses_assistant_text_to_stored_artifact() {
        let input = json!({
            "role": "assistant",
            "content": [{"type": "output_text", "text": "answer"}]
        });
        let stored = json!({"role": "assistant", "content": "answer"});

        assert!(assistant_message_matches(&input, &stored));
    }

    #[test]
    fn rejects_a_different_assistant_message() {
        let input = json!({"role": "assistant", "content": "other"});
        let stored = json!({"role": "assistant", "content": "answer"});

        assert!(!assistant_message_matches(&input, &stored));
    }
}
