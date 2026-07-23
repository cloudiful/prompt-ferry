use crate::db;
use anyhow::Result;
use chrono::Utc;

fn request_budget_message(scope_label: &str, scope_name: &str, period: &str, limit: i32) -> String {
    format!("{scope_label} {scope_name} exceeded the {period} request budget ({limit})")
}

pub(super) async fn check_named_request_budget(
    pool: &sqlx::PgPool,
    category: db::RequestRecordCategory,
    scope: db::RequestBudgetScope,
    scope_label: &str,
    scope_name: &str,
    daily_max_requests: Option<i32>,
    monthly_max_requests: Option<i32>,
) -> Result<Option<String>> {
    if daily_max_requests.is_none() && monthly_max_requests.is_none() {
        return Ok(None);
    }
    let counts = db::request_budget_counts(pool, category, scope, Utc::now()).await?;
    if let Some(limit) = daily_max_requests
        && counts.daily >= i64::from(limit)
    {
        return Ok(Some(request_budget_message(
            scope_label,
            scope_name,
            "daily",
            limit,
        )));
    }
    if let Some(limit) = monthly_max_requests
        && counts.monthly >= i64::from(limit)
    {
        return Ok(Some(request_budget_message(
            scope_label,
            scope_name,
            "monthly",
            limit,
        )));
    }
    Ok(None)
}
