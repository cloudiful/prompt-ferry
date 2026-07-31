use crate::{redact, worker_admin::AdminState};
use anyhow::Error;
use serde_json::Value;

pub(super) struct PassthroughSseFilter {
    pending: Vec<u8>,
    current_event: Vec<u8>,
    terminal: Option<PassthroughSseTerminal>,
    responses_error_body: Option<String>,
    responses_terminal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResponsesSseTerminal {
    Completed,
    Failed,
    Incomplete,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassthroughSseTerminal {
    LegacyDone,
    Responses(ResponsesSseTerminal),
}

impl PassthroughSseFilter {
    pub(super) fn new() -> Self {
        Self::with_responses_terminal(false)
    }

    pub(super) fn new_responses() -> Self {
        Self::with_responses_terminal(true)
    }

    fn with_responses_terminal(responses_terminal: bool) -> Self {
        Self {
            pending: Vec::new(),
            current_event: Vec::new(),
            terminal: None,
            responses_error_body: None,
            responses_terminal,
        }
    }

    pub(super) fn push_chunk(
        &mut self,
        chunk: &[u8],
    ) -> Result<Vec<Vec<u8>>, std::convert::Infallible> {
        if self.terminal.is_some() {
            return Ok(Vec::new());
        }
        self.pending.extend_from_slice(chunk);
        Ok(self.drain_events(false))
    }

    pub(super) fn finish(&mut self) -> Result<Vec<Vec<u8>>, std::convert::Infallible> {
        if self.terminal.is_some() {
            return Ok(Vec::new());
        }
        let mut output = self.drain_events(true);
        if self.terminal.is_none() {
            if !self.pending.is_empty() {
                self.current_event.extend_from_slice(&self.pending);
                self.pending.clear();
            }
            if !self.current_event.is_empty() {
                output.push(std::mem::take(&mut self.current_event));
            }
        }
        Ok(output)
    }

    pub(super) fn is_done(&self) -> bool {
        self.terminal.is_some()
    }

    pub(super) fn responses_terminal(&self) -> Option<ResponsesSseTerminal> {
        match self.terminal {
            Some(PassthroughSseTerminal::Responses(terminal)) => Some(terminal),
            _ => None,
        }
    }

    pub(super) fn responses_error_body(&self) -> Option<&str> {
        self.responses_error_body.as_deref()
    }

    fn drain_events(&mut self, finalize: bool) -> Vec<Vec<u8>> {
        let mut output = Vec::new();
        while let Some(line) = self.take_next_line(finalize) {
            if self.terminal.is_some() {
                self.current_event.clear();
                self.pending.clear();
                break;
            }
            let is_blank = line_terminator_trimmed(&line).is_empty();
            self.current_event.extend_from_slice(&line);
            if is_blank {
                let event = std::mem::take(&mut self.current_event);
                if let Some((terminal, error_body)) =
                    event_contains_terminal(&event, self.responses_terminal)
                {
                    self.terminal = Some(terminal);
                    self.responses_error_body = error_body;
                }
                output.push(event);
                if self.terminal.is_some() {
                    self.pending.clear();
                    break;
                }
            }
        }
        output
    }

    fn take_next_line(&mut self, finalize: bool) -> Option<Vec<u8>> {
        let mut index = 0usize;
        while index < self.pending.len() {
            match self.pending[index] {
                b'\n' => return Some(self.pending.drain(..=index).collect()),
                b'\r' => {
                    if index + 1 == self.pending.len() && !finalize {
                        return None;
                    }
                    let end = if self.pending.get(index + 1) == Some(&b'\n') {
                        index + 1
                    } else {
                        index
                    };
                    return Some(self.pending.drain(..=end).collect());
                }
                _ => index += 1,
            }
        }

        if finalize && !self.pending.is_empty() {
            return Some(std::mem::take(&mut self.pending));
        }

        None
    }
}

fn line_terminator_trimmed(line: &[u8]) -> &[u8] {
    if let Some(stripped) = line.strip_suffix(b"\r\n") {
        stripped
    } else if let Some(stripped) = line.strip_suffix(b"\n") {
        stripped
    } else if let Some(stripped) = line.strip_suffix(b"\r") {
        stripped
    } else {
        line
    }
}

fn event_contains_terminal(
    event: &[u8],
    responses_terminal: bool,
) -> Option<(PassthroughSseTerminal, Option<String>)> {
    let mut event_name_is_error = false;
    let mut error_value = None;
    let mut error_data = None;
    for segment in event.split(|byte| *byte == b'\n') {
        let trimmed = segment.strip_suffix(b"\r").unwrap_or(segment);
        if let Some(name) = trimmed.strip_prefix(b"event:") {
            event_name_is_error = String::from_utf8_lossy(name).trim() == "error";
            continue;
        }
        if trimmed == b"data: [DONE]" {
            return Some((
                if responses_terminal {
                    PassthroughSseTerminal::Responses(ResponsesSseTerminal::Completed)
                } else {
                    PassthroughSseTerminal::LegacyDone
                },
                None,
            ));
        }
        let Some(data) = trimmed.strip_prefix(b"data:") else {
            continue;
        };
        if !responses_terminal {
            continue;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(data) {
            let terminal = match value.get("type").and_then(Value::as_str) {
                Some("response.completed") => Some(ResponsesSseTerminal::Completed),
                Some("response.failed") => Some(ResponsesSseTerminal::Failed),
                Some("response.incomplete") => Some(ResponsesSseTerminal::Incomplete),
                Some("error") => Some(ResponsesSseTerminal::Error),
                _ => None,
            };
            if let Some(terminal) = terminal {
                let error_body =
                    (terminal == ResponsesSseTerminal::Error).then(|| format_json_value(&value));
                return Some((PassthroughSseTerminal::Responses(terminal), error_body));
            }
            if event_name_is_error {
                error_value = Some(value);
            }
        } else if event_name_is_error {
            error_data = Some(String::from_utf8_lossy(data).trim().to_string());
        }
    }
    if responses_terminal && event_name_is_error {
        return Some((
            PassthroughSseTerminal::Responses(ResponsesSseTerminal::Error),
            error_value
                .map(|value| format_json_value(&value))
                .or(error_data),
        ));
    }
    None
}

pub(super) fn format_json_value(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| String::from_utf8_lossy(value.to_string().as_bytes()).to_string())
}

pub(super) fn format_mcp_response_body(body: &[u8]) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(body).to_string();
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        return Some(format_json_value(&value));
    }
    Some(text)
}

