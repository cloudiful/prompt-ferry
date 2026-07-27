use std::collections::BTreeMap;

use http::StatusCode;
use serde_json::Value;

use super::{CompatError, decode_sse_chunk, finish_sse_decoder};
use crate::stream_text::Utf8LineDecoder;

#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
    emitted_arguments: usize,
}

pub struct ResponsesChatResponseStreamAdapter {
    sse_decoder: Utf8LineDecoder,
    response_id: Option<String>,
    model: Option<String>,
    created_at: Option<i64>,
    full_text: String,
    full_reasoning: String,
    usage: Option<Value>,
    tool_calls: Vec<ToolCall>,
    tool_positions: BTreeMap<usize, usize>,
    created_emitted: bool,
    completed: bool,
    finish_reason: Option<&'static str>,
}

impl Default for ResponsesChatResponseStreamAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponsesChatResponseStreamAdapter {
    pub fn new() -> Self {
        Self {
            sse_decoder: Utf8LineDecoder::default(),
            response_id: None,
            model: None,
            created_at: None,
            full_text: String::new(),
            full_reasoning: String::new(),
            usage: None,
            tool_calls: Vec::new(),
            tool_positions: BTreeMap::new(),
            created_emitted: false,
            completed: false,
            finish_reason: None,
        }
    }

    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<Vec<u8>>, CompatError> {
        let mut output = Vec::new();
        for line in decode_sse_chunk(&mut self.sse_decoder, chunk)? {
            self.process_line(&line, &mut output)?;
        }
        Ok(output)
    }

    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, CompatError> {
        let mut output = Vec::new();
        if let Some(line) = finish_sse_decoder(&mut self.sse_decoder)?
            && !line.trim().is_empty()
        {
            self.process_line(line.trim_end_matches(['\r', '\n']), &mut output)?;
        }
        if !self.completed {
            self.emit_completion(&mut output)?;
        }
        Ok(output)
    }

    pub fn provider_response_id(&self) -> Option<&str> {
        self.response_id.as_deref()
    }

    pub fn model_name(&self) -> Option<&str> {
        self.model.as_deref()
    }

