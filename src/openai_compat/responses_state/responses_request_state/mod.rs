use super::*;

mod tests;
mod translate;
mod validate;

#[derive(Debug, Clone)]
pub(crate) struct NormalizedResponsesRequest {
    object: Map<String, Value>,
    pub(crate) instructions: Option<String>,
    pub(crate) conversation: Option<String>,
    pub(crate) items: Vec<Value>,
}

pub(crate) fn raw_responses_input_items_from_body(body: &[u8]) -> Result<Vec<Value>, CompatError> {
    let value = serde_json::from_slice::<Value>(body).map_err(|_| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "request body must be valid JSON",
        )
    })?;
    let object = value.as_object().cloned().ok_or_else(|| {
        CompatError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "responses request must be a JSON object",
        )
    })?;
    input_items_from_object(&object)
}

pub(crate) fn validate_raw_responses_request_body(body: &[u8]) -> Result<(), CompatError> {
    let items = raw_responses_input_items_from_body(body)?;
    validate::validate_raw_responses_passthrough(&items)
}

impl NormalizedResponsesRequest {
    pub(crate) fn from_body(body: &[u8]) -> Result<Self, CompatError> {
        let value = serde_json::from_slice::<Value>(body).map_err(|_| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "request body must be valid JSON",
            )
        })?;
        let object = value.as_object().cloned().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "responses request must be a JSON object",
            )
        })?;
        let mut items = input_items_from_object(&object)?;
        let conversation = object
            .get("conversation")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let instructions = normalize_instruction_messages(
            object
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            &mut items,
        )?;

        Ok(Self {
            object,
            instructions,
            conversation,
            items,
        })
    }

    pub(crate) fn validate_for_raw_responses_passthrough(&self) -> Result<(), CompatError> {
        validate::validate_raw_responses_passthrough(&self.items)
    }

    pub(crate) fn validate_for_chat_compat(
        &self,
        prior_call_ids: &HashSet<String>,
        has_replay_context: bool,
    ) -> Result<(), CompatError> {
        validate::validate_for_chat_compat(&self.items, prior_call_ids, has_replay_context)
    }

    pub(crate) fn to_chat_request_with_prefix(
        &self,
        prefix_messages: &[Value],
    ) -> Result<Vec<u8>, CompatError> {
        translate::to_chat_request_with_prefix(self, prefix_messages)
    }

    pub(crate) fn to_responses_request_with_prefix(
        &self,
        prefix_items: &[Value],
        drop_item_references: bool,
        drop_conversation: bool,
    ) -> Result<Vec<u8>, CompatError> {
        translate::to_responses_request_with_prefix(
            self,
            prefix_items,
            drop_item_references,
            drop_conversation,
        )
    }

    fn chat_compat_instructions(&self) -> Result<Option<String>, CompatError> {
        translate::chat_compat_instructions(self.instructions.as_deref(), &self.items)
    }

    fn chat_compat_items(&self) -> Vec<Value> {
        translate::chat_compat_items(&self.items)
    }
}
