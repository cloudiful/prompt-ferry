//! PostgreSQL configuration import.
//!
//! Replaces the restorable configuration domains inside one transaction. Every
//! statement is built dynamically because the archive carries arbitrary row
//! counts and the destination is a fixed, known table set; values are always
//! bound, never interpolated. Deleting and re-inserting the archive's rows
//! (with their original identifiers) makes the destination match the archive
//! while leaving request/usage/billing/session rows untouched.
//!
//! The domain inserts that live outside this file (routes targets, MCP,
//! settings, redaction) share the same transaction through
//! [`super::postgres_domains`].

use anyhow::{Context, Result};
use sea_orm::{EntityTrait, Set};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;

use super::super::snapshot::{EndpointSnapshot, ModelRouteSnapshot, SnapshotDomains};
use super::postgres_domains;
use crate::db::entities::postgres::model_endpoint_rules;
use crate::db::seaorm::PostgresNativeConnection;

const USER_ID_SEQUENCE: &str = "public.users_user_id_seq";
const CLIENT_KEY_ID_SEQUENCE: &str = "public.client_keys_key_id_seq";

/// Child-before-parent order; deleting an MCP server or endpoint first keeps
/// the cascade graph simple and never touches non-configuration tables.
const DELETE_ORDER: [&str; 10] = [
    "mcp_credentials",
    "mcp_servers",
    "model_route_targets",
    "model_endpoint_rules",
    "endpoint_api_keys",
    "endpoint_oauth_tokens",
    "provider_endpoints",
    "client_keys",
    "user_redaction_configs",
    "users",
];

pub(super) async fn apply(
    pool: &sqlx::PgPool,
    manager: &crate::relay_secrets::RelaySecretManager,
    domains: &SnapshotDomains,
) -> Result<()> {
    let mut tx = pool
        .begin()
        .await
        .context("begin configuration import transaction")?;
    let conn = &mut *tx;
    // Freeze identity writers before any row or sequence changes: a concurrent
    // INSERT cannot evaluate a default nextval() against a sequence this
    // transaction is about to restart.
    sqlx::query_file!("src/sql/config_import/lock_identity_tables.sql")
        .execute(&mut *conn)
        .await
        .context("failed to lock the identity tables for the configuration import")?;
    delete_configuration(conn).await?;
    insert_users(conn, domains).await?;
    insert_client_keys(conn, domains).await?;
    insert_endpoints(conn, domains).await?;
    postgres_domains::insert_user_endpoint_settings(conn, domains).await?;
    insert_endpoint_api_keys(conn, domains).await?;
    insert_endpoint_oauth(conn, domains).await?;
    insert_route_rules(conn, domains).await?;
    postgres_domains::insert_route_targets(conn, domains).await?;
    postgres_domains::insert_mcp_servers(conn, domains).await?;
    postgres_domains::insert_mcp_credentials(conn, domains).await?;
    // The grants are installed inside this transaction: a failed sealing
    // rolls back the whole configuration, and no grant can outlive a
    // half-restored instance.
    postgres_domains::insert_mcp_oauth_grants(conn, manager, domains).await?;
    postgres_domains::insert_settings(conn, manager, domains).await?;
    postgres_domains::insert_user_redaction_configs(conn, domains).await?;
    // Both new heads are computed before either ALTER runs so an overflow
    // fails without partially moving the identity state.
    synchronize_identity_sequences(conn).await?;
    tx.commit()
        .await
        .context("commit configuration import transaction")?;
    Ok(())
}

pub(super) async fn run(conn: &mut PgConnection, mut query: QueryBuilder<Postgres>) -> Result<()> {
    query
        .build()
        .execute(&mut *conn)
        .await
        .context("configuration import statement failed")?;
    Ok(())
}

async fn synchronize_identity_sequences(conn: &mut PgConnection) -> Result<()> {
    let heads = sqlx::query_file!("src/sql/config_import/identity_sequence_heads.sql")
        .fetch_one(&mut *conn)
        .await
        .context("failed to read the identity sequence heads")?;
    let users_next = next_identity_head(
        heads.max_user_id,
        heads.user_last_value,
        heads.user_is_called,
    )?;
    let client_keys_next = next_identity_head(
        heads.max_client_key_id,
        heads.client_key_last_value,
        heads.client_key_is_called,
    )?;
    restart_identity_sequence(conn, USER_ID_SEQUENCE, users_next).await?;
    restart_identity_sequence(conn, CLIENT_KEY_ID_SEQUENCE, client_keys_next).await?;
    Ok(())
}

