use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{McpBearerToken, McpCredential};

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
    quota_group_id: Option<Uuid>,
) -> Result<McpCredential> {
    Ok(sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/insert_credential.sql",
        server_id,
        label,
        secret,
        position,
        enabled,
        quota_group_id,
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
/// removed positions are deleted, and matching positions keep their existing
/// quota configuration while the token text, label and enabled flag follow the
/// configuration.
pub async fn sync_credentials_from_tokens(
    pool: &PgPool,
    server_id: Uuid,
    tokens_json: &serde_json::Value,
) -> Result<()> {
    let tokens = McpBearerToken::parse_array(tokens_json);
    let mut tx = pool.begin().await?;
    let existing = sqlx::query_file_as!(
        McpCredential,
        "src/sql/mcp_credentials/list_credentials_by_server.sql",
        server_id,
    )
    .fetch_all(&mut *tx)
    .await?;
    // Newly added tokens inherit the server's default quota group so that a
    // post-upgrade token addition does not silently opt the server out of
    // budget enforcement.
    let default_group = sqlx::query_file!(
        "src/sql/mcp_credentials/find_default_group_for_server.sql",
        server_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .map(|row| row.group_id);
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
            {
                sqlx::query_file!(
                    "src/sql/mcp_credentials/update_credential_token.sql",
                    credential.credential_id,
                    label,
                    token.token,
                    token.enabled,
                )
                .fetch_one(&mut *tx)
                .await?;
            }
        } else {
            sqlx::query_file!(
                "src/sql/mcp_credentials/insert_credential.sql",
                server_id,
                label,
                token.token,
                position,
                token.enabled,
                default_group,
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

pub async fn get_quota_group(
    pool: &PgPool,
    group_id: Uuid,
) -> Result<Option<crate::db::McpQuotaGroup>> {
    Ok(sqlx::query_file_as!(
        crate::db::McpQuotaGroup,
        "src/sql/mcp_credentials/get_quota_group.sql",
        group_id,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn list_quota_groups(pool: &PgPool) -> Result<Vec<crate::db::McpQuotaGroup>> {
    Ok(sqlx::query_file_as!(
        crate::db::McpQuotaGroup,
        "src/sql/mcp_credentials/list_quota_groups.sql",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn create_quota_group(
    pool: &PgPool,
    input: crate::db::McpQuotaGroupInput,
) -> Result<crate::db::McpQuotaGroup> {
    Ok(sqlx::query_file_as!(
        crate::db::McpQuotaGroup,
        "src/sql/mcp_credentials/create_quota_group.sql",
        input.name,
        input.scope.as_deref().unwrap_or("admin"),
        input.owner_user_id,
        input.provider_kind,
        input
            .unit
            .unwrap_or(crate::db::QuotaUnit::Requests)
            .as_str(),
        input.daily_limit,
        input.monthly_limit,
        input.default_cost.unwrap_or(1.0),
        input.strict_mode.unwrap_or(false),
        input.billing_period_start,
        input.billing_period_end,
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_quota_group(
    pool: &PgPool,
    group_id: Uuid,
    input: crate::db::McpQuotaGroupInput,
) -> Result<Option<crate::db::McpQuotaGroup>> {
    Ok(sqlx::query_file_as!(
        crate::db::McpQuotaGroup,
        "src/sql/mcp_credentials/update_quota_group.sql",
        group_id,
        input.name,
        input.scope.as_deref().unwrap_or("admin"),
        input.owner_user_id,
        input.provider_kind,
        input
            .unit
            .unwrap_or(crate::db::QuotaUnit::Requests)
            .as_str(),
        input.daily_limit,
        input.monthly_limit,
        input.default_cost.unwrap_or(1.0),
        input.strict_mode.unwrap_or(false),
        input.billing_period_start,
        input.billing_period_end,
    )
    .fetch_optional(pool)
    .await?)
}

pub async fn delete_quota_group(pool: &PgPool, group_id: Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/mcp_credentials/delete_quota_group.sql", group_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn set_credential_quota_group(
    pool: &PgPool,
    credential_id: Uuid,
    quota_group_id: Option<Uuid>,
) -> Result<bool> {
    let result = sqlx::query_file!(
        "src/sql/mcp_credentials/set_credential_quota_group.sql",
        credential_id,
        quota_group_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(result.is_some())
}
