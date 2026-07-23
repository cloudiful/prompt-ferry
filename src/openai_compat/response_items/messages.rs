use super::*;
use tracing::warn;

pub(crate) struct ResponseObjectBuilder {
    id: String,
    model: Option<String>,
    created_at: Option<i64>,
    output_items: Vec<Value>,
    usage: Option<Value>,
    status: String,
}

impl ResponseObjectBuilder {
    pub(crate) fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            model: None,
            created_at: None,
            output_items: Vec::new(),
            usage: None,
            status: "completed".to_string(),
        }
    }

    pub(crate) fn model(mut self, model: Option<&str>) -> Self {
        self.model = model.map(str::to_string);
        self
    }

    pub(crate) fn created_at(mut self, created_at: Option<i64>) -> Self {
        self.created_at = created_at;
        self
    }

    pub(crate) fn output_items(mut self, output_items: Vec<Value>) -> Self {
        self.output_items = output_items;
        self
    }

    pub(crate) fn usage(mut self, usage: Option<Value>) -> Self {
        self.usage = usage;
        self
    }

    pub(crate) fn status(mut self, status: impl Into<String>) -> Self {
        self.status = status.into();
        self
    }

    pub(crate) fn build(self) -> Value {
        let output_text = text_extract::output_text_from_items(&self.output_items);
        let mut response = response_shell(
            self.id,
            self.model.as_deref(),
            self.created_at,
            &self.status,
        );
        if let Some(object) = response.as_object_mut() {
            object.insert("output".to_string(), Value::Array(self.output_items));
            object.insert("output_text".to_string(), Value::String(output_text));
            object.insert(
                "completed_at".to_string(),
                if self.status == "completed" {
                    Value::from(ids::current_timestamp())
                } else {
                    Value::Null
                },
            );
            object.insert("usage".to_string(), self.usage.unwrap_or(Value::Null));
        }
        response
    }
}

pub(crate) fn build_response_object(
    id: impl Into<String>,
    model: Option<&str>,
    created_at: Option<i64>,
    output_items: Vec<Value>,
    usage: Option<Value>,
    status: &str,
) -> Value {
    ResponseObjectBuilder::new(id)
        .model(model)
        .created_at(created_at)
        .output_items(output_items)
        .usage(usage)
        .status(status)
        .build()
}

pub(crate) fn response_shell(
    id: impl Into<String>,
    model: Option<&str>,
    created_at: Option<i64>,
    status: &str,
) -> Value {
    let id = id.into();
    json!({
        "id": if id.is_empty() { generate_response_id() } else { id },
        "object": "response",
        "created_at": created_at.unwrap_or_else(ids::current_timestamp),
        "completed_at": Value::Null,
        "error": Value::Null,
        "incomplete_details": Value::Null,
        "metadata": {},
        "status": status,
        "model": model.unwrap_or("unknown"),
        "output": [],
        "output_text": "",
        "parallel_tool_calls": false,
        "store": false,
        "text": {
            "format": {
                "type": "text"
            }
        },
        "tool_choice": "auto",
        "truncation": "disabled",
        "usage": Value::Null,
    })
}

pub(crate) fn message_item_with_status(message_id: &str, text: &str, status: &str) -> Value {
    json!({
        "id": message_id,
        "type": "message",
        "status": status,
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": text,
            "annotations": [],
            "logprobs": [],
        }],
    })
}

pub(crate) fn reasoning_item_with_status(reasoning_id: &str, text: &str, status: &str) -> Value {
    json!({
        "id": reasoning_id,
        "type": "reasoning",
        "status": status,
        "summary": if text.is_empty() {
            Vec::<Value>::new()
        } else {
            vec![json!({
                "type": "summary_text",
                "text": text,
            })]
        },
        "content": [{
            "type": "reasoning_text",
            "text": text,
        }],
    })
}

pub(crate) fn function_call_item(
    call_id: &str,
    name: &str,
    arguments: &str,
    status: &str,
) -> Value {
    json!({
        "id": call_id,
        "type": "function_call",
        "status": status,
        "call_id": call_id,
        "name": name,
        "arguments": arguments,
    })
}

pub(crate) fn chat_output_items_from_response(value: &Value) -> Result<Vec<Value>, CompatError> {
    let mut items = Vec::new();
    if let Some(choices) = value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            if let Some(message) = choice.get("message") {
                items.extend(chat_output_items_from_message(message)?);
            }
        }
    }
    Ok(items)
}

pub(crate) fn chat_output_items_from_message(message: &Value) -> Result<Vec<Value>, CompatError> {
    let mut items = Vec::new();
    let assistant_text = message.get("content").map(extract_text).unwrap_or_default();
    let reasoning = extract_reasoning_text(message);
    if !reasoning.is_empty() {
        items.push(reasoning_item_with_status(
            &generate_reasoning_id(),
            &reasoning,
            "completed",
        ));
    }
    if !assistant_text.is_empty() {
        items.push(message_item_with_status(
            &generate_message_id(),
            &assistant_text,
            "completed",
        ));
    }
    for mut tool_call in chat_tool_calls_from_message(message)? {
        let (arguments, repair_status) =
            normalize_tool_call_arguments(&tool_call.name, &tool_call.arguments, &assistant_text)?;
        if repair_status == ToolCallArgumentRepairStatus::Repaired {
            warn!(
                tool_name = %tool_call.name,
                streaming = false,
                "repaired invalid upstream tool call arguments from assistant text"
            );
        }
        tool_call.arguments = arguments;
        items.push(function_call_item(
            &tool_call.call_id,
            &tool_call.name,
            &tool_call.arguments,
            "completed",
        ));
    }
    Ok(items)
}

fn chat_tool_calls_from_message(
    message: &Value,
) -> Result<Vec<tool_calls::FunctionToolCall>, CompatError> {
    tool_calls::chat_tool_calls_from_message(message)
}