/// Next value a sequence must be able to hand out without colliding with the
/// imported rows or rewinding an already-advanced destination.
///
/// `last_value`/`is_called` come from the sequence relation itself: an
/// uncalled sequence keeps its advanced head (`is_called = false` means the
/// next `nextval` returns exactly `last_value`), and an ordinary called
/// sequence continues at `last_value + 1`. Gaps are allowed; a rewind is not.
fn next_identity_head(max_id: Option<i64>, last_value: i64, is_called: bool) -> Result<i64> {
    let imported_next = match max_id {
        Some(max_id) => max_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("imported identity exceeds the sequence range"))?,
        None => 1,
    };
    let current_next = if is_called {
        last_value
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("current identity head exceeds the sequence range"))?
    } else {
        last_value
    };
    Ok(imported_next.max(current_next).max(1))
}

async fn restart_identity_sequence(
    conn: &mut PgConnection,
    sequence: &'static str,
    next: i64,
) -> Result<()> {
    // ALTER SEQUENCE is utility DDL and cannot take a bind parameter; the two
    // identifiers are fixed constants and the value is a checked positive i64.
    let mut query = QueryBuilder::new("ALTER SEQUENCE ");
    query
        .push(sequence)
        .push(" RESTART WITH ")
        .push(next.to_string());
    run(conn, query).await
}

async fn delete_configuration(conn: &mut PgConnection) -> Result<()> {
    for table in DELETE_ORDER {
        run(conn, QueryBuilder::new(format!("DELETE FROM {table}"))).await?;
    }
    let excluded_keys: Vec<String> = crate::db::INTERNAL_COORDINATION_SETTING_KEYS
        .iter()
        .map(|key| key.to_string())
        .collect();
    sqlx::query_file!(
        "src/sql/settings/delete_restorable_settings.sql",
        &excluded_keys
    )
    .execute(&mut *conn)
    .await
    .context("failed to replace the restorable settings rows")?;
    Ok(())
}

/// The archive stores PostgreSQL client keys as a UUID derived from the
/// bigint identity; recover the original identity so usage references survive.
fn client_key_id(key_id: Uuid) -> i64 {
    key_id.as_u64_pair().0 as i64
}

async fn insert_users(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.users.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO users (user_id, login_name, password_hash, display_name, is_admin, \
         is_active, created_at, updated_at) ",
    );
    query.push_values(&domains.users, |mut row, user| {
        row.push_bind(user.user_id)
            .push_bind(user.login_name.clone())
            .push_bind(user.password_hash.clone().unwrap_or_default())
            .push_bind(user.display_name.clone())
            .push_bind(user.is_admin)
            .push_bind(user.is_active)
            .push_bind(user.created_at)
            .push_bind(user.updated_at);
    });
    run(conn, query).await
}

async fn insert_client_keys(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.client_keys.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO client_keys (key_id, user_id, key_prefix, key_hash, label, enabled, secret, \
         created_at) ",
    );
    query.push_values(&domains.client_keys, |mut row, key| {
        let hash = key
            .key_hash
            .clone()
            .or_else(|| key.secret.as_deref().map(crate::keys::hash_client_key));
        row.push_bind(client_key_id(key.key_id))
            .push_bind(key.user_id)
            .push_bind(key.key_prefix.clone())
            .push_bind(hash)
            .push_bind(key.label.clone())
            .push_bind(key.enabled)
            .push_bind(key.secret.clone())
            .push_bind(key.created_at.unwrap_or_else(chrono::Utc::now));
    });
    run(conn, query).await
}

