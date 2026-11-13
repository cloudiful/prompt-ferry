use anyhow::Result;
use sea_orm::sea_query::{Expr, ExprTrait, Query, Value};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::entities::postgres::{mcp_credentials, mcp_servers};
use crate::db::seaorm::{native_error, postgres_connection};
use crate::db::types::{
    McpBearerToken, McpCredential, McpProviderSecret, canonical_mcp_provider_kind,
};

fn credential_from_model(model: mcp_credentials::Model) -> McpCredential {
    McpCredential {
        credential_id: model.credential_id,
        server_id: model.server_id,
        credential_label: model.credential_label,
        secret: model.secret,
        position: model.position,
        enabled: model.enabled,
        provider_kind: model.provider_kind,
        default_cost: model.default_cost,
        strict_mode: model.strict_mode,
        billing_period_start: model.billing_period_start,
        billing_period_end: model.billing_period_end,
        provider_remaining: model.provider_remaining,
        provider_synced_at: model.provider_synced_at,
        provider_reset_at: model.provider_reset_at,
        cooldown_until: model.cooldown_until,
        last_error: model.last_error,
        last_error_at: model.last_error_at,
        created_at: model.created_at,
        updated_at: model.updated_at,
    }
}

pub async fn list_credentials_by_server(
    pool: &PgPool,
    server_id: Uuid,
) -> Result<Vec<McpCredential>> {
    let db = postgres_connection(pool);
    let rows = mcp_credentials::Entity::find()
        .filter(mcp_credentials::Column::ServerId.eq(server_id))
        .order_by_asc(mcp_credentials::Column::Position)
        .order_by_asc(mcp_credentials::Column::CredentialId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows.into_iter().map(credential_from_model).collect())
}

pub async fn insert_credential(
    pool: &PgPool,
    server_id: Uuid,
    label: &str,
    secret: &str,
    position: i32,
    enabled: bool,
) -> Result<McpCredential> {
    let db = postgres_connection(pool);
    let row = mcp_credentials::Entity::insert(mcp_credentials::ActiveModel {
        server_id: Set(server_id),
        credential_label: Set(label.to_string()),
        secret: Set(secret.to_string()),
        position: Set(position),
        enabled: Set(enabled),
        ..Default::default()
    })
    .exec_with_returning(&db)
    .await
    .map_err(native_error)?;
    Ok(credential_from_model(row))
}

pub async fn update_credential_token(
    pool: &PgPool,
    credential_id: Uuid,
    label: &str,
    secret: &str,
    enabled: bool,
) -> Result<McpCredential> {
    let db = postgres_connection(pool);
    let row = mcp_credentials::Entity::update_many()
        .col_expr(mcp_credentials::Column::CredentialLabel, Expr::value(label))
        .col_expr(mcp_credentials::Column::Secret, Expr::value(secret))
        .col_expr(mcp_credentials::Column::Enabled, Expr::value(enabled))
        .col_expr(
            mcp_credentials::Column::UpdatedAt,
            Expr::current_timestamp(),
        )
        .filter(mcp_credentials::Column::CredentialId.eq(credential_id))
        .exec_with_returning(&db)
        .await
        .map_err(native_error)?
        .into_iter()
        .next()
        .ok_or_else(|| native_error(sea_orm::DbErr::RecordNotUpdated))?;
    Ok(credential_from_model(row))
}

pub async fn delete_credential(pool: &PgPool, credential_id: Uuid) -> Result<bool> {
    let db = postgres_connection(pool);
    let result = mcp_credentials::Entity::delete_many()
        .filter(mcp_credentials::Column::CredentialId.eq(credential_id))
        .exec(&db)
        .await
        .map_err(native_error)?;
    Ok(result.rows_affected > 0)
}

