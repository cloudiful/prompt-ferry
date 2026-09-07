//! CommandCode alpha billing fetcher (issue #184 P3): whoami→orgId→credits+
//! subscriptions+usage/summary with the endpoint Bearer key.

use reqwest::Client;
use serde_json::Value;
use uuid::Uuid;

use super::command_code_parsing::{
    build_window_usage, command_code_business_error, parse_credits_section,
    parse_subscription_plan, parse_summary_present, parse_whoami_org_id, parse_window_entry,
    plan_monthly_credits,
};
use super::json_scalars::{failed_key, truncate_message, value_as_string};
use crate::worker_admin_types::{CommandCodeBalances, TokenPlanKeyUsage};

// Keep the cache call path stable after the P3 split.
pub(crate) use super::command_code_parsing::command_code_remaining_percent;

/// Alpha-fixed CommandCode API base URL.
///
/// Intentionally a constant with no `COMMANDCODE_BASE_URL` env or
/// `EndpointSetting` override: alpha billing paths (`/alpha/*`) are pinned to
/// the production host, `EndpointSetting` carries no base-URL field, and
/// threading an override through DB/schema buys nothing today. Revisit if a
/// staging host or relocation appears (P5/P6).
pub(crate) const COMMAND_CODE_BASE: &str = "https://api.commandcode.ai";

fn with_org_query(base: &str, path: &str, org_id: Option<&str>) -> String {
    match org_id.filter(|id| !id.trim().is_empty()) {
        Some(id) => format!("{base}{path}?orgId={}", urlencoding::encode(id)),
        None => format!("{base}{path}"),
    }
}

async fn get_alpha_json(
    client: &Client,
    url: String,
    secret: &str,
) -> std::result::Result<(u16, Value), (Option<u16>, Option<String>, String)> {
    let response = client
        .get(&url)
        .bearer_auth(secret)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|error| (None, None, truncate_message(error.to_string())))?;
    let status = response.status().as_u16();
    let body: Value = response
        .json()
        .await
        .map_err(|error| (Some(status), None, truncate_message(error.to_string())))?;
    if !(200..300).contains(&status) {
        let (code, message) = match command_code_business_error(&body) {
            Some((code, message)) => (code, message),
            None => {
                let message = body
                    .get("message")
                    .and_then(value_as_string)
                    .unwrap_or_else(|| format!("CommandCode returned HTTP {status}"));
                (None, truncate_message(message))
            }
        };
        return Err((Some(status), code, message));
    }
    Ok((status, body))
}

// One key through whoami→credits+subscriptions→summary. whoami is fatal;
// optional sections degrade (only 401/403 fatal); all three missing fails;
// missing windowLimits (PAYG) degrades to None windows.
pub(crate) async fn fetch_command_code_key_usage(
    client: Client,
    base: &str,
    key_id: Uuid,
    key_label: String,
    secret: String,
) -> TokenPlanKeyUsage {
    let base = base.trim_end_matches('/');
    let (whoami_status, whoami_body) = match get_alpha_json(
        &client,
        with_org_query(base, "/alpha/whoami", None),
        &secret,
    )
    .await
    {
        Ok((status, body)) => (Some(status), body),
        Err((status, code, message)) => {
            return failed_key(key_id, key_label, status, code, message);
        }
    };
    if let Some((code, message)) = command_code_business_error(&whoami_body) {
        return failed_key(key_id, key_label, whoami_status, code, message);
    }
    let Some(org_id) = parse_whoami_org_id(&whoami_body) else {
        return failed_key(
            key_id,
            key_label,
            whoami_status,
            None,
            "CommandCode returned an unrecognized account response".to_string(),
        );
    };
    let org = org_id.as_deref();
    let (credits_result, subscriptions_result) = tokio::join!(
        get_alpha_json(
            &client,
            with_org_query(base, "/alpha/billing/credits", org),
            &secret
        ),
        get_alpha_json(
            &client,
            with_org_query(base, "/alpha/billing/subscriptions", org),
            &secret
        ),
    );
    if let Err((status, code, message)) = &credits_result
        && matches!(status, Some(401 | 403))
    {
        return failed_key(key_id, key_label, *status, code.clone(), message.clone());
    }
    if let Err((status, code, message)) = &subscriptions_result
        && matches!(status, Some(401 | 403))
    {
        return failed_key(key_id, key_label, *status, code.clone(), message.clone());
    }
    let credits_body = credits_result.ok().map(|(_, body)| body).filter(|body| {
        command_code_business_error(body).is_none() && parse_credits_section(body).is_some()
    });
    let subscriptions_body = subscriptions_result
        .ok()
        .map(|(_, body)| body)
        .filter(|body| {
            command_code_business_error(body).is_none() && parse_subscription_plan(body).is_some()
        });
    let plan_id = subscriptions_body
        .as_ref()
        .and_then(parse_subscription_plan)
        .flatten();
    let summary_result = get_alpha_json(
        &client,
        with_org_query(base, "/alpha/usage/summary", org),
        &secret,
    )
    .await;
    if let Err((status, code, message)) = &summary_result
        && matches!(status, Some(401 | 403))
    {
        return failed_key(key_id, key_label, *status, code.clone(), message.clone());
    }
    let summary_present = summary_result
        .ok()
        .map(|(_, body)| body)
        .filter(|body| command_code_business_error(body).is_none())
        .is_some_and(|body| parse_summary_present(&body));
    let parsed_credits = credits_body.as_ref().and_then(parse_credits_section);
    if parsed_credits.is_none() && plan_id.is_none() && !summary_present {
        return failed_key(
            key_id,
            key_label,
            whoami_status,
            None,
            "CommandCode returned no recognized usage data for the account".to_string(),
        );
    }
    let balances = parsed_credits.as_ref().map(|parsed| {
        let monthly = plan_id
            .as_deref()
            .and_then(plan_monthly_credits)
            .or(parsed.monthly)
            .unwrap_or(0.0);
        let purchased = parsed.purchased.unwrap_or(0.0);
        let free = parsed.free.unwrap_or(0.0);
        CommandCodeBalances {
            monthly_credits: monthly,
            purchased_credits: purchased,
            free_credits: free,
            remaining_credits: monthly + purchased + free,
        }
    });
    let (five_hour, weekly) = parsed_credits
        .as_ref()
        .map(|parsed| {
            (
                parse_window_entry(&parsed.windows, "fiveHour")
                    .map(|(used, cap, reset_at)| build_window_usage(used, cap, reset_at)),
                parse_window_entry(&parsed.windows, "weekly")
                    .map(|(used, cap, reset_at)| build_window_usage(used, cap, reset_at)),
            )
        })
        .unwrap_or((None, None));
    TokenPlanKeyUsage {
        key_id,
        key_label,
        ok: true,
        status: whoami_status,
        error_code: None,
        error_message: None,
        model_remains: Vec::new(),
        balances,
        five_hour,
        weekly,
        opencodego_rolling: None,
        opencodego_weekly: None,
        opencodego_monthly: None,
    }
}
