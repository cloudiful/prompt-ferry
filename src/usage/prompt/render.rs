use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct RenderedPromptMessage {
    pub role: String,
    pub block_hash: String,
    pub preview_text: String,
    pub content_json: Value,
    pub same_as_turn: Option<i32>,
}

pub fn render_prompt_text(messages: &[RenderedPromptMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            if let Some(turn) = message.same_as_turn {
                format!("{}: same as turn #{turn}", message.role)
            } else {
                format!("{}: {}", message.role, message.preview_text)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
