use std::borrow::Cow;

use serde_json::{Map, Value, json};

use super::extract_text;

/// Self-describing token prefix minted onto MiniMax Responses-passthrough
/// reasoning items that arrive without `encrypted_content` (issue #459).
pub(crate) const MINIMAX_REASONING_ENCRYPTED_PREFIX: &str = "minimax-";
/// Self-describing token prefix minted onto reasoning items produced by the
/// Chat->Responses bridge (`ChatResponseStreamAdapter`) when the upstream
/// chat provider returns no encrypted content.
pub(crate) const BRIDGE_REASONING_ENCRYPTED_PREFIX: &str = "ferry-";

pub(crate) fn minimax_reasoning_encrypted_token(id: &str) -> String {
    format!("{MINIMAX_REASONING_ENCRYPTED_PREFIX}{id}")
}

pub(crate) fn bridge_reasoning_encrypted_token(id: &str) -> String {
    format!("{BRIDGE_REASONING_ENCRYPTED_PREFIX}{id}")
}

/// Mint `token` as `encrypted_content` on a reasoning item that lacks a
/// usable value. Items of other types, items that already carry a non-empty
/// value, and empty tokens are left untouched.
pub(crate) fn mint_reasoning_encrypted_content(item: &mut Value, token: &str) -> bool {
    if item.get("type").and_then(Value::as_str) != Some("reasoning") || token.is_empty() {
        return false;
    }
    if is_present_encrypted_content(item.get("encrypted_content")) {
        return false;
    }
    item["encrypted_content"] = Value::String(token.to_string());
    true
}

fn is_present_encrypted_content(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

/// Original id embedded in one of ferry's self-describing reasoning tokens,
/// or `None` when `value` is an opaque/absent upstream token. Both mint
/// prefixes are ferry-defined and never collide with a provider's own opaque
/// `encrypted_content`, so recognizing either is safe.
fn ferry_reasoning_echo_source(value: &Value) -> Option<&str> {
    let token = value.as_str()?;
    for prefix in [
        MINIMAX_REASONING_ENCRYPTED_PREFIX,
        BRIDGE_REASONING_ENCRYPTED_PREFIX,
    ] {
        if let Some(original) = token.strip_prefix(prefix)
            && !original.is_empty()
        {
            return Some(original);
        }
    }
    None
}

/// Request-path counterpart to the response mint (issue #459).
///
/// Ferry-minted reasoning echoes (`minimax-<id>` / `ferry-<id>`) are rewritten
/// back into provider-plausible reasoning items before a Responses request is
/// translated: the original `id` is restored, the ferry token is stripped, and
/// `content[].reasoning_text` is backfilled from the plaintext `summary` when
/// the echo omitted it. Bodies without a ferry-minted token are borrowed
/// byte-for-byte, so prefix caches are never disturbed for untouched requests.
pub(crate) fn restore_reasoning_echoes(body: &[u8]) -> Cow<'_, [u8]> {
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Cow::Borrowed(body);
    };
    let changed = value
        .as_object_mut()
        .and_then(|object| object.get_mut("input"))
        .and_then(Value::as_array_mut)
        .is_some_and(|input| {
            input
                .iter_mut()
                .map(restore_reasoning_echo_item)
                .fold(false, |changed, item| changed | item)
        });
    if !changed {
        return Cow::Borrowed(body);
    }
    match serde_json::to_vec(&value) {
        Ok(bytes) => Cow::Owned(bytes),
        Err(_) => Cow::Borrowed(body),
    }
}

fn restore_reasoning_echo_item(item: &mut Value) -> bool {
    let Some(object) = item.as_object_mut() else {
        return false;
    };
    if object.get("type").and_then(Value::as_str) != Some("reasoning") {
        return false;
    }
    let Some(original_id) = object
        .get("encrypted_content")
        .and_then(ferry_reasoning_echo_source)
        .map(str::to_string)
    else {
        return false;
    };
    object.remove("encrypted_content");
    object.insert("id".to_string(), Value::String(original_id));
    if !has_reasoning_text_content(object)
        && let Some(text) = object
            .get("summary")
            .map(extract_text)
            .filter(|text| !text.trim().is_empty())
    {
        object.insert(
            "content".to_string(),
            json!([{"type": "reasoning_text", "text": text}]),
        );
    }
    true
}