pub(super) fn format_response_raw_body(body: &[u8]) -> Option<String> {
    format_mcp_response_body(body)
}

fn mcp_error_code_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn fallback_mcp_error_code(status: u16) -> &'static str {
    match status {
        400 => "bad_request",
        404 => "session_not_found",
        405 => "method_not_allowed",
        _ => "mcp_error",
    }
}

pub(super) fn extract_mcp_error(status: u16, body: &[u8]) -> (String, String) {
    if let Ok(value) = serde_json::from_slice::<Value>(body)
        && let Some(error) = value.get("error")
    {
        let mut code = error
            .get("code")
            .and_then(mcp_error_code_value)
            .unwrap_or_else(|| fallback_mcp_error_code(status).to_string());
        if status == 404 && code == "mcp_error" {
            code = "session_not_found".to_string();
        }
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                let fallback = String::from_utf8_lossy(body).trim().to_string();
                if fallback.is_empty() {
                    "mcp request failed".to_string()
                } else {
                    fallback
                }
            });
        return (code, message);
    }

    let text = String::from_utf8_lossy(body).trim().to_string();
    (
        fallback_mcp_error_code(status).to_string(),
        if text.is_empty() {
            "mcp request failed".to_string()
        } else {
            text
        },
    )
}

pub(super) fn maybe_redact_text(text: &str, redact_enabled: bool, user_id: Option<i64>) -> String {
    if redact_enabled {
        redact::redact_text_for_user(text, user_id)
    } else {
        text.to_string()
    }
}

pub(super) fn safe_error(err: &Error, redact_enabled: bool, user_id: Option<i64>) -> String {
    let message = maybe_redact_text(&format!("{err:#}"), redact_enabled, user_id);
    redact::truncate(&message, 240)
}

pub(super) fn http_error_message(status: u16, body: Option<&str>) -> String {
    let hint = match status {
        524 => {
            "upstream timeout after connection established, often emitted by Cloudflare or another proxy"
        }
        408 | 504 => "upstream timed out",
        429 => "upstream rate limited the request",
        500..=599 => "upstream server error",
        400..=499 => "upstream rejected the request",
        _ => "upstream returned non-success HTTP status",
    };
    match body.map(str::trim).filter(|body| !body.is_empty()) {
        Some(body) => format!(
            "HTTP {status}: {hint}; body: {}",
            redact::truncate(body, 240)
        ),
        None => format!("HTTP {status}: {hint}"),
    }
}

pub(super) fn redaction_enabled(admin_state: Option<&AdminState>) -> bool {
    admin_state.is_some_and(|state| {
        state
            .redaction_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    })
}

#[cfg(test)]
mod tests {
    use super::PassthroughSseFilter;

    #[test]
    fn preserves_complete_sse_events_without_reframing_lines() {
        let event = concat!(
            "event: response.output_text.delta\r\n",
            ": keep-alive\r\n",
            "data: hello\r\n",
            "data: world\r\n",
            "\r\n"
        );
        let bytes = event.as_bytes();
        let split = event.find("hello").unwrap() + "hel".len();
        let mut filter = PassthroughSseFilter::new();

        let first = filter.push_chunk(&bytes[..split]).unwrap();
        assert!(first.is_empty());

        let second = filter.push_chunk(&bytes[split..]).unwrap();
        assert_eq!(second, vec![bytes.to_vec()]);
    }

    #[test]
    fn waits_for_split_crlf_and_utf8_before_emitting_event() {
        let event = "event: response.created\r\ndata: 你好\r\n\r\n";
        let bytes = event.as_bytes();
        let split = bytes
            .windows("你".len())
            .position(|window| window == "你".as_bytes())
            .unwrap()
            + 1;
        let tail_split = bytes.len() - 1;
        let mut filter = PassthroughSseFilter::new();

        assert!(filter.push_chunk(&bytes[..split]).unwrap().is_empty());
        assert!(
            filter
                .push_chunk(&bytes[split..tail_split])
                .unwrap()
                .is_empty()
        );

        let output = filter.push_chunk(&bytes[tail_split..]).unwrap();
        assert_eq!(output, vec![bytes.to_vec()]);
    }

    #[test]
    fn stops_after_done_event_without_forwarding_trailing_bytes() {
        let mut filter = PassthroughSseFilter::new();
        let output = filter
            .push_chunk(b"data: [DONE]\n\nevent: ignored\ndata: later\n\n")
            .unwrap();

        assert_eq!(output, vec![b"data: [DONE]\n\n".to_vec()]);
        assert!(filter.finish().unwrap().is_empty());
    }
}
