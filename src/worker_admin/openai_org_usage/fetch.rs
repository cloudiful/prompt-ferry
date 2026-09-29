//! HTTP client, cursor pagination, and URL construction for the OpenAI
//! organization Admin API reads.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;

use super::aggregate::{
    UsagePage, openai_error_message, parse_page, sum_cost_results, sum_usage_results,
    utc_month_start,
};
use super::{
    BUCKET_WIDTH, DEFAULT_CURRENCY, MAX_PAGES, ORGANIZATION_COSTS_PATH, ORGANIZATION_USAGE_PATH,
    OpenAiOrganizationUsageResponse, PAGE_LIMIT,
};
use crate::db::EndpointProvider;
use crate::worker_admin::json_scalars::truncate_message;

/// Read both official organization endpoints concurrently (at most two
/// in-flight requests) against an already-pinned base and flatten them into one
/// month-to-date snapshot. `base_url` must come from `organization_base_url`.
pub(super) async fn fetch_organization_usage_at(
    base_url: &str,
    admin_api_key: &str,
    proxy_url: Option<&str>,
    now: DateTime<Utc>,
) -> Result<OpenAiOrganizationUsageResponse> {
    // Reuse the pooled, proxy-aware protocol client (15s per-request timeout);
    // an invalid proxy fails closed instead of silently going direct. `base_url`
    // is the pinned official origin, so the pool key cannot follow a custom host.
    let client =
        crate::endpoint_protocol::endpoint_protocol_client_for_endpoint(proxy_url, base_url)
            .map_err(|message| anyhow!("{message}"))?;
    let period_start = utc_month_start(now);
    let start_time = period_start.timestamp();
    let end_time = now.timestamp();

    let (usage, costs) = tokio::try_join!(
        collect_buckets(
            &client,
            base_url,
            ORGANIZATION_USAGE_PATH,
            admin_api_key,
            start_time,
            end_time,
        ),
        collect_buckets(
            &client,
            base_url,
            ORGANIZATION_COSTS_PATH,
            admin_api_key,
            start_time,
            end_time,
        ),
    )?;

    let (input_tokens, output_tokens) = sum_usage_results(&usage.0);
    let (cost_usd, currency) = sum_cost_results(&costs.0);
    Ok(OpenAiOrganizationUsageResponse {
        provider: EndpointProvider::OpenAi,
        period_start,
        period_end: now,
        currency: currency.unwrap_or_else(|| DEFAULT_CURRENCY.to_string()),
        input_tokens,
        output_tokens,
        total_tokens: input_tokens.saturating_add(output_tokens),
        cost_usd,
        truncated: usage.1 || costs.1,
        cached: false,
        fetched_at: now,
    })
}

/// Cursor-paginated bucket reader shared by the usage and costs endpoints.
/// Returns the buckets plus whether the page cap stopped a longer upstream
/// pagination.
async fn collect_buckets(
    client: &Client,
    base_url: &str,
    path: &str,
    secret: &str,
    start_time: i64,
    end_time: i64,
) -> Result<(Vec<Value>, bool)> {
    let url = organization_url(base_url, path);
    let mut buckets = Vec::new();
    let mut cursor: Option<String> = None;
    for page_index in 0..MAX_PAGES {
        let mut query = format!(
            "start_time={start_time}&end_time={end_time}&bucket_width={BUCKET_WIDTH}&limit={PAGE_LIMIT}"
        );
        if let Some(page) = cursor.as_deref() {
            query.push_str("&page=");
            query.push_str(&urlencoding::encode(page));
        }
        let response = client
            .get(format!("{url}?{query}"))
            .bearer_auth(secret)
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(|error| anyhow!("{}", truncate_message(error.to_string())))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| anyhow!("{}", truncate_message(error.to_string())))?;
        let body: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) if status.is_success() => {
                return Err(anyhow!("{}", truncate_message(text)));
            }
            Err(_) => Value::Null,
        };
        if !status.is_success() {
            return Err(anyhow!("{}", openai_error_message(status.as_u16(), &body)));
        }
        let UsagePage {
            buckets: page_buckets,
            has_more,
            next_page,
        } = parse_page(&body);
        buckets.extend(page_buckets);
        match (has_more, next_page) {
            (false, _) => return Ok((buckets, false)),
            (true, Some(next)) if page_index + 1 < MAX_PAGES => cursor = Some(next),
            (true, _) => return Ok((buckets, true)),
        }
    }
    Ok((buckets, true))
}

