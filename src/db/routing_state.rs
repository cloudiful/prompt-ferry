use crate::db::entities::postgres::conversation_endpoint_overrides as overrides;
use crate::db::seaorm::{native_error, postgres_connection};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sea_orm::sea_query::{
    CommonTableExpression, Expr, OnConflict, Query, SelectStatement, WithClause, WithQuery,
};
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, ExprTrait, QueryFilter};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::ConversationEndpointOverride;

/// The checked conversion enforces the migration's NOT NULL contract.
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

fn projection(source: &'static str) -> SelectStatement {
    let mut query = Query::select();
    query.columns([
        (source, "conversation_id"),
        (source, "endpoint_id"),
        (source, "endpoint_key_id"),
        (source, "created_by_user_id"),
        (source, "created_at"),
        (source, "updated_at"),
    ]);
    let key = Query::select()
        .column("key_label")
        .from("endpoint_api_keys")
        .and_where(Expr::col(("endpoint_api_keys", "key_id")).equals((source, "endpoint_key_id")))
        .to_owned();
    let endpoint = Query::select()
        .column("name")
        .from("provider_endpoints")
        .and_where(Expr::col(("provider_endpoints", "endpoint_id")).equals((source, "endpoint_id")))
        .to_owned();
    query
        .expr_as(
            Expr::SubQuery(None, Box::new(key.into())),
            "endpoint_key_label",
        )
        .expr_as(
            Expr::SubQuery(None, Box::new(endpoint.into())),
            "endpoint_name",
        );
    query
}

fn decode_row(row: sea_orm::QueryResult) -> Result<ConversationEndpointOverrideRow> {
    Ok(ConversationEndpointOverrideRow {
        conversation_id: row.try_get("", "conversation_id")?,
        endpoint_id: row.try_get("", "endpoint_id")?,
        endpoint_key_id: row.try_get("", "endpoint_key_id")?,
        endpoint_key_label: row.try_get("", "endpoint_key_label")?,
        endpoint_name: row.try_get("", "endpoint_name")?,
        created_by_user_id: row.try_get("", "created_by_user_id")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn get_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<Option<ConversationEndpointOverride>> {
    let mut query = projection("o");
    query
        .from_as(overrides::Entity, "o")
        .and_where(Expr::col(("o", "conversation_id")).eq(conversation_id));
    let db = postgres_connection(pool);
    let row = db.query_one(&query).await.map_err(native_error)?;
    row.map(decode_row)
        .transpose()?
        .map(ConversationEndpointOverride::try_from)
        .transpose()
}

pub async fn upsert_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key_id: Option<Uuid>,
    created_by_user_id: i64,
) -> Result<ConversationEndpointOverride> {
    let query = upsert_query(
        conversation_id,
        endpoint_id,
        endpoint_key_id,
        created_by_user_id,
    );
    let db = postgres_connection(pool);
    let row = db
        .query_one(&query)
        .await
        .map_err(native_error)?
        .ok_or_else(|| anyhow::anyhow!("conversation endpoint override upsert returned no row"))?;
    decode_row(row)?.try_into()
}

fn upsert_query(
    conversation_id: Uuid,
    endpoint_id: Uuid,
    endpoint_key_id: Option<Uuid>,
    created_by_user_id: i64,
) -> WithQuery {
    use overrides::Column as C;
    let insert = Query::insert()
        .into_table(overrides::Entity)
        .columns([
            C::ConversationId,
            C::EndpointId,
            C::EndpointKeyId,
            C::CreatedByUserId,
        ])
        .values_panic([
            conversation_id.into(),
            endpoint_id.into(),
            endpoint_key_id.into(),
            created_by_user_id.into(),
        ])
        .on_conflict(
            OnConflict::column(C::ConversationId)
                .update_columns([C::EndpointId, C::EndpointKeyId, C::CreatedByUserId])
                .value(C::UpdatedAt, Expr::current_timestamp())
                .to_owned(),
        )
        .returning_all()
        .to_owned();
    let cte = CommonTableExpression::new()
        .table_name("upserted")
        .query(insert)
        .to_owned();
    let mut select = projection("upserted");
    select.from("upserted");
    WithClause::new().cte(cte).to_owned().query(select)
}

pub async fn clear_conversation_endpoint_key_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    use overrides::Column as C;
    let db = postgres_connection(pool);
    let result = overrides::Entity::update_many()
        .col_expr(C::EndpointKeyId, Expr::val(Option::<Uuid>::None))
        .col_expr(C::UpdatedAt, Expr::current_timestamp())
        .filter(C::ConversationId.eq(conversation_id))
        .filter(C::EndpointKeyId.is_not_null())
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

pub async fn delete_conversation_endpoint_override(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = overrides::Entity::delete_many()
        .filter(overrides::Column::ConversationId.eq(conversation_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
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
    fn upsert_is_one_bound_data_modifying_cte() {
        use sea_orm::sea_query::{PostgresQueryBuilder, Value};
        let conversation = Uuid::from_u128(1);
        let endpoint = Uuid::from_u128(2);
        let key = Uuid::from_u128(3);
        let (sql, values) =
            upsert_query(conversation, endpoint, Some(key), 7).build(PostgresQueryBuilder);
        assert!(
            sql.starts_with("WITH \"upserted\" AS (INSERT INTO "),
            "{sql}"
        );
        assert!(
            sql.contains("ON CONFLICT (\"conversation_id\") DO UPDATE SET"),
            "{sql}"
        );
        assert!(sql.contains("RETURNING *"), "{sql}");
        assert!(sql.contains("FROM \"upserted\""), "{sql}");
        assert!(
            sql.contains("SELECT \"key_label\" FROM \"endpoint_api_keys\""),
            "{sql}"
        );
        assert!(
            sql.contains("SELECT \"name\" FROM \"provider_endpoints\""),
            "{sql}"
        );
        assert_eq!(
            values.0,
            vec![
                Value::from(conversation),
                Value::from(endpoint),
                Value::from(Some(key)),
                Value::from(7_i64)
            ]
        );
        assert!(
            sql.contains("$1") && sql.contains("$2") && sql.contains("$3") && sql.contains("$4"),
            "{sql}"
        );
        assert!(!sql.contains(';'), "{sql}");
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
