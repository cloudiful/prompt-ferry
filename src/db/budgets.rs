use anyhow::Result;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::RequestRecordCategory;

#[derive(Debug, Clone, Copy)]
pub enum RequestBudgetScope {
    Endpoint(Uuid),
    ModelRoute(Uuid),
    McpServer(Uuid),
}

#[derive(Debug, Clone, Copy)]
pub struct RequestBudgetCounts {
    pub daily: i64,
    pub monthly: i64,
}

pub async fn request_budget_counts(
    pool: &PgPool,
    category: RequestRecordCategory,
    scope: RequestBudgetScope,
    now: DateTime<Utc>,
) -> Result<RequestBudgetCounts> {
    let day_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), now.day(), 0, 0, 0)
        .single()
        .expect("valid UTC day start");
    let month_start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .expect("valid UTC month start");

    match scope {
        RequestBudgetScope::Endpoint(endpoint_id) => {
            let counts = sqlx::query_file!(
                "src/sql/usage/request_budget_counts_endpoint.sql",
                category.as_str(),
                endpoint_id,
                day_start,
                month_start,
            )
            .fetch_one(pool)
            .await?;
            Ok(RequestBudgetCounts {
                daily: counts.daily_count.unwrap_or(0),
                monthly: counts.monthly_count.unwrap_or(0),
            })
        }
        RequestBudgetScope::ModelRoute(rule_id) => {
            let counts = sqlx::query_file!(
                "src/sql/usage/request_budget_counts_model_route.sql",
                category.as_str(),
                rule_id,
                day_start,
                month_start,
            )
            .fetch_one(pool)
            .await?;
            Ok(RequestBudgetCounts {
                daily: counts.daily_count.unwrap_or(0),
                monthly: counts.monthly_count.unwrap_or(0),
            })
        }
        RequestBudgetScope::McpServer(server_id) => {
            let counts = sqlx::query_file!(
                "src/sql/usage/request_budget_counts_mcp_server.sql",
                category.as_str(),
                server_id,
                day_start,
                month_start,
            )
            .fetch_one(pool)
            .await?;
            Ok(RequestBudgetCounts {
                daily: counts.daily_count.unwrap_or(0),
                monthly: counts.monthly_count.unwrap_or(0),
            })
        }
    }
}
