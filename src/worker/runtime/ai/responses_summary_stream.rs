use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use super::responses_summary_events::{
    reasoning_item_added, reasoning_key, summary_delta, summary_part_added, summary_part_done,
    summary_text_done,
};
use crate::openai_compat::{
    ensure_reasoning_summary, minimax_reasoning_encrypted_token, mint_reasoning_encrypted_content,
};

pub(super) struct ResponsesReasoningSummarySseFilter {
    mint_encrypted_content: bool,
    reasoning_text: HashMap<String, String>,
    item_added: HashSet<String>,
    summary_started: HashSet<String>,
    summary_seen: HashSet<String>,
    summary_completed: HashSet<String>,
}

impl ResponsesReasoningSummarySseFilter {
    /// `mint_encrypted_content` enables the MiniMax issue #459 echo fix on a
    /// Responses passthrough stream: reasoning items that arrive without
    /// `encrypted_content` gain a self-describing `minimax-<original id>`
    /// token, and a missing `response.output_item.added` is synthesized before
    /// the first reasoning/summary event (MiniMax can start reasoning at the
    /// text-delta stage).
    pub(super) fn new(mint_encrypted_content: bool) -> Self {
        Self {
            mint_encrypted_content,
            reasoning_text: HashMap::new(),
            item_added: HashSet::new(),
            summary_started: HashSet::new(),
            summary_seen: HashSet::new(),
            summary_completed: HashSet::new(),
        }
    }

