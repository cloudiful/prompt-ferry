use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{McpBearerToken, McpCredential, McpProviderSecret,
                         canonical_mcp_provider_kind};

pub async fn list_credentials_by_server(
    pool: &PgPool,
    server_id: Uuid,
) -> Result<Vec<McpCredential>> {
    Ok(sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/list_credentials_by_server.sql",
        server_id,
    )
    .fetch_all(pool)
    .await?)
}

pub async fn insert_credential(
    pool: &PgPool,
    server_id: Uuid,
    label: &str,
    secret: &str,
    position: i32,
    enabled: bool,
) -> Result<McpCredential> {
    Ok(sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/insert_credential.sql",
        server_id,
        label,
        secret,
        position,
        enabled,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_credential_token(
    pool: &PgPool,
    credential_id: Uuid,
    label: &str,
    secret: &str,
    enabled: bool,
) -> Result<McpCredential> {
    Ok(sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/update_credential_token.sql",
        credential_id,
        label,
        secret,
        enabled,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn delete_credential(pool: &PgPool, credential_id: Uuid) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/mcp_credentials/delete_credential.sql",
        credential_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
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
    let mut tx = pool.begin().await?;
    // The owning MCP server is the source of truth for the credential
    // provider; generic/legacy servers resolve to NULL so they never trigger
    // provider-specific flows such as a Firecrawl balance fetch.
    let server_provider_kind = sqlx::query_file!(
        "src/sql/mcp_credentials/get_credential_provider_kind.sql",
        server_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .and_then(|row| row.provider_kind);
    let provider_kind = canonical_mcp_provider_kind(server_provider_kind.as_deref());
    let existing = sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/list_credentials_by_server.sql",
        server_id,
    )
    .fetch_all(&mut *tx)
    .await?;
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
                sqlx::query_file!(
                    "src/sql/mcp_credentials/update_credential_with_provider.sql",
                    credential.credential_id,
                    label,
                    token.token,
                    token.enabled,
                    provider_kind,
                )
                .fetch_one(&mut *tx)
                .await?;
            }
        } else {
            sqlx::query_file!(
                "src/sql/mcp_credentials/insert_credential_with_provider.sql",
                server_id,
                label,
                token.token,
                position,
                token.enabled,
                provider_kind,
            )
            .fetch_one(&mut *tx)
            .await?;
        }
    }
    for credential in existing
        .iter()
        .filter(|credential| !seen.contains(&credential.position))
    {
        sqlx::query_file!(
            "src/sql/mcp_credentials/delete_credential.sql",
            credential.credential_id,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Enabled Firecrawl credentials eligible for a provider balance refresh.
/// Only PostgreSQL stores these rows; the SQLite runtime never calls this.
pub async fn list_firecrawl_credentials(pool: &PgPool) -> Result<Vec<McpProviderSecret>> {
    Ok(sqlx::query_file_as!(
        McpProviderSecret,
        "src/sql/mcp_credentials/list_firecrawl_credentials.sql",
    )
    .fetch_all(pool)
    .await?)
}

/// Reconcile every credential's `provider_kind` with its owning MCP server.
/// Idempotent: only rows whose canonical value differs are touched. Returns
/// the number of credentials updated.
pub async fn backfill_credential_provider_kinds(pool: &PgPool) -> Result<i64> {
    let result =
        sqlx::query_file!("src/sql/mcp_credentials/backfill_credential_provider_kinds.sql")
            .execute(pool)
            .await?;
    Ok(result.rows_affected() as i64)
}
