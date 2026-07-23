use http::StatusCode;
use serde_json::Value;

use super::CompatError;

#[derive(Debug, Clone)]
pub(crate) struct StreamToolCallState {
    pub call_id: String,
    pub name: Option<String>,
    pub arguments: String,
    pub output_index: usize,
    pub added_emitted: bool,
    pub done_emitted: bool,
}

impl StreamToolCallState {
    pub fn new(call_id: String, output_index: usize) -> Self {
        Self {
            call_id,
            name: None,
            arguments: String::new(),
            output_index,
            added_emitted: false,
            done_emitted: false,
        }
    }
}

pub(crate) fn sse_event(value: &Value) -> Result<Vec<u8>, CompatError> {
    let json = serde_json::to_string(value).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode translated streaming event",
        )
    })?;
    Ok(format!("data: {json}\n\n").into_bytes())
}