async fn insert_endpoints(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    if domains.endpoints.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO provider_endpoints (endpoint_id, scope, owner_user_id, name, base_url, \
         native_api, native_api_source, api_key, enabled, key_lb_enabled, provider, \
         provider_region, mcp_enabled, service_tier, proxy_url, active_windows, admin_api_key, \
         created_at, updated_at) ",
    );
    query.push_values(&domains.endpoints, |mut row, endpoint| {
        row.push_bind(endpoint.endpoint_id)
            .push_bind(endpoint.scope.clone())
            .push_bind(endpoint.owner_user_id)
            .push_bind(endpoint.name.clone())
            .push_bind(endpoint.base_url.clone())
            .push_bind(endpoint.native_api.clone())
            .push_bind(endpoint.native_api_source.clone())
            .push_bind(fallback_api_key(endpoint))
            .push_bind(endpoint.enabled)
            .push_bind(endpoint.key_lb_enabled)
            .push_bind(endpoint.provider.clone())
            .push_bind(endpoint.provider_region.clone())
            .push_bind(endpoint.mcp_enabled)
            .push_bind(endpoint.service_tier.clone())
            .push_bind(endpoint.proxy_url.clone())
            .push_bind(endpoint.active_windows.clone())
            .push_bind(endpoint.admin_api_key.clone())
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

fn fallback_api_key(endpoint: &EndpointSnapshot) -> String {
    endpoint
        .api_key
        .clone()
        .or_else(|| {
            endpoint
                .api_keys
                .first()
                .and_then(|key| key.api_key.clone())
        })
        .unwrap_or_default()
}

async fn insert_endpoint_api_keys(
    conn: &mut PgConnection,
    domains: &SnapshotDomains,
) -> Result<()> {
    let mut keys = Vec::new();
    for endpoint in &domains.endpoints {
        for key in &endpoint.api_keys {
            keys.push((endpoint, key));
        }
    }
    if keys.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO endpoint_api_keys (key_id, endpoint_id, key_label, api_key, position, \
         enabled, created_at, updated_at) ",
    );
    query.push_values(keys, |mut row, (endpoint, key)| {
        row.push_bind(key.key_id)
            .push_bind(endpoint.endpoint_id)
            .push_bind(key.key_label.clone())
            .push_bind(key.api_key.clone().unwrap_or_default())
            .push_bind(key.position)
            .push_bind(key.enabled)
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

async fn insert_endpoint_oauth(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    let tokens: Vec<&EndpointSnapshot> = domains
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.oauth.is_some())
        .collect();
    if tokens.is_empty() {
        return Ok(());
    }
    let mut query = QueryBuilder::new(
        "INSERT INTO endpoint_oauth_tokens (endpoint_id, access_token, refresh_token, expires_at, \
         created_at, updated_at) ",
    );
    query.push_values(tokens, |mut row, endpoint| {
        let oauth = endpoint
            .oauth
            .as_ref()
            .expect("filtered to oauth endpoints");
        row.push_bind(endpoint.endpoint_id)
            .push_bind(oauth.access_token.clone())
            .push_bind(oauth.refresh_token.clone())
            .push_bind(oauth.expires_at)
            .push_bind(endpoint.created_at)
            .push_bind(endpoint.updated_at);
    });
    run(conn, query).await
}

/// Route rules reference their first target's endpoint through the legacy
/// `endpoint_id` column; rules without targets cannot be restored and are
/// skipped (the export never produces them).
async fn insert_route_rules(conn: &mut PgConnection, domains: &SnapshotDomains) -> Result<()> {
    let routes: Vec<&ModelRouteSnapshot> = domains
        .model_routes
        .iter()
        .filter(|route| !route.targets.is_empty())
        .collect();
    if routes.is_empty() {
        return Ok(());
    }
    let connection = PostgresNativeConnection::new(&mut *conn);
    for route in routes {
        let now = route
            .updated_at
            .unwrap_or(route.created_at.unwrap_or_else(chrono::Utc::now));
        model_endpoint_rules::Entity::insert(model_endpoint_rules::ActiveModel {
            rule_id: Set(route.rule_id),
            scope: Set(route.scope.clone()),
            owner_user_id: Set(route.owner_user_id),
            model_pattern: Set(route.model_pattern.clone()),
            routing_strategy: Set(route.routing_strategy.clone()),
            endpoint_id: Set(route.targets[0].endpoint_id),
            priority: Set(100),
            enabled: Set(route.enabled),
            created_at: Set(route.created_at.unwrap_or(now)),
            updated_at: Set(now),
        })
        .exec(&connection)
        .await
        .map_err(crate::db::seaorm::native_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::next_identity_head;

    #[test]
    fn an_empty_destination_with_an_uncalled_sequence_starts_at_one() {
        assert_eq!(next_identity_head(None, 1, false).expect("head"), 1);
        assert_eq!(next_identity_head(None, 1, true).expect("head"), 2);
    }

    #[test]
    fn imported_ids_advance_past_the_maximum() {
        assert_eq!(
            next_identity_head(Some(1000), 1, false).expect("head"),
            1001
        );
        assert_eq!(
            next_identity_head(Some(1000), 1000, true).expect("head"),
            1001
        );
    }

    #[test]
    fn an_advanced_uncalled_head_is_never_rewound() {
        assert_eq!(
            next_identity_head(Some(1), 5000, false).expect("head"),
            5000
        );
        assert_eq!(next_identity_head(None, 5000, false).expect("head"), 5000);
    }

    #[test]
    fn an_advanced_called_head_continues_after_it() {
        assert_eq!(next_identity_head(Some(1), 5000, true).expect("head"), 5001);
    }

    #[test]
    fn null_or_negative_import_ids_cannot_rewind_below_one() {
        assert_eq!(next_identity_head(Some(-10), -5, false).expect("head"), 1);
        assert_eq!(next_identity_head(None, -50, false).expect("head"), 1);
        assert_eq!(next_identity_head(Some(-1), -50, true).expect("head"), 1);
    }

    #[test]
    fn checked_overflow_rejects_the_import() {
        assert!(next_identity_head(Some(i64::MAX), 1, true).is_err());
        assert!(next_identity_head(Some(1), i64::MAX, true).is_err());
        assert_eq!(
            next_identity_head(Some(1), i64::MAX, false).expect("uncalled head"),
            i64::MAX
        );
    }
}
