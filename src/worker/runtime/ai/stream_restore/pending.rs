use anyhow::{Result, anyhow};
use serde_json::Value;

pub(super) fn synthetic_delta(mut template: Value, pointer: &str, text: String) -> Result<Vec<u8>> {
    blank_stream_text(&mut template);
    let target = template
        .pointer_mut(pointer)
        .ok_or_else(|| anyhow!("missing pending stream field `{pointer}`"))?;
    *target = Value::String(text);
    let json = serde_json::to_string(&template)?;
    let event_type = template.get("type").and_then(Value::as_str);
    Ok(match event_type {
        Some(event_type) if event_type.starts_with("response.") => {
            format!("event: {event_type}\ndata: {json}\n\n").into_bytes()
        }
        _ => format!("data: {json}\n\n").into_bytes(),
    })
}

fn blank_stream_text(value: &mut Value) {
    match value.get("type").and_then(Value::as_str) {
        Some("response.output_text.delta")
        | Some("response.reasoning_text.delta")
        | Some("response.function_call_arguments.delta") => {
            set_empty(value, "/delta");
            return;
        }
        Some("content_block_start") => {
            for pointer in [
                "/content_block/text",
                "/content_block/thinking",
                "/content_block/partial_json",
            ] {
                set_empty(value, pointer);
            }
            return;
        }
        Some("content_block_delta") => {
            for pointer in ["/delta/text", "/delta/thinking", "/delta/partial_json"] {
                set_empty(value, pointer);
            }
            return;
        }
        _ => {}
    }

    let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) else {
        return;
    };
    for choice in choices {
        let Some(delta) = choice.get_mut("delta") else {
            continue;
        };
        for field in ["content", "reasoning_content", "reasoning", "refusal"] {
            if delta.get(field).is_some_and(Value::is_string) {
                delta[field] = Value::String(String::new());
            }
        }
        if let Some(details) = delta
            .get_mut("reasoning_details")
            .and_then(Value::as_array_mut)
        {
            for detail in details {
                if detail.get("text").is_some_and(Value::is_string) {
                    detail["text"] = Value::String(String::new());
                }
            }
        }
        if let Some(tool_calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for tool_call in tool_calls {
                set_empty(tool_call, "/function/arguments");
            }
        }
    }
}

fn set_empty(value: &mut Value, pointer: &str) {
    if let Some(target) = value.pointer_mut(pointer)
        && target.is_string()
    {
        *target = Value::String(String::new());
    }
}
