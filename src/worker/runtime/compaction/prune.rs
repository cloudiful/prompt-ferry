use serde_json::{Value, json};

const REASONING_SUMMARY_TEXT_LIMIT_CHARS: usize = 2000;

fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(4).max(1)
}

/// Strip every `encrypted_content` blob from one history item.
///
/// Both ferry-minted echoes (`ferry-`/`minimax-`) and opaque upstream blobs
/// are dropped; ferry never forwards or fabricates encrypted bytes in the
/// self-summarize path. Reasoning items keep only their plaintext `summary`
/// (truncated); `compaction`/`item_reference` items are not replayable and
/// are dropped entirely.
fn sanitize_item(item: Value) -> Option<Value> {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match item_type {
        "compaction" | "item_reference" => None,
        "reasoning" => Some(sanitize_reasoning_item(&item)),
        _ => {
            let mut item = item;
            crate::openai_compat::strip_encrypted_content_for_compact(&mut item);
            Some(item)
        }
    }
}

fn sanitize_reasoning_item(item: &Value) -> Value {
    let summary_text: String = item
        .get("summary")
        .map(crate::openai_compat::extract_text)
        .unwrap_or_default();
    let truncated: String = summary_text
        .chars()
        .take(REASONING_SUMMARY_TEXT_LIMIT_CHARS)
        .collect();
    let mut out = json!({
        "type": "reasoning",
        "summary": [{"type": "summary_text", "text": truncated}],
    });
    if let Some(id) = item.get("id").and_then(Value::as_str) {
        out["id"] = Value::String(id.to_string());
    }
    out
}

fn is_user_message(item: &Value) -> bool {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    if item_type != "message" {
        return false;
    }
    item.get("role").and_then(Value::as_str) == Some("user")
}

fn is_instruction_item(item: &Value) -> bool {
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    if item_type != "message" {
        return false;
    }
    matches!(
        item.get("role").and_then(Value::as_str),
        Some("system") | Some("developer")
    )
}

/// Split leading instruction items (`system`/`developer` messages) from
/// the compactable history, sanitize every item (drop all
/// `encrypted_content`, truncate reasoning to summary text, drop
/// `compaction`/`item_reference`), then keep the tail of the history within
/// `keep_tail_tokens` (chars/4 estimate).
///
/// The most recent two user messages and everything after the
/// second-to-last user message are always kept; older `function_call_output`
/// items are dropped head-first when over budget and replaced with a single
/// placeholder message so the handoff stays positionally coherent.
/// Returns `(head_instructions, compactable_history)`.
pub fn prune_for_compact(items: Vec<Value>, keep_tail_tokens: usize) -> (Vec<Value>, Vec<Value>) {
    let mut sanitized: Vec<Value> = items.into_iter().filter_map(sanitize_item).collect();
    let mut head = Vec::new();
    while sanitized.first().is_some_and(is_instruction_item) {
        head.push(sanitized.remove(0));
    }
    if sanitized.is_empty() {
        return (head, sanitized);
    }
    let user_positions: Vec<usize> = sanitized
        .iter()
        .enumerate()
        .filter(|(_, item)| is_user_message(item))
        .map(|(index, _)| index)
        .collect();
    let protected_from = user_positions.iter().rev().nth(1).copied().unwrap_or(0);
    let mut kept: Vec<Value> = Vec::with_capacity(sanitized.len());
    let mut budget = keep_tail_tokens.max(1);
    let mut dropped = 0usize;
    for (index, item) in sanitized.into_iter().enumerate().rev() {
        let cost = estimated_tokens(&item.to_string());
        if index >= protected_from {
            kept.push(item);
            budget = budget.saturating_sub(cost.min(budget));
            continue;
        }
        if cost <= budget {
            budget -= cost;
            kept.push(item);
        } else {
            dropped += 1;
        }
    }
    kept.reverse();
    let mut history = Vec::with_capacity(kept.len() + 1);
    if dropped > 0 {
        history.push(json!({
            "type": "message",
            "role": "user",
            "content": format!(
                "[compaction: {dropped} older history item(s) omitted to fit the compaction window]"
            ),
        }));
    }
    history.extend(kept);
    (head, history)
}

#[cfg(test)]
mod tests {
    use super::prune_for_compact;
    use serde_json::{Value, json};

    #[test]
    fn drops_all_encrypted_content_blobs() {
        let items = vec![
            json!({"type": "reasoning", "encrypted_content": "ferry-rs_1", "summary": [{"type": "summary_text", "text": "thought"}]}),
            json!({"type": "reasoning", "encrypted_content": "6e4bd8b4-opaque-upstream", "summary": [{"type": "summary_text", "text": "other"}]}),
            json!({"type": "message", "role": "user", "content": "hi", "encrypted_content": "stray"}),
        ];
        let (_, history) = prune_for_compact(items, 15_000);
        let serialized = serde_json::to_string(&history).unwrap();
        assert!(!serialized.contains("encrypted_content"));
        assert!(!serialized.contains("ferry-rs_1"));
        assert!(!serialized.contains("6e4bd8b4-opaque-upstream"));
    }

    #[test]
    fn drops_compaction_and_item_reference_items() {
        let items = vec![
            json!({"type": "compaction", "encrypted_content": "blob"}),
            json!({"type": "item_reference", "id": "x"}),
            json!({"type": "message", "role": "user", "content": "keep me"}),
        ];
        let (_, history) = prune_for_compact(items, 15_000);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["content"], "keep me");
    }

    #[test]
    fn reasoning_keeps_truncated_summary_text_only() {
        let long = "t".repeat(5000);
        let items = vec![
            json!({"id": "rs_1", "type": "reasoning", "encrypted_content": "minimax-rs_1", "summary": [{"type": "summary_text", "text": long}]}),
        ];
        let (_, history) = prune_for_compact(items, 15_000);
        assert_eq!(history.len(), 1);
        let text = history[0]["summary"][0]["text"].as_str().unwrap();
        assert_eq!(text.len(), 2000);
        assert_eq!(history[0]["id"], "rs_1");
        assert!(history[0].get("encrypted_content").is_none());
    }

    #[test]
    fn trims_old_tool_outputs_but_protects_recent_user_rounds() {
        let big = "v".repeat(4000);
        let mut items = Vec::new();
        for round in 0..6 {
            items.push(json!({"type": "message", "role": "user", "content": format!("q{round}")}));
            items.push(json!({"type": "function_call", "call_id": format!("c{round}"), "name": "read", "arguments": "{}"}));
            items.push(json!({"type": "function_call_output", "call_id": format!("c{round}"), "output": big}));
        }
        let (_, history) = prune_for_compact(items, 1500);
        let serialized = serde_json::to_string(&history).unwrap();
        assert!(serialized.contains("q5"));
        assert!(serialized.contains("q4"));
        assert!(serialized.contains("compaction:"));
        assert!(!serialized.contains("encrypted_content"));
        let history_values: Vec<Value> = history;
        assert!(history_values.iter().any(|item| {
            item["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("[compaction:"))
        }));
    }

    #[test]
    fn splits_leading_instructions_into_head() {
        let items = vec![
            json!({"type": "message", "role": "system", "content": "be concise"}),
            json!({"type": "message", "role": "user", "content": "hi"}),
        ];
        let (head, history) = prune_for_compact(items, 15_000);
        assert_eq!(head.len(), 1);
        assert_eq!(head[0]["content"], "be concise");
        assert_eq!(history.len(), 1);
    }
}
