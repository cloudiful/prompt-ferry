use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::ConversationEndpointOverride;

/// Row shape the SQLx analyzer infers for the endpoint override query. The
/// label `LEFT JOIN`s can make the analyzer report schema-guaranteed NOT NULL
/// columns as nullable, so the raw row keeps them optional and the checked
/// conversion enforces the migration's NOT NULL contract.
struct ConversationEndpointOverrideRow {
    conversation_id: Uuid,
    endpoint_id: Option<Uuid>,
    endpoint_key_id: Option<Uuid>,
    endpoint_key_label: Option<String>,
    endpoint_name: Option<String>,
    created_by_user_id: Option<i64>,
    created_at: DateTime<Utc>,
    updated_at: Option<DateTime<Utc>>,
}

impl TryFrom<ConversationEndpointOverrideRow> for ConversationEndpointOverride {
    type Error = anyhow::Error;

    fn try_from(row: ConversationEndpointOverrideRow) -> Result<Self> {
        let conversation_id = row.conversation_id;
        Ok(Self {
            conversation_id,
            endpoint_id: row
                .endpoint_id
                .ok_or_else(|| null_column_error(conversation_id, "endpoint_id"))?,
            endpoint_key_id: row.endpoint_key_id,
            endpoint_key_label: row.endpoint_key_label,
            endpoint_name: row.endpoint_name,
            created_by_user_id: row.created_by_user_id,
            created_at: row.created_at,
            updated_at: row
                .updated_at
                .ok_or_else(|| null_column_error(conversation_id, "updated_at"))?,
        })
    }
}

fn null_column_error(conversation_id: Uuid, column: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "conversation endpoint override {conversation_id} returned NULL {column}, \
         violating the NOT NULL constraint"
    )
}

pub async fn get_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<Option<ConversationEndpointOverride>> {
    let row = sqlx::query_file_as!(
        ConversationEndpointOverrideRow,
        "src/sql/routes/get_conversation_endpoint_override.sql",
        conversation_id,
    )
    .fetch_optional(pool)
    .await?;

    row.map(ConversationEndpointOverride::try_from).transpose()
}

pub async fn upsert_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key_id: Option<Uuid>,
    created_by_user_id: i64,
) -> Result<ConversationEndpointOverride> {
    Ok(sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/upsert_conversation_endpoint_override.sql",
        conversation_id,
        endpoint_id,
        endpoint_key_id,
        created_by_user_id,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn clear_conversation_endpoint_key_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/routes/clear_conversation_endpoint_key_override.sql",
        conversation_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn delete_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/routes/delete_conversation_endpoint_override.sql",
        conversation_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ConversationEndpointOverrideRow {
        ConversationEndpointOverrideRow {
            conversation_id: Uuid::from_u128(1),
            endpoint_id: Some(Uuid::from_u128(2)),
            endpoint_key_id: Some(Uuid::from_u128(3)),
            endpoint_key_label: Some("primary".to_owned()),
            endpoint_name: Some("endpoint-a".to_owned()),
            created_by_user_id: Some(7),
            created_at: "2024-01-02T03:04:05Z".parse().unwrap(),
            updated_at: Some("2024-01-02T03:04:06Z".parse().unwrap()),
        }
    }

    #[test]
    fn converts_non_null_columns() {
        let override_ = ConversationEndpointOverride::try_from(row()).unwrap();

        assert_eq!(override_.conversation_id, Uuid::from_u128(1));
        assert_eq!(override_.endpoint_id, Uuid::from_u128(2));
        assert_eq!(override_.endpoint_key_id, Some(Uuid::from_u128(3)));
        assert_eq!(override_.endpoint_key_label.as_deref(), Some("primary"));
        assert_eq!(override_.endpoint_name.as_deref(), Some("endpoint-a"));
        assert_eq!(override_.created_by_user_id, Some(7));
        assert_eq!(
            override_.created_at,
            "2024-01-02T03:04:05Z".parse::<DateTime<Utc>>().unwrap()
        );
        assert_eq!(
            override_.updated_at,
            "2024-01-02T03:04:06Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn rejects_null_endpoint_id() {
        let mut row = row();
        row.endpoint_id = None;

        let error = ConversationEndpointOverride::try_from(row).unwrap_err();
        assert!(error.to_string().contains("endpoint_id"), "{error}");
    }

    #[test]
    fn rejects_null_updated_at() {
        let mut row = row();
        row.updated_at = None;

        let error = ConversationEndpointOverride::try_from(row).unwrap_err();
        assert!(error.to_string().contains("updated_at"), "{error}");
    }
}
