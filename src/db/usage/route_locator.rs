use super::*;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RequestRecordRouteLocator {
    pub record_id: i64,
    pub user_id: Option<i64>,
    pub conversation_id: Option<uuid::Uuid>,
    pub model: Option<String>,
    pub model_route_rule_id: Option<uuid::Uuid>,
}

pub async fn get_request_record_route_locator(
    pool: &PgPool,
    record_id: i64,
) -> Result<Option<RequestRecordRouteLocator>> {
    Ok(sqlx::query_file_as!(
        RequestRecordRouteLocator,
        "src/sql/usage/get_request_record_route_locator.sql",
        record_id,
    )
    .fetch_optional(pool)
    .await?)
}