    pub(super) fn push_chunk(&mut self, chunk: Vec<u8>) -> Result<Vec<Vec<u8>>> {
        let Some((data_start, data_end, mut value)) = parse_event(&chunk)? else {
            return Ok(vec![chunk]);
        };
        let Some(event_type) = value.get("type").and_then(Value::as_str).map(str::to_owned) else {
            return Ok(vec![chunk]);
        };
        let output_index = value.get("output_index").cloned().unwrap_or(json!(0));
        let mut output = Vec::new();
        match event_type.as_str() {
            "response.output_item.added" | "response.output_item.done" => {
                let item = if value.get("item").is_some() {
                    value.get_mut("item").expect("item exists")
                } else {
                    &mut value
                };
                let mut changed = false;
                if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                    let key = reasoning_key(item);
                    let minted = self.mint_item(item);
                    self.item_added.insert(key.clone());
                    let has_summary = item.get("summary").is_some_and(|summary| {
                        !crate::openai_compat::extract_text(summary)
                            .trim()
                            .is_empty()
                    });
                    if has_summary {
                        self.summary_seen.insert(key.clone());
                    }
                    let fallback = self.reasoning_text.get(&key).cloned();
                    if event_type == "response.output_item.done"
                        && !has_summary
                        && !self.summary_seen.contains(&key)
                        && !self.summary_completed.contains(&key)
                        && let Some(text) = item
                            .get("content")
                            .map(crate::openai_compat::extract_text)
                            .filter(|text| !text.trim().is_empty())
                            .or_else(|| fallback.clone())
                    {
                        if self.summary_started.insert(key.clone()) {
                            output.push(summary_part_added(output_index.clone(), &key));
                        }
                        let known_text = self
                            .reasoning_text
                            .get(&key)
                            .filter(|known| !known.is_empty())
                            .cloned();
                        if known_text.is_none() {
                            output.push(summary_delta(output_index.clone(), &key, &text));
                            self.reasoning_text.insert(key.clone(), text.clone());
                        }
                        let complete_text = known_text.as_deref().unwrap_or(&text);
                        output.push(summary_text_done(output_index.clone(), &key, complete_text));
                        output.push(summary_part_done(output_index.clone(), &key, complete_text));
                        self.summary_completed.insert(key.clone());
                    }
                    let summary_filled = ensure_reasoning_summary(item, fallback.as_deref());
                    changed = summary_filled || minted;
                    if summary_filled && event_type == "response.output_item.added" {
                        self.summary_seen.insert(key);
                    }
                }
                if changed {
                    output.push(replace_data(&chunk, data_start, data_end, &value)?);
                } else {
                    output.push(chunk);
                }
            }
            "response.reasoning_text.delta" => {
                let key = reasoning_key(&value);
                if let Some(delta) = value.get("delta").and_then(Value::as_str)
                    && !delta.is_empty()
                {
                    self.reasoning_text
                        .entry(key.clone())
                        .or_default()
                        .push_str(delta);
                    if !self.summary_seen.contains(&key) {
                        self.ensure_item_announced(&mut output, &output_index, &key);
                        if self.summary_started.insert(key.clone()) {
                            output.push(summary_part_added(output_index.clone(), &key));
                        }
                        if !self.summary_completed.contains(&key) {
                            output.push(chunk);
                            output.push(summary_delta(output_index.clone(), &key, delta));
                        } else {
                            output.push(chunk);
                        }
                    } else {
                        output.push(chunk);
                    }
                } else {
                    output.push(chunk);
                }
            }
            "response.reasoning_text.done" => {
                let key = reasoning_key(&value);
                if let Some(text) = value
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    && self.reasoning_text.get(&key).is_none_or(String::is_empty)
                {
                    self.reasoning_text.insert(key.clone(), text.to_string());
                }
                if !self.summary_seen.contains(&key) && !self.summary_completed.contains(&key) {
                    self.ensure_item_announced(&mut output, &output_index, &key);
                }
                output.push(chunk);
                if !self.summary_seen.contains(&key) && !self.summary_completed.contains(&key) {
                    let text = self.reasoning_text.get(&key).cloned().unwrap_or_default();
                    if self.summary_started.insert(key.clone()) {
                        output.push(summary_part_added(output_index.clone(), &key));
                    }
                    output.push(summary_text_done(output_index.clone(), &key, &text));
                    output.push(summary_part_done(output_index.clone(), &key, &text));
                    self.summary_completed.insert(key);
                }
            }
            "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done" => {
                let key = reasoning_key(&value);
                if !self.summary_seen.contains(&key) {
                    self.ensure_item_announced(&mut output, &output_index, &key);
                }
                self.summary_seen.insert(key);
                output.push(chunk);
            }
            "response.completed" => {
                if self.normalize_completed_response(&mut value) {
                    output.push(replace_data(&chunk, data_start, data_end, &value)?);
                } else {
                    output.push(chunk);
                }
            }
            _ => output.push(chunk),
        }
        Ok(output)
    }

    /// Emit a synthesized `response.output_item.added` the first time a
    /// reasoning item is referenced by summary/reasoning events but no
    /// upstream `added` was observed. MiniMax can start a reasoning stream at
    /// the text-delta stage, and clients need the item announcement (with the
    /// minted token) before summary parts can be attached. Only the MiniMax
    /// echo path synthesizes, so other Responses-native upstreams stay
    /// byte-for-byte untouched.
    fn ensure_item_announced(
        &mut self,
        output: &mut Vec<Vec<u8>>,
        output_index: &Value,
        key: &str,
    ) {
        if !self.mint_encrypted_content || !self.item_added.insert(key.to_string()) {
            return;
        }
        output.push(reasoning_item_added(
            output_index.clone(),
            key,
            self.reasoning_token(key),
        ));
    }

    fn reasoning_token(&self, key: &str) -> Option<String> {
        self.mint_encrypted_content
            .then(|| minimax_reasoning_encrypted_token(key))
    }

    fn mint_item(&self, item: &mut Value) -> bool {
        if !self.mint_encrypted_content {
            return false;
        }
        // Prefer the item's own id so the embedded original round-trips; fall
        // back to the event key when the item itself is anonymous.
        let token = item
            .get("id")
            .and_then(Value::as_str)
            .map(minimax_reasoning_encrypted_token)
            .unwrap_or_else(|| minimax_reasoning_encrypted_token(&reasoning_key(item)));
        mint_reasoning_encrypted_content(item, &token)
    }

    fn normalize_completed_response(&self, value: &mut Value) -> bool {
        if let Some(response) = value.get_mut("response") {
            self.normalize_response_output(response)
        } else {
            self.normalize_response_output(value)
        }
    }

    fn normalize_response_output(&self, value: &mut Value) -> bool {
        let Some(output) = value.get_mut("output").and_then(Value::as_array_mut) else {
            return false;
        };
        output.iter_mut().fold(false, |changed, item| {
            if item.get("type").and_then(Value::as_str) != Some("reasoning") {
                return changed;
            }
            let key = reasoning_key(item);
            let fallback = self.reasoning_text.get(&key).cloned();
            let minted = self.mint_item(item);
            ensure_reasoning_summary(item, fallback.as_deref()) || minted || changed
        })
    }
}

fn parse_event(event: &[u8]) -> Result<Option<(usize, usize, Value)>> {
    let text = std::str::from_utf8(event).map_err(|err| anyhow!(err))?;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if let Some(data) = content.strip_prefix("data:") {
            let start = offset + content.find("data:").unwrap_or(0);
            let end = start + content.len();
            let payload = data.trim_start();
            if payload == "[DONE]" || payload.is_empty() {
                return Ok(None);
            }
            return Ok(Some((start, end, serde_json::from_str(payload)?)));
        }
        offset += line.len();
    }
    Ok(None)
}

fn replace_data(
    event: &[u8],
    data_start: usize,
    data_end: usize,
    value: &Value,
) -> Result<Vec<u8>> {
    let mut output = Vec::with_capacity(event.len() + 64);
    output.extend_from_slice(&event[..data_start]);
    output.extend_from_slice(format!("data: {value}").as_bytes());
    output.extend_from_slice(&event[data_end..]);
    Ok(output)
}
