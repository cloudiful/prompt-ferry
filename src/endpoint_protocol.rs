use std::time::Duration;

use reqwest::{Client, StatusCode};

use crate::config::{NativeApi, NativeApiSource};

#[derive(Debug, Clone, Copy)]
pub struct EndpointProtocolResolution {
    pub native_api: NativeApi,
    pub native_api_source: NativeApiSource,
}

pub fn endpoint_protocol_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest client config is valid")
}

pub async fn detect_endpoint_protocol(
    client: &Client,
    base_url: &str,
    api_key: &str,
) -> Result<NativeApi, String> {
    let anthropic = probe_native_api(client, base_url, api_key, NativeApi::AnthropicMessages).await;
    if anthropic.is_ok() {
        return Ok(NativeApi::AnthropicMessages);
    }
    let responses = probe_native_api(client, base_url, api_key, NativeApi::Responses).await;
    if responses.is_ok() {
        return Ok(NativeApi::Responses);
    }
    let chat = probe_native_api(client, base_url, api_key, NativeApi::Chat).await;
    if chat.is_ok() {
        return Ok(NativeApi::Chat);
    }
    Err(format!(
        "anthropic_messages probe: {}; responses probe: {}; chat probe: {}",
        anthropic.err().unwrap_or_else(|| "ok".to_string()),
        responses.err().unwrap_or_else(|| "ok".to_string()),
        chat.err().unwrap_or_else(|| "ok".to_string())
    ))
}

pub async fn probe_native_api(
    client: &Client,
    base_url: &str,
    api_key: &str,
    native_api: NativeApi,
) -> Result<(), String> {
    if matches!(native_api, NativeApi::Realtime) {
        return Err("realtime protocol detection is not supported".to_string());
    }
    let base = base_url.trim_end_matches('/');
    let payload = match native_api {
        NativeApi::AnthropicMessages => serde_json::json!({
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
        }),
        NativeApi::Responses => serde_json::json!({
            "model": "gpt-4.1",
            "input": "ping",
            "max_output_tokens": 1,
        }),
        NativeApi::Chat => serde_json::json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1,
        }),
        NativeApi::Realtime => unreachable!(),
    };
    let request = client
        .post(format!("{base}{}", native_api.path()))
        .json(&payload);
    let request = match native_api {
        NativeApi::AnthropicMessages => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        NativeApi::Chat | NativeApi::Responses => request.bearer_auth(api_key),
        NativeApi::Realtime => unreachable!(),
    };
    let response = request.send().await.map_err(|err| err.to_string())?;
    if response.status().is_success() {
        return Ok(());
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(format_probe_error(status, &body))
}

pub fn resolve_protocol_mode(
    native_api: NativeApi,
    source: NativeApiSource,
) -> EndpointProtocolResolution {
    EndpointProtocolResolution {
        native_api,
        native_api_source: source,
    }
}

fn format_probe_error(status: StatusCode, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        format!("HTTP {}", status.as_u16())
    } else {
        let short = body.chars().take(160).collect::<String>();
        format!("HTTP {} {}", status.as_u16(), short)
    }
}