/// Reconcile `mcp_credentials` rows with the bearer token array of a server.
///
/// The array position is the credential identity: new positions are inserted,
/// removed positions are deleted, and matching positions keep the token text,
/// label and enabled flag following the configuration.
pub async fn sync_credentials_from_tokens(
    pool: &PgPool,
    server_id: Uuid,
    tokens_json: &serde_json::Value,
) -> Result<()> {
    let tokens = McpBearerToken::parse_array(tokens_json);
    let db = postgres_connection(pool);
    let tx = db.begin().await.map_err(native_error)?;
    // The owning MCP server is the source of truth for the credential
    // provider; generic/legacy servers resolve to NULL so they never trigger
    // provider-specific flows such as a Firecrawl balance fetch.
    let server_provider_kind = mcp_servers::Entity::find_by_id(server_id)
        .one(&tx)
        .await
        .map_err(native_error)?
        .and_then(|row| row.provider_kind);
    let provider_kind = canonical_mcp_provider_kind(server_provider_kind.as_deref());
    let existing = mcp_credentials::Entity::find()
        .filter(mcp_credentials::Column::ServerId.eq(server_id))
        .order_by_asc(mcp_credentials::Column::Position)
        .order_by_asc(mcp_credentials::Column::CredentialId)
        .all(&tx)
        .await
        .map_err(native_error)?
        .into_iter()
        .map(credential_from_model)
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    for (index, token) in tokens.iter().enumerate() {
        let position = index as i32;
        seen.insert(position);
        let label = format!("token-{}", index + 1);
        if let Some(credential) = existing
            .iter()
            .find(|credential| credential.position == position)
        {
            if credential.credential_label != label
                || credential.secret != token.token
                || credential.enabled != token.enabled
                || credential.provider_kind.as_deref() != provider_kind
            {
                mcp_credentials::Entity::update_many()
                    .col_expr(mcp_credentials::Column::CredentialLabel, Expr::value(label))
                    .col_expr(
                        mcp_credentials::Column::Secret,
                        Expr::value(token.token.as_str()),
                    )
                    .col_expr(mcp_credentials::Column::Enabled, Expr::value(token.enabled))
                    .col_expr(
                        mcp_credentials::Column::ProviderKind,
                        Expr::value(provider_kind),
                    )
                    .col_expr(
                        mcp_credentials::Column::UpdatedAt,
                        Expr::current_timestamp(),
                    )
                    .filter(mcp_credentials::Column::CredentialId.eq(credential.credential_id))
                    .exec_with_returning(&tx)
                    .await
                    .map_err(native_error)?;
            }
        } else {
            mcp_credentials::Entity::insert(mcp_credentials::ActiveModel {
                server_id: Set(server_id),
                credential_label: Set(label),
                secret: Set(token.token.clone()),
                position: Set(position),
                enabled: Set(token.enabled),
                provider_kind: Set(provider_kind.map(str::to_string)),
                ..Default::default()
            })
            .exec_with_returning(&tx)
            .await
            .map_err(native_error)?;
        }
    }
    for credential in existing
        .iter()
        .filter(|credential| !seen.contains(&credential.position))
    {
        mcp_credentials::Entity::delete_many()
            .filter(mcp_credentials::Column::CredentialId.eq(credential.credential_id))
            .exec(&tx)
            .await
            .map_err(native_error)?;
    }
    tx.commit().await.map_err(native_error)?;
    Ok(())
}

/// Enabled Firecrawl credentials eligible for a provider balance refresh.
/// Only PostgreSQL stores these rows; the SQLite runtime never calls this.
pub async fn list_firecrawl_credentials(pool: &PgPool) -> Result<Vec<McpProviderSecret>> {
    let db = postgres_connection(pool);
    let rows = mcp_credentials::Entity::find()
        .filter(mcp_credentials::Column::Enabled.eq(true))
        .filter(mcp_credentials::Column::ProviderKind.eq("firecrawl"))
        .order_by_asc(mcp_credentials::Column::CredentialId)
        .all(&db)
        .await
        .map_err(native_error)?;
    Ok(rows
        .into_iter()
        .map(|row| McpProviderSecret {
            credential_id: row.credential_id,
            secret: row.secret,
        })
        .collect())
}

/// Reconcile every credential's `provider_kind` with its owning MCP server.
/// Idempotent: only rows whose canonical value differs are touched. Returns
/// the number of credentials updated.
pub async fn backfill_credential_provider_kinds(pool: &PgPool) -> Result<i64> {
    let db = postgres_connection(pool);
    let credential_kind = || {
        Expr::col((
            mcp_credentials::Entity,
            mcp_credentials::Column::ProviderKind,
        ))
    };
    let server_kind = Expr::col((mcp_servers::Entity, mcp_servers::Column::ProviderKind));
    let target: Expr = Expr::case(
        server_kind
            .clone()
            .is_in(["context7", "firecrawl", "minimax"]),
        server_kind,
    )
    .finally(Expr::value(Value::String(None)))
    .into();
    let equal = credential_kind()
        .clone()
        .is_null()
        .and(target.clone().is_null())
        .or(credential_kind()
            .clone()
            .is_not_null()
            .and(target.clone().is_not_null())
            .and(credential_kind().eq(target.clone())));
    let distinct = equal.not();
    let statement = Query::update()
        .table(mcp_credentials::Entity)
        .from(mcp_servers::Entity)
        .value(mcp_credentials::Column::ProviderKind, target)
        .value(
            mcp_credentials::Column::UpdatedAt,
            Expr::current_timestamp(),
        )
        .and_where(
            Expr::col((mcp_servers::Entity, mcp_servers::Column::ServerId))
                .equals((mcp_credentials::Entity, mcp_credentials::Column::ServerId)),
        )
        .and_where(distinct)
        .to_owned();
    let result = db.execute(&statement).await.map_err(native_error)?;
    Ok(result.rows_affected() as i64)
}
