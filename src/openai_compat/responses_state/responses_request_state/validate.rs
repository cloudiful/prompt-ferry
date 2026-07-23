use super::*;

pub(super) fn validate_raw_responses_passthrough(items: &[Value]) -> Result<(), CompatError> {
    for item in items {
        let object = item.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses input items must be JSON objects",
            )
        })?;
        if matches!(item_kind(object)?, ItemKind::FunctionCallOutput) {
            let _ = required_string_field(
                object,
                &["call_id"],
                "function_call_output items require call_id",
            )?;
            if object.get("output").is_none() {
                return Err(CompatError::new(
                    StatusCode::BAD_REQUEST,
                    "unsupported_feature",
                    "function_call_output items require output",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_for_chat_compat(
    items: &[Value],
    prior_call_ids: &HashSet<String>,
    has_replay_context: bool,
) -> Result<(), CompatError> {
    let mut available_call_ids = prior_call_ids.clone();
    for item in items {
        let object = item.as_object().ok_or_else(|| {
            CompatError::new(
                StatusCode::BAD_REQUEST,
                "unsupported_feature",
                "responses input items must be JSON objects",
            )
        })?;
        match item_kind(object)? {
            ItemKind::RoleMessage(role) => match role {
                "user" | "assistant" => {}
                "system" | "developer" => {}
                "tool" => {
                    return Err(invalid_continuation(
                        "tool role messages are not supported in Responses input; use function_call_output items",
                    ));
                }
                other => {
                    return Err(CompatError::new(
                        StatusCode::BAD_REQUEST,
                        "unsupported_feature",
                        format!(
                            "responses role `{other}` is not supported for the compatibility subset"
                        ),
                    ));
                }
            },
            ItemKind::FunctionCall => {
                let call_id = required_string_field(
                    object,
                    &["call_id", "id"],
                    "function_call items require call_id",
                )?;
                available_call_ids.insert(call_id.to_string());
            }
            ItemKind::ItemReference => {
                let reference_id =
                    required_string_field(object, &["id"], "item_reference items require id")?;
                if !has_replay_context && !available_call_ids.contains(reference_id) {
                    return Err(invalid_continuation(format!(
                        "item_reference `{reference_id}` requires previous_response_id or a preceding function_call item"
                    )));
                }
            }
            ItemKind::FunctionCallOutput => {
                let call_id = required_string_field(
                    object,
                    &["call_id"],
                    "function_call_output items require call_id",
                )?;
                if !available_call_ids.contains(call_id) {
                    return Err(invalid_continuation(format!(
                        "function_call_output `{call_id}` requires previous_response_id or a preceding function_call item"
                    )));
                }
            }
            ItemKind::PartOnly => {}
        }
    }
    Ok(())
}
