//! Shared JSON transport for the DeepSeek/OpenRouter balance fetchers
//! (issue #906 P2). Both providers issue the identical bearer-authenticated
//! `GET` and decode a JSON body, so the request/response shape lives here once
//! while each provider keeps its own URLs, parsing and error semantics.

use reqwest::Client;
use serde_json::Value;

use super::json_scalars::truncate_message;

pub(crate) async fn get_balance_json(
    client: &Client,
    url: String,
    secret: &str,
) -> std::result::Result<(u16, Value), (Option<u16>, String)> {
    let response = client
        .get(&url)
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|error| (None, truncate_message(error.to_string())))?;
    let status = response.status().as_u16();
    let body: Value = response
        .json()
        .await
        .map_err(|error| (Some(status), truncate_message(error.to_string())))?;
    Ok((status, body))
}

#[cfg(test)]
mod tests {}
