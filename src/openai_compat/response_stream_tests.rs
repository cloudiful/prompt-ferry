use super::response_stream::{AnthropicResponseStreamAdapter, ChatResponseStreamAdapter};
use http::StatusCode;
use serde_json::Value;

#[test]
fn translates_chat_stream_to_responses_events() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let mut output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"delta":{"content":"hel"}}]}

"#)
        .unwrap();
    output.extend(
        adapter
            .push_chunk(
                br#"data: {"choices":[{"delta":{"content":"lo"}}]}
data: {"choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}
data: [DONE]

"#,
            )
            .unwrap(),
    );

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    assert!(text.contains("\"type\":\"response.created\""));
    assert!(text.contains("\"type\":\"response.output_text.delta\""));
    assert!(text.contains("\"type\":\"response.completed\""));
    assert!(text.contains("\"delta\":\"hel\""));
    assert!(text.contains("\"delta\":\"lo\""));
    assert_eq!(events[0]["sequence_number"].as_u64(), Some(0));
    assert_eq!(events[1]["sequence_number"].as_u64(), Some(1));
    assert_eq!(events[2]["sequence_number"].as_u64(), Some(2));
    assert!(
        events
            .iter()
            .all(|event| event.get("sequence_number").is_some())
    );
    assert_eq!(
        events[0]["response"]["output"].as_array().map(Vec::len),
        Some(0)
    );
    assert_eq!(
        events
            .last()
            .and_then(|event| event.get("response"))
            .and_then(|response| response.get("usage"))
            .and_then(|usage| usage.get("output_tokens_details"))
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64),
        Some(0)
    );
}

#[test]
fn leaves_chat_stream_usage_null_when_upstream_omits_usage() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"delta":{"content":"hello"}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();

    assert!(completed["response"]["usage"].is_null());
}

#[test]
fn translates_tool_call_stream_to_responses_events() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"gpt-test","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Bos"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"ton\"}"}}]}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    assert!(text.contains("\"type\":\"response.output_item.added\""));
    assert!(text.contains("\"type\":\"response.function_call_arguments.delta\""));
    assert!(text.contains("\"type\":\"response.function_call_arguments.done\""));
    assert!(text.contains("\"type\":\"response.completed\""));
    assert!(text.contains("\"call_id\":\"call_1\""));
    assert!(text.contains("\"name\":\"get_weather\""));
    assert!(
        events
            .iter()
            .all(|event| event.get("sequence_number").is_some())
    );
    assert!(
        events
            .iter()
            .enumerate()
            .all(|(index, event)| event["sequence_number"].as_u64() == Some(index as u64))
    );
}

#[test]
fn translates_tool_call_stream_when_provider_reuses_index() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"mimo-test","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"first_tool","arguments":"{"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"}"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_b","type":"function","function":{"name":"second_tool","arguments":"{"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"}"}}]}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    let output_items = completed["response"]["output"].as_array().unwrap();
    let function_calls = output_items
        .iter()
        .filter(|item| item["type"].as_str() == Some("function_call"))
        .collect::<Vec<_>>();
    assert_eq!(function_calls.len(), 2);
    assert_eq!(function_calls[0]["call_id"].as_str(), Some("call_a"));
    assert_eq!(function_calls[0]["name"].as_str(), Some("first_tool"));
    assert_eq!(function_calls[0]["arguments"].as_str(), Some("{}"));
    assert_eq!(function_calls[1]["call_id"].as_str(), Some("call_b"));
    assert_eq!(function_calls[1]["name"].as_str(), Some("second_tool"));
    assert_eq!(function_calls[1]["arguments"].as_str(), Some("{}"));
}