fn has_reasoning_text_content(object: &Map<String, Value>) -> bool {
    object
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
}

#[cfg(test)]
mod tests {
    use super::{
        bridge_reasoning_encrypted_token, minimax_reasoning_encrypted_token,
        mint_reasoning_encrypted_content, restore_reasoning_echoes,
    };
    use serde_json::{Value, json};

    #[test]
    fn builds_self_describing_tokens() {
        assert_eq!(
            minimax_reasoning_encrypted_token("resp_1_rs"),
            "minimax-resp_1_rs"
        );
        assert_eq!(bridge_reasoning_encrypted_token("rs_9"), "ferry-rs_9");
    }

    #[test]
    fn mint_skips_non_reasoning_and_existing_values() {
        let mut message = json!({"type": "message"});
        assert!(!mint_reasoning_encrypted_content(&mut message, "minimax-x"));
        assert!(message.get("encrypted_content").is_none());

        let mut existing = json!({"type": "reasoning", "encrypted_content": "opaque"});
        assert!(!mint_reasoning_encrypted_content(
            &mut existing,
            "minimax-x"
        ));
        assert_eq!(existing["encrypted_content"], "opaque");

        let mut blank = json!({"type": "reasoning", "encrypted_content": ""});
        assert!(mint_reasoning_encrypted_content(&mut blank, "minimax-x"));
        assert_eq!(blank["encrypted_content"], "minimax-x");
    }

    #[test]
    fn restore_rewrites_own_token_and_backfills_content() {
        let body = br#"{"input":[{"id":"r1","type":"reasoning","encrypted_content":"minimax-resp_1_rs","summary":[{"type":"summary_text","text":"think"}]}]}"#;
        let out = restore_reasoning_echoes(body);
        let value: Value = serde_json::from_slice(out.as_ref()).unwrap();
        let item = &value["input"][0];
        assert!(item.get("encrypted_content").is_none());
        assert_eq!(item["id"], "resp_1_rs");
        assert_eq!(item["content"][0]["type"], "reasoning_text");
        assert_eq!(item["content"][0]["text"], "think");
    }

    #[test]
    fn restore_handles_bridge_token() {
        let body = br#"{"input":[{"type":"reasoning","encrypted_content":"ferry-rs_9","summary":[{"type":"summary_text","text":"s"}]}]}"#;
        let out = restore_reasoning_echoes(body);
        let value: Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(value["input"][0]["id"], "rs_9");
        assert!(value["input"][0].get("encrypted_content").is_none());
    }

    #[test]
    fn restore_borrows_opaque_and_foreign_bodies() {
        for body in [
            br#"{"input":[{"type":"reasoning","encrypted_content":"6e4bd8b4-b70d-4f22-aef6-cb5b905790b5-0","summary":[]}]}"#.as_slice(),
            br#"{"input":[{"type":"reasoning","encrypted_content":"minimaxonly","summary":[]}]}"#.as_slice(),
            br#"{"input":[{"role":"user","content":"hi"}]}"#.as_slice(),
            b"not json".as_slice(),
        ] {
            assert!(
                matches!(restore_reasoning_echoes(body), std::borrow::Cow::Borrowed(_)),
                "foreign/untouched body must borrow: {}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn restore_keeps_existing_reasoning_text_content() {
        let body = br#"{"input":[{"type":"reasoning","encrypted_content":"minimax-rs_2","content":[{"type":"reasoning_text","text":"keep"}],"summary":[{"type":"summary_text","text":"other"}]}]}"#;
        let out = restore_reasoning_echoes(body);
        let value: Value = serde_json::from_slice(out.as_ref()).unwrap();
        assert_eq!(value["input"][0]["id"], "rs_2");
        assert_eq!(value["input"][0]["content"][0]["text"], "keep");
    }
}
