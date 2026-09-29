//! Pure parsing and aggregation for the OpenAI organization usage responses.

use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde_json::Value;

use crate::worker_admin::json_scalars::{truncate_message, value_as_f64, value_as_i64};

pub(super) struct UsagePage {
    pub(super) buckets: Vec<Value>,
    pub(super) has_more: bool,
    pub(super) next_page: Option<String>,
}

pub(super) fn parse_page(body: &Value) -> UsagePage {
    let buckets = body
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_more = body
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let next_page = body
        .get("next_page")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|page| !page.trim().is_empty());
    UsagePage {
        buckets,
        has_more,
        next_page,
    }
}

/// `usage/completions` returns one `results` array per bucket; without
/// `group_by` each bucket carries a single aggregated row.
pub(super) fn sum_usage_results(buckets: &[Value]) -> (i64, i64) {
    let mut input_tokens = 0i64;
    let mut output_tokens = 0i64;
    for result in bucket_results(buckets) {
        input_tokens = input_tokens.saturating_add(
            result
                .get("input_tokens")
                .and_then(value_as_i64)
                .unwrap_or(0),
        );
        output_tokens = output_tokens.saturating_add(
            result
                .get("output_tokens")
                .and_then(value_as_i64)
                .unwrap_or(0),
        );
    }
    (input_tokens, output_tokens)
}

/// `costs` amounts are `{ value, currency }` decimal values. The organization
/// costs API reports USD only, so every bucket value is summed and the first
/// seen currency is reported (defaulting to `usd` when absent).
pub(super) fn sum_cost_results(buckets: &[Value]) -> (f64, Option<String>) {
    let mut total = 0.0f64;
    let mut currency = None;
    for result in bucket_results(buckets) {
        let amount = result.get("amount");
        if let Some(value) = amount
            .and_then(|amount| amount.get("value"))
            .and_then(value_as_f64)
        {
            total += value;
        }
        if currency.is_none() {
            currency = amount
                .and_then(|amount| amount.get("currency"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }
    (total, currency)
}

fn bucket_results(buckets: &[Value]) -> impl Iterator<Item = &Value> {
    buckets
        .iter()
        .filter_map(|bucket| bucket.get("results").and_then(Value::as_array))
        .flatten()
}

/// First instant of the current UTC month.
pub(super) fn utc_month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    let date = now
        .date_naive()
        .with_day(1)
        .expect("day 1 exists in every month");
    Utc.from_utc_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight is a valid time"))
}

pub(super) fn openai_error_message(status: u16, body: &Value) -> String {
    let error = body.get("error");
    let code = error
        .and_then(|error| error.get("code").or_else(|| error.get("type")))
        .and_then(Value::as_str);
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| body.get("message").and_then(Value::as_str));
    match (code, message) {
        (Some(code), Some(message)) => truncate_message(format!(
            "OpenAI rejected the request (HTTP {status}, {code}): {message}"
        )),
        (None, Some(message)) => truncate_message(format!(
            "OpenAI rejected the request (HTTP {status}): {message}"
        )),
        _ => truncate_message(format!("OpenAI returned HTTP {status}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_start_truncates_to_utc_day_one_midnight() {
        let now: DateTime<Utc> = Utc.with_ymd_and_hms(2026, 9, 27, 13, 45, 12).unwrap();
        let start = utc_month_start(now);
        assert_eq!(start, Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap());
    }

    #[test]
    fn usage_results_sum_across_buckets_and_skip_missing_fields() {
        let buckets = vec![
            serde_json::json!({
                "results": [
                    { "input_tokens": 120, "output_tokens": 30 },
                    { "output_tokens": 5 }
                ]
            }),
            serde_json::json!({ "results": [{ "input_tokens": 8, "output_tokens": 2 }] }),
            serde_json::json!({}),
        ];
        assert_eq!(sum_usage_results(&buckets), (128, 37));
    }

    #[test]
    fn cost_results_sum_amounts_and_report_currency() {
        let buckets = vec![
            serde_json::json!({
                "results": [
                    { "amount": { "value": 0.06, "currency": "usd" } },
                    { "amount": { "value": "0.14", "currency": "usd" } }
                ]
            }),
            serde_json::json!({ "results": [{ "amount": { "value": 0.05 } }] }),
        ];
        let (total, currency) = sum_cost_results(&buckets);
        assert!((total - 0.25).abs() < 1e-9, "total={total}");
        assert_eq!(currency.as_deref(), Some("usd"));
    }

    #[test]
    fn page_parsing_reads_cursor_and_more_flag() {
        let page = parse_page(&serde_json::json!({
            "data": [{ "results": [] }],
            "has_more": true,
            "next_page": "page_2"
        }));
        assert_eq!(page.buckets.len(), 1);
        assert!(page.has_more);
        assert_eq!(page.next_page.as_deref(), Some("page_2"));

        let last = parse_page(&serde_json::json!({
            "data": [],
            "has_more": false,
            "next_page": "  "
        }));
        assert!(!last.has_more);
        assert!(last.next_page.is_none());
    }

    #[test]
    fn error_message_carries_status_code_and_upstream_text() {
        let body = serde_json::json!({
            "error": { "code": "invalid_api_key", "message": "Incorrect API key provided" }
        });
        let message = openai_error_message(401, &body);
        assert!(message.contains("401"));
        assert!(message.contains("invalid_api_key"));
        assert!(message.contains("Incorrect API key provided"));

        let opaque = openai_error_message(500, &Value::Null);
        assert_eq!(opaque, "OpenAI returned HTTP 500");
    }
}
