use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use serde_json::{Value, json};

use crate::openai_compat::ensure_reasoning_summary;

pub(super) struct ResponsesReasoningSummarySseFilter {
    reasoning_text: HashMap<String, String>,
    summary_started: HashSet<String>,
    summary_seen: HashSet<String>,
    summary_completed: HashSet<String>,
}

impl ResponsesReasoningSummarySseFilter {
    pub(super) fn new() -> Self {
        Self {
            reasoning_text: HashMap::new(),
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
                    {
                        if let Some(text) = item
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
                            output.push(summary_text_done(
                                output_index.clone(),
                                &key,
                                complete_text,
                            ));
                            output.push(summary_part_done(
                                output_index.clone(),
                                &key,
                                complete_text,
                            ));
                            self.summary_completed.insert(key.clone());
                        }
                    }
                    changed = ensure_reasoning_summary(item, fallback.as_deref());
                    if changed && event_type == "response.output_item.added" {
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
                self.summary_seen.insert(reasoning_key(&value));
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
        output
            .iter_mut()
            .map(|item| {
                if item.get("type").and_then(Value::as_str) != Some("reasoning") {
                    return false;
                }
                let key = reasoning_key(item);
                let fallback = self.reasoning_text.get(&key).cloned();
                ensure_reasoning_summary(item, fallback.as_deref())
            })
            .any(|changed| changed)
    }
}

fn reasoning_key(value: &Value) -> String {
    value
        .get("item_id")
        .or_else(|| value.get("id"))
        .or_else(|| value.get("item").and_then(|item| item.get("id")))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| {
            value
                .get("output_index")
                .and_then(Value::as_u64)
                .map(|index| format!("output:{index}"))
                .unwrap_or_else(|| "output:0".to_string())
        })
}

fn summary_part_added(output_index: Value, key: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_part.added",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "part": {"type": "summary_text", "text": ""}
    }))
}

fn summary_delta(output_index: Value, key: &str, delta: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_text.delta",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "delta": delta
    }))
}

fn summary_part_done(output_index: Value, key: &str, text: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_part.done",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "part": {"type": "summary_text", "text": text}
    }))
}

fn summary_text_done(output_index: Value, key: &str, text: &str) -> Vec<u8> {
    sse_event(json!({
        "type": "response.reasoning_summary_text.done",
        "output_index": output_index,
        "summary_index": 0,
        "item_id": key,
        "text": text
    }))
}

fn sse_event(value: Value) -> Vec<u8> {
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    format!("event: {event_type}\ndata: {}\n\n", value).into_bytes()
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
