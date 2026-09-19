pub(super) fn should_process_ai_string_field(
    request_path: &str,
    json_path: &str,
    object_type: Option<&str>,
    key: &str,
) -> bool {
    match request_path {
        "/v1/chat/completions" => should_process_chat_string_field(json_path, key),
        "/v1/responses" | "/v1/responses/compact" => {
            should_process_responses_string_field(json_path, object_type, key)
        }
        "/v1/messages" => should_process_anthropic_string_field(json_path, object_type, key),
        _ => false,
    }
}

fn should_process_anthropic_string_field(
    json_path: &str,
    object_type: Option<&str>,
    key: &str,
) -> bool {
    (key == "text" && (json_path.contains("/messages/") || json_path == "/system"))
        || (key == "thinking" && json_path.contains("/messages/"))
        || (key == "content" && json_path.contains("/messages/"))
        || (key == "input"
            && json_path.contains("/messages/")
            && json_path.contains("/content/")
            && object_type == Some("tool_use"))
}

fn is_reasoning_field(key: &str) -> bool {
    matches!(
        key,
        "reasoning_content"
            | "reasoning"
            | "reasoning_text"
            | "reasoning_details"
            | "thinking"
            | "chain_of_thought"
            | "summary"
    )
}

fn should_process_chat_string_field(json_path: &str, key: &str) -> bool {
    (key == "content" && json_path.contains("/messages/"))
        || (key == "text" && json_path.contains("/messages/") && json_path.contains("/content/"))
        || (key == "arguments"
            && (json_path.ends_with("/function")
                || json_path.ends_with("/function_call")
                || json_path.contains("/tool_calls/")))
        || (key == "output" && json_path.contains("/messages/"))
        || is_reasoning_field(key)
        || (key == "text" && json_path.contains("/reasoning_details/"))
}

fn should_process_responses_string_field(
    json_path: &str,
    object_type: Option<&str>,
    key: &str,
) -> bool {
    match key {
        "instructions" => json_path == "/instructions",
        "input" => json_path == "/input",
        "text" => matches!(
            object_type,
            Some("input_text")
                | Some("output_text")
                | Some("summary_text")
                | Some("reasoning_text")
                | Some("refusal")
        ),
        "content" => json_path.contains("/input/") || json_path.contains("/output/"),
        "arguments" => object_type == Some("function_call"),
        "output" => object_type == Some("function_call_output"),
        "summary" => object_type == Some("reasoning"),
        _ => is_reasoning_field(key),
    }
}

#[cfg(test)]
mod tests {
    use super::should_process_ai_string_field;

    #[test]
    fn matches_top_level_responses_instructions_and_input() {
        assert!(should_process_ai_string_field(
            "/v1/responses",
            "/instructions",
            None,
            "instructions",
        ));
        assert!(should_process_ai_string_field(
            "/v1/responses",
            "/input",
            None,
            "input",
        ));
    }

    #[test]
    fn matches_nested_chat_tool_call_arguments_but_not_name() {
        assert!(should_process_ai_string_field(
            "/v1/chat/completions",
            "/messages/1/tool_calls/0/function",
            None,
            "arguments",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/chat/completions",
            "/messages/1/tool_calls/0/function",
            None,
            "name",
        ));
    }

    #[test]
    fn only_matches_anthropic_tool_use_input() {
        assert!(should_process_ai_string_field(
            "/v1/messages",
            "/messages/0/content/1/input",
            Some("tool_use"),
            "input",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/messages",
            "/messages/0/metadata/input",
            None,
            "input",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/messages",
            "/messages/0/content/1/input",
            Some("text"),
            "input",
        ));
    }

    #[test]
    fn reasoning_fields_are_redacted() {
        for path in [
            "/messages/0/reasoning_content",
            "/messages/0/reasoning",
            "/messages/0/reasoning_text",
            "/messages/0/thinking",
            "/messages/0/chain_of_thought",
            "/messages/0/summary",
            "/choices/0/message/reasoning_content",
        ] {
            let key = path.rsplit('/').next().expect("field name");
            assert!(
                should_process_ai_string_field("/v1/chat/completions", path, None, key),
                "chat field {path} must be redacted",
            );
        }
        assert!(should_process_ai_string_field(
            "/v1/chat/completions",
            "/messages/0/reasoning_details/0/text",
            None,
            "text",
        ));
        assert!(should_process_ai_string_field(
            "/v1/chat/completions",
            "/choices/0/delta/reasoning_details/0/text",
            None,
            "text",
        ));

        for (path, object_type, key) in [
            ("/output/0/content/0/text", Some("reasoning_text"), "text"),
            ("/output/0/summary", Some("reasoning"), "summary"),
            (
                "/input/0/reasoning_content",
                Some("reasoning"),
                "reasoning_content",
            ),
            (
                "/input/0/reasoning_text",
                Some("reasoning_text"),
                "reasoning_text",
            ),
            ("/input/0/thinking", None, "thinking"),
            ("/input/0/chain_of_thought", None, "chain_of_thought"),
        ] {
            assert!(
                should_process_ai_string_field("/v1/responses", path, object_type, key),
                "responses field {path} must be redacted",
            );
        }

        assert!(!should_process_ai_string_field(
            "/v1/responses",
            "/reasoning/summary",
            None,
            "summary",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/responses",
            "/reasoning/effort",
            None,
            "effort",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/responses",
            "/input/0/type",
            Some("reasoning"),
            "type",
        ));
        assert!(!should_process_ai_string_field(
            "/v1/chat/completions",
            "/model",
            None,
            "model",
        ));
    }
}
