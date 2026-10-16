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
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use serde_json::json;

    async fn spawn_mock(router: axum::Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock upstream");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn returns_status_and_decoded_body_with_bearer_auth() {
        async fn balance(headers: axum::http::HeaderMap) -> axum::Json<Value> {
            let authorization = headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            axum::Json(json!({ "authorization": authorization }))
        }

        let base =
            spawn_mock(axum::Router::new().route("/balance", axum::routing::get(balance))).await;
        let (status, body) = get_balance_json(&Client::new(), format!("{base}/balance"), "sk-test")
            .await
            .expect("transport success");
        assert_eq!(status, 200);
        assert_eq!(body["authorization"], "Bearer sk-test");
    }

    #[tokio::test]
    async fn preserves_error_status_with_json_body() {
        async fn failing() -> impl IntoResponse {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(json!({ "error": "boom" })),
            )
        }

        let base =
            spawn_mock(axum::Router::new().route("/balance", axum::routing::get(failing))).await;
        let (status, body) = get_balance_json(&Client::new(), format!("{base}/balance"), "sk")
            .await
            .expect("json error body still decodes");
        assert_eq!(status, 500);
        assert_eq!(body["error"], "boom");
    }

    #[tokio::test]
    async fn reports_non_json_body_with_status() {
        async fn not_json() -> impl IntoResponse {
            (axum::http::StatusCode::OK, "not json")
        }

        let base =
            spawn_mock(axum::Router::new().route("/balance", axum::routing::get(not_json))).await;
        let (status, message) = get_balance_json(&Client::new(), format!("{base}/balance"), "sk")
            .await
            .expect_err("non-json body fails");
        assert_eq!(status, Some(200));
        assert!(!message.is_empty());
    }

    #[tokio::test]
    async fn reports_send_failure_without_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind closed port");
        let addr = listener.local_addr().expect("closed addr");
        drop(listener);

        let (status, message) =
            get_balance_json(&Client::new(), format!("http://{addr}/balance"), "sk")
                .await
                .expect_err("closed port fails");
        assert_eq!(status, None);
        assert!(!message.is_empty());
    }
}
