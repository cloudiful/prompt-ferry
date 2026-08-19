use super::capture::{AssistantArtifactCapture, ResponsesArtifactCapture, fallback_text_artifact};

#[test]
fn captures_non_stream_reasoning_content() {
    let mut capture = AssistantArtifactCapture::new(false);
    capture.observe_chunk(
        br#"{"choices":[{"message":{"content":"hello","reasoning_content":"hidden","tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{}"}}]}}]}"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["content"].as_str(),
        Some("hello")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("hidden")
    );
    assert!(artifact.has_reasoning_content);
    assert!(artifact.has_tool_calls);
}

#[test]
fn captures_non_stream_reasoning_details() {
    let mut capture = AssistantArtifactCapture::new(false);
    capture.observe_chunk(
        br#"{"choices":[{"message":{"content":"hello","reasoning_details":[{"text":"hidden"}]}}]}"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("hidden")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_details"][0]["text"].as_str(),
        Some("hidden")
    );
    assert!(artifact.has_reasoning_content);
}

#[test]
fn captures_streaming_split_reasoning_content() {
    let mut capture = AssistantArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"choices":[{"delta":{"reasoning_content":"rea"}}]}
data: {"choices":[{"delta":{"reasoning_content":"son"}}]}
data: {"choices":[{"delta":{"content":"done"}}]}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["content"].as_str(),
        Some("done")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("reason")
    );
}

#[test]
fn captures_streaming_reasoning_details() {
    let mut capture = AssistantArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"choices":[{"delta":{"reasoning_details":[{"text":"rea"}]}}]}
data: {"choices":[{"delta":{"reasoning_details":[{"text":"son"}]}}]}
data: {"choices":[{"delta":{"content":"done"}}]}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("reason")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_details"][0]["text"].as_str(),
        Some("reason")
    );
}

#[test]
fn captures_streaming_tool_calls_and_reasoning() {
    let mut capture = AssistantArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"ci"}}],"reasoning_content":"h"}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"ty\":\"Bos"}}],"reasoning_content":"i"}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"ton\"}"}}]}}]}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["tool_calls"][0]["function"]["arguments"]
            .as_str(),
        Some("{\"city\":\"Boston\"}")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("hi")
    );
}

#[test]
fn captures_streaming_tool_calls_when_provider_reuses_index() {
    let mut capture = AssistantArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","type":"function","function":{"name":"first_tool","arguments":"{"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"}"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_b","type":"function","function":{"name":"second_tool","arguments":"{"}}]}}]}
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"type":"function","function":{"arguments":"}"}}]}}]}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    let tool_calls = artifact.message_json["assistant_message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(tool_calls.len(), 2);
    assert_eq!(tool_calls[0]["id"].as_str(), Some("call_a"));
    assert_eq!(
        tool_calls[0]["function"]["name"].as_str(),
        Some("first_tool")
    );
    assert_eq!(tool_calls[0]["function"]["arguments"].as_str(), Some("{}"));
    assert_eq!(tool_calls[1]["id"].as_str(), Some("call_b"));
    assert_eq!(
        tool_calls[1]["function"]["name"].as_str(),
        Some("second_tool")
    );
    assert_eq!(tool_calls[1]["function"]["arguments"].as_str(), Some("{}"));
}

#[test]
fn captures_repaired_streaming_tool_call_arguments_in_artifact() {
    let mut capture = AssistantArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"choices":[{"delta":{"content":"<tool_call>\n<function=search_stocks>\n<parameter=query>\u6b63\u6cf0\u7535\u6e90</parameter>\n<parameter=limit>5</parameter>\n</function>\n</tool_call>","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"search_stocks","arguments":"{\"query\": "}}]}}]}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["tool_calls"][0]["function"]["arguments"]
            .as_str(),
        Some("{\"limit\":5,\"query\":\"正泰电源\"}")
    );
    let function_call = artifact.message_json["output_items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"].as_str() == Some("function_call"))
        .unwrap();
    assert_eq!(
        function_call["arguments"].as_str(),
        Some("{\"limit\":5,\"query\":\"正泰电源\"}")
    );
}

#[test]
fn captures_responses_reasoning_with_tool_calls_in_one_assistant_message() {
    let mut capture = ResponsesArtifactCapture::new(false);
    capture.observe_chunk(
        br#"{"output":[
            {"type":"reasoning","summary":[{"type":"summary_text","text":"short summary"}],"content":[{"type":"reasoning_text","text":"complete reasoning"}]},
            {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}
        ]}"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["role"].as_str(),
        Some("assistant")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("complete reasoning")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
    assert!(artifact.has_reasoning_content);
    assert!(artifact.has_tool_calls);
}

#[test]
fn does_not_turn_responses_summary_into_reasoning_content() {
    let mut capture = ResponsesArtifactCapture::new(false);
    capture.observe_chunk(
        br#"{"output":[
            {"type":"reasoning","summary":[{"type":"summary_text","text":"summary only"}]},
            {"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{}"}
        ]}"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert!(
        artifact.message_json["assistant_message"]
            .get("reasoning_content")
            .is_none()
    );
    assert!(!artifact.has_reasoning_content);
    assert!(artifact.has_tool_calls);
}

#[test]
fn captures_streaming_responses_reasoning_delta_with_tool_call() {
    let mut capture = ResponsesArtifactCapture::new(true);
    capture.observe_chunk(
        br#"data: {"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","content":[{"type":"reasoning_text","text":""}]}}
data: {"type":"response.reasoning_text.delta","output_index":0,"delta":"complete reasoning"}
data: {"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","call_id":"call_1","name":"lookup","arguments":""}}
data: {"type":"response.function_call_arguments.delta","output_index":1,"call_id":"call_1","delta":"{}"}
"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["reasoning_content"].as_str(),
        Some("complete reasoning")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["tool_calls"][0]["id"].as_str(),
        Some("call_1")
    );
}

#[test]
fn captures_refusal_and_phase_for_replay_guarding() {
    let mut capture = AssistantArtifactCapture::new(false);
    capture.observe_chunk(
        br#"{"choices":[{"message":{"content":null,"refusal":"cannot help","phase":"analysis"}}]}"#,
    );
    capture.finish();

    let artifact = capture.artifact().unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["refusal"].as_str(),
        Some("cannot help")
    );
    assert_eq!(
        artifact.message_json["assistant_message"]["phase"].as_str(),
        Some("analysis")
    );
}

#[test]
fn builds_fallback_text_artifact() {
    let artifact = fallback_text_artifact("done").unwrap();
    assert_eq!(
        artifact.message_json["assistant_message"]["content"].as_str(),
        Some("done")
    );
    assert_eq!(
        artifact.message_json["output_items"][0]["type"].as_str(),
        Some("message")
    );
    assert!(!artifact.has_reasoning_content);
    assert!(!artifact.has_tool_calls);
}