#[test]
fn translates_reasoning_stream_to_responses_events() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"deepseek-test","choices":[{"delta":{"reasoning_content":"plan "}}]}
data: {"choices":[{"delta":{"reasoning_content":"steps","content":"answer"}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    assert!(text.contains("\"type\":\"response.reasoning_summary_part.added\""));
    assert!(text.contains("\"type\":\"response.reasoning_summary_text.delta\""));
    assert!(text.contains("\"type\":\"response.reasoning_summary_part.done\""));
    assert!(text.contains("\"type\":\"response.reasoning_text.delta\""));
    assert!(text.contains("\"type\":\"response.reasoning_text.done\""));
    assert!(text.contains("\"type\":\"reasoning\""));
    assert!(text.contains("\"type\":\"response.output_text.delta\""));
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    let output_items = completed["response"]["output"].as_array().unwrap();
    assert_eq!(output_items[0]["type"].as_str(), Some("reasoning"));
    assert_eq!(
        output_items[0]["content"][0]["text"].as_str(),
        Some("plan steps")
    );
    assert_eq!(
        output_items[0]["summary"][0]["text"].as_str(),
        Some("plan steps")
    );
    assert_eq!(output_items[1]["type"].as_str(), Some("message"));
    assert_eq!(
        completed["response"]["output_text"].as_str(),
        Some("answer")
    );
}

#[test]
fn translates_minimax_reasoning_details_stream_to_responses_events() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"MiniMax-M3","choices":[{"delta":{"reasoning_details":[{"text":"plan "}]}}]}
data: {"choices":[{"delta":{"reasoning_details":[{"text":"steps"}],"content":"answer"}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    assert_eq!(
        completed["response"]["output"][0]["content"][0]["text"].as_str(),
        Some("plan steps")
    );
}

#[test]
fn translates_reasoning_and_tool_call_stream_to_responses_events() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"deepseek-test","choices":[{"delta":{"reasoning_content":"need date ","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"get_date","arguments":"{"}}]}}]}
data: {"choices":[{"delta":{"reasoning_content":"then weather","tool_calls":[{"index":0,"type":"function","function":{"arguments":"}"}}]}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    let output_items = completed["response"]["output"].as_array().unwrap();
    let reasoning_item = output_items
        .iter()
        .find(|item| item["type"].as_str() == Some("reasoning"))
        .unwrap();
    let function_call_item = output_items
        .iter()
        .find(|item| item["type"].as_str() == Some("function_call"))
        .unwrap();
    assert_eq!(function_call_item["call_id"].as_str(), Some("call_1"));
    assert_eq!(
        reasoning_item["content"][0]["text"].as_str(),
        Some("need date then weather")
    );
    assert!(text.contains("\"type\":\"response.function_call_arguments.done\""));
}

#[test]
fn repairs_stream_tool_call_arguments_from_tool_call_markup() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"id":"chatcmpl_123","created":123,"model":"mimo-test","choices":[{"delta":{"content":"<tool_call>\n<function=search_stocks>\n<parameter=query>\u6b63\u6cf0\u7535\u6e90</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"search_stocks","arguments":"{\"query\": "}}]}}]}
data: [DONE]

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let done = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.function_call_arguments.done"))
        .unwrap();
    assert_eq!(
        done["arguments"].as_str(),
        Some("{\"limit\":5,\"query\":\"正泰电源\"}")
    );
}

#[test]
fn rejects_unrepairable_stream_tool_call_arguments() {
    let mut adapter = ChatResponseStreamAdapter::new();
    let err = adapter
        .push_chunk(
            br#"data: {"id":"chatcmpl_123","created":123,"model":"mimo-test","choices":[{"delta":{"content":"plain text","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"search_stocks","arguments":"{\"query\": "}}]}}]}
data: [DONE]

"#,
        )
        .unwrap_err();

    assert_eq!(err.status, StatusCode::BAD_GATEWAY);
    assert_eq!(err.code, "invalid_upstream_response");
}

#[test]
fn translates_anthropic_text_stream_to_responses_events() {
    let mut adapter = AnthropicResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"type":"message_start","message":{"id":"msg_123","model":"claude-sonnet-4-5","usage":{"input_tokens":2}}}
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hel"}}
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}
data: {"type":"message_delta","usage":{"output_tokens":3}}
data: {"type":"message_stop"}

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    assert!(text.contains("\"type\":\"response.created\""));
    assert!(text.contains("\"type\":\"response.output_text.delta\""));
    assert!(text.contains("\"type\":\"response.completed\""));
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    assert_eq!(completed["response"]["output_text"].as_str(), Some("hello"));
    assert_eq!(
        completed["response"]["usage"]["input_tokens"].as_i64(),
        Some(2)
    );
    assert_eq!(
        completed["response"]["usage"]["output_tokens"].as_i64(),
        Some(3)
    );
}

#[test]
fn leaves_anthropic_stream_usage_null_when_upstream_omits_usage() {
    let mut adapter = AnthropicResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"type":"message_start","message":{"id":"msg_123","model":"claude-sonnet-4-5"}}
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hello"}}
data: {"type":"message_stop"}

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();

    assert!(completed["response"]["usage"].is_null());
}

#[test]
fn translates_anthropic_thinking_and_tool_stream_to_responses_events() {
    let mut adapter = AnthropicResponseStreamAdapter::new();
    let output = adapter
        .push_chunk(br#"data: {"type":"message_start","message":{"id":"msg_123","model":"claude-sonnet-4-5"}}
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"plan "}}
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"steps"}}
data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}
data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"city\":\"Boston\""}}
data: {"type":"message_stop"}

"#)
        .unwrap();

    let text = String::from_utf8(output.concat()).unwrap();
    let events = parse_sse_events(&text);
    assert!(text.contains("\"type\":\"response.reasoning_text.delta\""));
    assert!(text.contains("\"type\":\"response.function_call_arguments.done\""));
    let completed = events
        .iter()
        .find(|event| event["type"].as_str() == Some("response.completed"))
        .unwrap();
    let output_items = completed["response"]["output"].as_array().unwrap();
    assert_eq!(output_items[0]["type"].as_str(), Some("reasoning"));
    assert_eq!(
        output_items[0]["content"][0]["text"].as_str(),
        Some("plan steps")
    );
    let function_call = output_items
        .iter()
        .find(|item| item["type"].as_str() == Some("function_call"))
        .unwrap();
    assert_eq!(function_call["call_id"].as_str(), Some("toolu_1"));
    assert_eq!(function_call["name"].as_str(), Some("lookup"));
    assert_eq!(
        function_call["arguments"].as_str(),
        Some("{\"city\":\"Boston\"}")
    );
}

fn parse_sse_events(text: &str) -> Vec<Value> {
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect()
}
