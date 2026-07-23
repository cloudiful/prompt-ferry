use http::StatusCode;
use serde_json::{Map, Number, Value};

use crate::openai_compat::CompatError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolCallArgumentRepairStatus {
    Unchanged,
    Repaired,
}

pub(crate) fn normalize_tool_call_arguments(
    tool_name: &str,
    arguments: &str,
    assistant_text: &str,
) -> Result<(String, ToolCallArgumentRepairStatus), CompatError> {
    if serde_json::from_str::<Value>(arguments).is_ok() {
        return Ok((
            arguments.to_string(),
            ToolCallArgumentRepairStatus::Unchanged,
        ));
    }

    let Some(params) = extract_tool_call_parameters(tool_name, assistant_text) else {
        return Err(CompatError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            format!("upstream returned invalid tool call arguments for `{tool_name}`"),
        ));
    };

    let repaired = serde_json::to_string(&Value::Object(params)).map_err(|_| {
        CompatError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "adapter_error",
            "failed to encode repaired tool call arguments",
        )
    })?;
    Ok((repaired, ToolCallArgumentRepairStatus::Repaired))
}

fn extract_tool_call_parameters(
    tool_name: &str,
    assistant_text: &str,
) -> Option<Map<String, Value>> {
    let mut scan_from = 0;
    while let Some(start_offset) = assistant_text[scan_from..].find("<tool_call>") {
        let block_start = scan_from + start_offset;
        let after_start = block_start + "<tool_call>".len();
        let Some(end_offset) = assistant_text[after_start..].find("</tool_call>") else {
            break;
        };
        let block_end = after_start + end_offset;
        let block = &assistant_text[after_start..block_end];
        scan_from = block_end + "</tool_call>".len();

        let Some((name, body)) = parse_function_block(block) else {
            continue;
        };
        if name != tool_name {
            continue;
        }

        let params = parse_parameters(body);
        if !params.is_empty() {
            return Some(params);
        }
    }
    None
}

fn parse_function_block(block: &str) -> Option<(&str, &str)> {
    let function_prefix = "<function=";
    let start = block.find(function_prefix)?;
    let name_start = start + function_prefix.len();
    let name_end = block[name_start..].find('>')? + name_start;
    let name = block[name_start..name_end].trim();
    if name.is_empty() {
        return None;
    }
    let body_start = name_end + 1;
    let body_end = block[body_start..].find("</function>")? + body_start;
    Some((name, &block[body_start..body_end]))
}

fn parse_parameters(body: &str) -> Map<String, Value> {
    let mut params = Map::new();
    let mut scan_from = 0;
    let prefix = "<parameter=";
    while let Some(start_offset) = body[scan_from..].find(prefix) {
        let key_start = scan_from + start_offset + prefix.len();
        let Some(key_end_offset) = body[key_start..].find('>') else {
            break;
        };
        let key_end = key_start + key_end_offset;
        let key = body[key_start..key_end].trim();
        let value_start = key_end + 1;
        let Some(value_end_offset) = body[value_start..].find("</parameter>") else {
            break;
        };
        let value_end = value_start + value_end_offset;
        scan_from = value_end + "</parameter>".len();
        if key.is_empty() {
            continue;
        }
        params.insert(
            key.to_string(),
            parse_parameter_value(&body[value_start..value_end]),
        );
    }
    params
}

fn parse_parameter_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if trimmed.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if let Ok(value) = trimmed.parse::<i64>() {
        return Value::Number(value.into());
    }
    if let Ok(value) = trimmed.parse::<u64>() {
        return Value::Number(value.into());
    }
    if let Ok(value) = trimmed.parse::<f64>()
        && let Some(number) = Number::from_f64(value)
    {
        return Value::Number(number);
    }
    serde_json::from_str::<Value>(trimmed).unwrap_or_else(|_| Value::String(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use http::StatusCode;

    use super::{ToolCallArgumentRepairStatus, normalize_tool_call_arguments};

    #[test]
    fn repairs_arguments_from_tool_call_markup() {
        let (repaired, status) = normalize_tool_call_arguments(
            "search_stocks",
            "{\"query\": ",
            "<tool_call>\n<function=search_stocks>\n<parameter=query>正泰电源</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>",
        )
        .unwrap();

        assert_eq!(repaired, "{\"limit\":5,\"query\":\"正泰电源\"}");
        assert_eq!(status, ToolCallArgumentRepairStatus::Repaired);
    }

    #[test]
    fn rejects_unrepairable_invalid_arguments() {
        let err = normalize_tool_call_arguments("search_stocks", "{\"query\": ", "").unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_GATEWAY);
        assert_eq!(err.code, "invalid_upstream_response");
    }
}
