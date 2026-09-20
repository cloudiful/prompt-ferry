use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;

use crate::{db::StreamDeltaBatchingSettings, openai_compat::sse_event};

#[derive(Debug, Clone, PartialEq, Eq)]
struct BatchingKey {
    event_type: String,
    item_id: String,
    output_index: i64,
    content_index: i64,
}

#[derive(Debug, Clone)]
struct PendingEvent {
    key: BatchingKey,
    payload: Value,
    delta: String,
    last_sequence_number: Option<i64>,
    started_at: Instant,
}

pub struct StreamDeltaBatcher {
    settings: StreamDeltaBatchingSettings,
    pending: Option<PendingEvent>,
}

impl StreamDeltaBatcher {
    pub fn new(settings: StreamDeltaBatchingSettings) -> Self {
        Self {
            settings,
            pending: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    pub fn flush_window_ms(&self) -> u64 {
        self.settings.flush_window_ms
    }

    pub fn flush_due(&mut self) -> Result<Vec<Vec<u8>>> {
        if !self.is_enabled() {
            return Ok(Vec::new());
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.started_at.elapsed() >= self.flush_window())
        {
            return self.flush_pending();
        }
        Ok(Vec::new())
    }

    pub fn push_chunk(&mut self, chunk: Vec<u8>) -> Result<Vec<Vec<u8>>> {
        if !self.is_enabled() {
            return Ok(vec![chunk]);
        }

        let mut output = self.flush_due()?;
        let Some(event) = parse_sse_json_event(&chunk)? else {
            output.extend(self.flush_pending()?);
            output.push(chunk);
            return Ok(output);
        };

        let Some((key, delta, sequence_number)) = extract_mergeable_text_delta(&event) else {
            output.extend(self.flush_pending()?);
            output.push(chunk);
            return Ok(output);
        };

        match self.pending.as_mut() {
            Some(pending) if pending.key == key => {
                pending.delta.push_str(&delta);
                if let Some(seq) = sequence_number {
                    pending.last_sequence_number = Some(seq);
                    if let Some(obj) = pending.payload.as_object_mut() {
                        obj.insert("sequence_number".to_string(), serde_json::json!(seq));
                    }
                }
                pending.payload["delta"] = Value::String(pending.delta.clone());
                if should_flush_pending(&self.settings, &pending.delta) {
                    output.extend(self.flush_pending()?);
                }
            }
            Some(_) => {
                output.extend(self.flush_pending()?);
                self.pending = Some(PendingEvent {
                    key,
                    payload: event,
                    delta,
                    last_sequence_number: sequence_number,
                    started_at: Instant::now(),
                });
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| should_flush_pending(&self.settings, &pending.delta))
                {
                    output.extend(self.flush_pending()?);
                }
            }
            None => {
                self.pending = Some(PendingEvent {
                    key,
                    payload: event,
                    delta,
                    last_sequence_number: sequence_number,
                    started_at: Instant::now(),
                });
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| should_flush_pending(&self.settings, &pending.delta))
                {
                    output.extend(self.flush_pending()?);
                }
            }
        }
        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>> {
        self.flush_pending()
    }

    fn flush_pending(&mut self) -> Result<Vec<Vec<u8>>> {
        let Some(mut pending) = self.pending.take() else {
            return Ok(Vec::new());
        };
        if let Some(seq) = pending.last_sequence_number {
            if let Some(obj) = pending.payload.as_object_mut() {
                obj.insert("sequence_number".to_string(), serde_json::json!(seq));
            }
        }
        Ok(vec![
            sse_event(&pending.payload).map_err(|err| anyhow::anyhow!(err.message))?,
        ])
    }

    fn flush_window(&self) -> Duration {
        Duration::from_millis(self.settings.flush_window_ms)
    }
}

fn parse_sse_json_event(chunk: &[u8]) -> Result<Option<Value>> {
    let text = std::str::from_utf8(chunk).context("stream chunk is not valid utf-8")?;
    let trimmed = text.trim();
    if !trimmed.starts_with("data: ") || trimmed == "data: [DONE]" {
        return Ok(None);
    }
    let payload = trimmed
        .strip_prefix("data: ")
        .context("invalid sse event prefix")?;
    Ok(Some(
        serde_json::from_str(payload).context("invalid responses sse json event")?,
    ))
}

fn extract_mergeable_text_delta(event: &Value) -> Option<(BatchingKey, String, Option<i64>)> {
    let event_type = event.get("type")?.as_str()?;
    if !matches!(
        event_type,
        "response.output_text.delta" | "response.reasoning_text.delta"
    ) {
        return None;
    }
    let item_id = event.get("item_id")?.as_str()?.to_string();
    let output_index = event.get("output_index")?.as_i64()?;
    let content_index = event.get("content_index")?.as_i64()?;
    let delta = event.get("delta")?.as_str()?.to_string();
    let sequence_number = event.get("sequence_number").and_then(|value| value.as_i64());
    Some((
        BatchingKey {
            event_type: event_type.to_string(),
            item_id,
            output_index,
            content_index,
        },
        delta,
        sequence_number,
    ))
}

fn should_flush_pending(settings: &StreamDeltaBatchingSettings, delta: &str) -> bool {
    if delta.chars().count() >= settings.max_buffer_chars {
        return true;
    }
    if delta.len() >= settings.max_buffer_bytes {
        return true;
    }
    if settings.flush_on_line_break && delta.contains('\n') {
        return true;
    }
    settings.flush_on_sentence_end && ends_with_sentence_boundary(delta)
}

fn ends_with_sentence_boundary(text: &str) -> bool {
    matches!(
        text.chars().next_back(),
        Some('.' | '!' | '?' | '。' | '！' | '？')
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event_bytes(value: Value) -> Vec<u8> {
        sse_event(&value).unwrap()
    }

    #[test]
    fn disabled_passthrough_keeps_chunk() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings::default());
        let chunk = event_bytes(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "delta": "hi"
        }));
        let output = batcher.push_chunk(chunk.clone()).unwrap();
        assert_eq!(output, vec![chunk]);
    }

    #[test]
    fn batches_matching_text_delta_events_until_done() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        });
        assert!(
            batcher
                .push_chunk(event_bytes(json!({
                    "type": "response.output_text.delta",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "hel",
                    "sequence_number": 3
                })))
                .unwrap()
                .is_empty()
        );
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "lo",
                "sequence_number": 4
            })))
            .unwrap();
        assert!(output.is_empty());
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.done",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "text": "hello",
                "sequence_number": 5
            })))
            .unwrap();
        assert_eq!(output.len(), 2);
        let merged = parse_sse_json_event(&output[0]).unwrap().unwrap();
        assert_eq!(merged["delta"].as_str(), Some("hello"));
        // Sequence number must follow the most recent merged input so downstream
        // consumers keep a strictly monotonic numbering when chunks are batched.
        assert_eq!(merged["sequence_number"].as_i64(), Some(4));
    }

    #[test]
    fn keeps_sequence_number_when_key_changes_and_continues_monotonic() {
        // A three-shot merge of the same key followed by a key change should
        // leave the merged event carrying the last input's sequence number;
        // the new key should then track its own latest sequence number.
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        });
        assert!(
            batcher
                .push_chunk(event_bytes(json!({
                    "type": "response.output_text.delta",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "a",
                    "sequence_number": 10
                })))
                .unwrap()
                .is_empty()
        );
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "b",
                "sequence_number": 11
            })))
            .unwrap();
        assert!(output.is_empty());
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "c",
                "sequence_number": 12
            })))
            .unwrap();
        assert!(output.is_empty());
        // Key change flushes the three-shot merged payload carrying seq 12.
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_2",
                "output_index": 0,
                "content_index": 0,
                "delta": "x",
                "sequence_number": 13
            })))
            .unwrap();
        assert_eq!(output.len(), 1);
        let merged = parse_sse_json_event(&output[0]).unwrap().unwrap();
        assert_eq!(merged["delta"].as_str(), Some("abc"));
        assert_eq!(merged["sequence_number"].as_i64(), Some(12));
        // The new key pending carries its own latest sequence number.
        let output = batcher.finish().unwrap();
        assert_eq!(output.len(), 1);
        let flushed = parse_sse_json_event(&output[0]).unwrap().unwrap();
        assert_eq!(flushed["item_id"].as_str(), Some("msg_2"));
        assert_eq!(flushed["delta"].as_str(), Some("x"));
        assert_eq!(flushed["sequence_number"].as_i64(), Some(13));
    }

    #[test]
    fn flushes_on_line_break() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        });
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "hello\n",
                "sequence_number": 3
            })))
            .unwrap();
        assert_eq!(output.len(), 1);
    }

    #[test]
    fn flushes_when_key_changes() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        });
        assert!(
            batcher
                .push_chunk(event_bytes(json!({
                    "type": "response.output_text.delta",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "hello",
                })))
                .unwrap()
                .is_empty()
        );
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_2",
                "output_index": 0,
                "content_index": 0,
                "delta": "world",
            })))
            .unwrap();
        assert_eq!(output.len(), 1);
        let flushed = parse_sse_json_event(&output[0]).unwrap().unwrap();
        assert_eq!(flushed["item_id"].as_str(), Some("msg_1"));
        assert_eq!(flushed["delta"].as_str(), Some("hello"));
    }

    #[test]
    fn flushes_on_sentence_boundary_when_enabled() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: false,
            flush_on_sentence_end: true,
        });
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "done.",
            })))
            .unwrap();
        assert_eq!(output.len(), 1);
        let flushed = parse_sse_json_event(&output[0]).unwrap().unwrap();
        assert_eq!(flushed["delta"].as_str(), Some("done."));
    }

    #[test]
    fn flushes_before_non_mergeable_event() {
        let mut batcher = StreamDeltaBatcher::new(StreamDeltaBatchingSettings {
            enabled: true,
            flush_window_ms: 1_000,
            max_buffer_chars: 160,
            max_buffer_bytes: 1024,
            flush_on_line_break: true,
            flush_on_sentence_end: false,
        });
        assert!(
            batcher
                .push_chunk(event_bytes(json!({
                    "type": "response.output_text.delta",
                    "item_id": "msg_1",
                    "output_index": 0,
                    "content_index": 0,
                    "delta": "hello",
                })))
                .unwrap()
                .is_empty()
        );
        let output = batcher
            .push_chunk(event_bytes(json!({
                "type": "response.output_item.done",
                "output_index": 0,
                "item": {"id": "msg_1", "type": "message"}
            })))
            .unwrap();
        assert_eq!(output.len(), 2);
        let flushed = parse_sse_json_event(&output[0]).unwrap().unwrap();
        let passthrough = parse_sse_json_event(&output[1]).unwrap().unwrap();
        assert_eq!(flushed["delta"].as_str(), Some("hello"));
        assert_eq!(
            passthrough["type"].as_str(),
            Some("response.output_item.done")
        );
    }
}