    fn process_line(&mut self, line: &str, output: &mut Vec<Vec<u8>>) -> Result<(), CompatError> {
        let Some(data) = line.strip_prefix("data:") else {
            return Ok(());
        };
        let data = data.trim();
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.emit_completion(output)?;
            return Ok(());
        }
        let value = serde_json::from_str::<Value>(data).map_err(|_| {
            CompatError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_upstream_response",
                "responses-native endpoint returned invalid streaming JSON",
            )
        })?;
        self.observe_metadata(&value);
        match value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "response.created" => self.ensure_created(output)?,
            "response.output_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    self.emit_text_delta(delta, output)?;
                }
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    self.emit_reasoning_delta(delta, output)?;
                }
            }
            "response.output_item.added" => {
                self.observe_output_item(&value, output)?;
            }
            "response.function_call_arguments.delta" => {
                self.observe_function_arguments_delta(&value, output)?;
            }
            "response.function_call_arguments.done" => {
                self.observe_function_arguments_done(&value, output)?;
            }
            "response.output_item.done" => {
                self.observe_output_item(&value, output)?;
            }
            "response.completed" => {
                if let Some(response) = value.get("response") {
                    self.observe_completed_response(response, output)?;
                }
                self.finish_reason = Some(if self.tool_calls.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                });
                self.emit_completion(output)?;
            }
            "response.incomplete" => {
                if let Some(response) = value.get("response") {
                    self.observe_completed_response(response, output)?;
                }
                self.finish_reason = Some("length");
                self.emit_completion(output)?;
            }
            "response.failed" | "error" => {
                self.emit_error(&value, output)?;
                self.finish_reason = Some("stop");
                self.emit_completion(output)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn observe_metadata(&mut self, value: &Value) {
        let response = value.get("response").unwrap_or(value);
        if self.response_id.is_none() {
            self.response_id = response
                .get("id")
                .or_else(|| value.get("id"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .map(str::to_string);
        }
        if self.model.is_none() {
            self.model = response
                .get("model")
                .or_else(|| value.get("model"))
                .and_then(Value::as_str)
                .filter(|model| !model.is_empty())
                .map(str::to_string);
        }
        if self.created_at.is_none() {
            self.created_at = response
                .get("created_at")
                .or_else(|| response.get("created"))
                .or_else(|| value.get("created_at"))
                .and_then(Value::as_i64);
        }
        if let Some(usage) = response.get("usage").or_else(|| value.get("usage")) {
            self.usage = Some(usage.clone());
        }
    }
}

#[path = "responses_to_chat_output.rs"]
mod responses_to_chat_output;

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::ResponsesChatResponseStreamAdapter;

    fn events(chunks: &[Vec<u8>]) -> Vec<Value> {
        chunks
            .iter()
            .filter_map(|chunk| {
                let text = String::from_utf8_lossy(chunk);
                let data = text.strip_prefix("data: ")?.trim();
                (data != "[DONE]").then(|| serde_json::from_str(data).unwrap())
            })
            .collect()
    }

    #[test]
    fn converts_text_and_usage_to_chat_sse() {
        let input = concat!(
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"m\"}}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":2,\"output_tokens\":3,\"total_tokens\":5}}}\n\n",
        );
        let mut adapter = ResponsesChatResponseStreamAdapter::new();
        let mut output = adapter.push_chunk(input.as_bytes()).unwrap();
        output.extend(adapter.finish().unwrap());
        let events = events(&output);
        assert_eq!(events[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(events[1]["choices"][0]["delta"]["content"], "hi");
        assert_eq!(events[2]["choices"][0]["finish_reason"], "stop");
        assert_eq!(events[2]["usage"]["prompt_tokens"], 2);
        assert!(output.iter().any(|chunk| chunk == b"data: [DONE]\n\n"));
    }

    #[test]
    fn converts_function_call_arguments_to_chat_tool_deltas() {
        let input = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"call_id\":\"call_1\",\"delta\":\"{\\\"q\\\":\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":1,\"call_id\":\"call_1\",\"delta\":\"1}\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":1,\"item_id\":\"call_1\",\"arguments\":\"{\\\"q\\\":1}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":1}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"{\\\"q\\\":1}\"}]}}\n\n",
        );
        let mut adapter = ResponsesChatResponseStreamAdapter::new();
        let mut output = adapter.push_chunk(input.as_bytes()).unwrap();
        output.extend(adapter.finish().unwrap());
        let events = events(&output);
        assert!(events.iter().any(|event| {
            event["choices"][0]["delta"]["tool_calls"][0]["function"]["name"] == "lookup"
        }));
        assert!(events.iter().any(|event| {
            event["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] == "{\"q\":"
        }));
        assert!(events.iter().any(|event| {
            event["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] == "1}"
        }));
        let argument_events = events
            .iter()
            .filter(|event| {
                event["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            })
            .count();
        assert_eq!(argument_events, 2);
        assert!(
            events
                .iter()
                .any(|event| event["choices"][0]["finish_reason"] == "tool_calls")
        );
    }

    #[test]
    fn keeps_multiple_tool_calls_separate_when_done_events_repeat_arguments() {
        let input = concat!(
            "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_a\",\"name\":\"first\"}}\n\n",
            "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_b\",\"name\":\"second\"}}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":0,\"call_id\":\"call_a\",\"delta\":\"{}\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.delta\",\"output_index\":2,\"call_id\":\"call_b\",\"delta\":\"{\\\"x\\\":1}\"}\n\n",
            "data: {\"type\":\"response.function_call_arguments.done\",\"output_index\":0,\"item_id\":\"call_a\",\"arguments\":\"{}\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"call_id\":\"call_b\",\"name\":\"second\",\"arguments\":\"{\\\"x\\\":1}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
        );
        let mut adapter = ResponsesChatResponseStreamAdapter::new();
        let mut output = adapter.push_chunk(input.as_bytes()).unwrap();
        output.extend(adapter.finish().unwrap());
        let events = events(&output);
        let tool_delta_events = events
            .iter()
            .filter_map(|event| event["choices"][0]["delta"]["tool_calls"].as_array())
            .flatten()
            .collect::<Vec<_>>();
        assert!(
            tool_delta_events
                .iter()
                .any(|call| { call["index"] == 0 && call["function"]["arguments"] == "{}" })
        );
        assert!(
            tool_delta_events
                .iter()
                .any(|call| { call["index"] == 1 && call["function"]["arguments"] == "{\"x\":1}" })
        );
        assert_eq!(
            tool_delta_events
                .iter()
                .filter(|call| {
                    call["function"]["arguments"]
                        .as_str()
                        .is_some_and(|arguments| !arguments.is_empty())
                })
                .count(),
            2
        );
    }
}