/// Organization endpoints live under the Platform root; a stored `/v1` suffix
/// is stripped so the path segment never doubles.
fn organization_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}{path}")
}

#[cfg(test)]
mod tests {
    use super::super::organization_base_url;
    use super::*;
    use crate::upstream_presets::OPENAI_BASE_URL;
    use chrono::TimeZone;

    #[test]
    fn custom_stored_base_still_targets_the_official_organization_origin() {
        for stored in [
            "https://custom.example.test/v1",
            "https://proxy-gateway.example.test/openai",
            "https://api.openai.com/v1",
            "http://127.0.0.1:8080",
            "",
        ] {
            let base = organization_base_url(stored);
            assert_eq!(base, OPENAI_BASE_URL, "stored base {stored}");
            assert_eq!(
                organization_url(base, ORGANIZATION_USAGE_PATH),
                "https://api.openai.com/v1/organization/usage/completions",
                "stored base {stored}"
            );
            assert_eq!(
                organization_url(base, ORGANIZATION_COSTS_PATH),
                "https://api.openai.com/v1/organization/costs",
                "stored base {stored}"
            );
        }
    }

    #[test]
    fn organization_url_appends_the_version_segment_once() {
        for base in [
            "https://api.openai.com",
            "https://api.openai.com/",
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/",
        ] {
            assert_eq!(
                organization_url(base, ORGANIZATION_USAGE_PATH),
                "https://api.openai.com/v1/organization/usage/completions"
            );
        }
    }

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
    async fn fetch_aggregates_both_endpoints_across_pages() {
        use std::collections::HashMap;

        async fn usage(
            axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
        ) -> axum::Json<Value> {
            if params.get("page").map(String::as_str) == Some("page_2") {
                axum::Json(serde_json::json!({
                    "data": [{ "results": [{ "input_tokens": 3, "output_tokens": 1 }] }],
                    "has_more": false
                }))
            } else {
                axum::Json(serde_json::json!({
                    "data": [{ "results": [{ "input_tokens": 10, "output_tokens": 4 }] }],
                    "has_more": true,
                    "next_page": "page_2"
                }))
            }
        }
        async fn costs() -> axum::Json<Value> {
            axum::Json(serde_json::json!({
                "data": [{ "results": [{ "amount": { "value": 0.5, "currency": "usd" } }] }],
                "has_more": false
            }))
        }

        let base = spawn_mock(
            axum::Router::new()
                .route(
                    "/v1/organization/usage/completions",
                    axum::routing::get(usage),
                )
                .route("/v1/organization/costs", axum::routing::get(costs)),
        )
        .await;

        let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 9, 27, 12, 0, 0).unwrap();
        let response = fetch_organization_usage_at(&base, "sk-admin", None, now)
            .await
            .expect("fetch organization usage");
        assert_eq!(response.input_tokens, 13);
        assert_eq!(response.output_tokens, 5);
        assert_eq!(response.total_tokens, 18);
        assert!((response.cost_usd - 0.5).abs() < 1e-9);
        assert_eq!(response.currency, "usd");
        assert!(!response.truncated);
        assert!(!response.cached);
        assert_eq!(response.period_end, now);
        assert_eq!(
            response.period_start,
            Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap()
        );
    }

    #[tokio::test]
    async fn fetch_surfaces_upstream_error_status() {
        async fn unauthorized() -> (axum::http::StatusCode, axum::Json<Value>) {
            (
                axum::http::StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": { "code": "invalid_api_key", "message": "Incorrect API key provided" }
                })),
            )
        }

        let base = spawn_mock(
            axum::Router::new()
                .route(
                    "/v1/organization/usage/completions",
                    axum::routing::get(unauthorized),
                )
                .route("/v1/organization/costs", axum::routing::get(unauthorized)),
        )
        .await;

        let error = fetch_organization_usage_at(&base, "sk-admin", None, Utc::now())
            .await
            .expect_err("upstream 401 must fail");
        let message = error.to_string();
        assert!(message.contains("401"), "message={message}");
        assert!(message.contains("Incorrect API key provided"));
    }
}
