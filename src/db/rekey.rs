use anyhow::{Result, anyhow, bail};
use sea_orm::sea_query::{Alias, Cond, Expr, ExprTrait, Order, Query};
use sea_orm::{
    AccessMode, ConnectionTrait, DatabaseTransaction, IsolationLevel, QueryResult, TransactionTrait,
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::maintenance::rekey::{decide_rekey, rekey_envelope, rekey_shared_valkey_value};
use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

const PAGE_SIZE: u64 = 100;
const REKEY_LOCK_KEY: i64 = 8_675_309_064;
const FENCING: &str = "Registry-only fencing; operator must stop old nodes, and pre-protocol binaries cannot be fenced.";

#[derive(Debug, Default)]
pub struct OwnerRekeyReport {
    pub table: &'static str,
    pub present: bool,
    pub rows: u64,
    pub envelopes: u64,
}

#[derive(Debug, Default)]
pub struct RekeyReport {
    pub same_key: bool,
    pub live_workers: Option<u64>,
    pub owners: Vec<OwnerRekeyReport>,
    pub fencing: &'static str,
}

fn id(name: &str) -> Alias {
    Alias::new(name)
}
fn failure() -> anyhow::Error {
    anyhow!("rekey database operation failed")
}

async fn exists(tx: &DatabaseTransaction, table: &str) -> Result<bool> {
    let q = Query::select()
        .expr_as(
            Expr::cust_with_values("to_regclass($1) IS NOT NULL", [table]),
            id("present"),
        )
        .to_owned();
    let row = tx
        .query_one(&q)
        .await
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    row.try_get("", "present").map_err(|_| failure())
}

async fn legacy_columns_exist(tx: &DatabaseTransaction) -> Result<bool> {
    let q = Query::select()
        .expr(Expr::val(1))
        .from((id("pg_catalog"), id("pg_attribute")))
        .and_where(Expr::cust(
            "attrelid = to_regclass('request_records') AND attnum > 0 AND NOT attisdropped",
        ))
        .and_where(Expr::col(id("attname")).is_in([
            "restore_session_ciphertext",
            "restore_session_nonce",
            "restore_session_key_version",
        ]))
        .limit(1)
        .to_owned();
    Ok(tx.query_one(&q).await.map_err(|_| failure())?.is_some())
}

async fn acquire(tx: &DatabaseTransaction) -> Result<()> {
    let q = Query::select()
        .expr_as(
            Expr::cust_with_values("pg_try_advisory_xact_lock($1)", [REKEY_LOCK_KEY]),
            id("acquired"),
        )
        .to_owned();
    let row = tx
        .query_one(&q)
        .await
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    if !row.try_get::<bool>("", "acquired").map_err(|_| failure())? {
        bail!("another rekey is running");
    }
    Ok(())
}

async fn lock(tx: &DatabaseTransaction, sql: &'static str) -> Result<()> {
    tx.execute_unprepared(sql)
        .await
        .map_err(|_| anyhow!("rekey table lock failed or timed out"))?;
    Ok(())
}

async fn live_count(tx: &DatabaseTransaction) -> Result<u64> {
    let q = Query::select()
        .expr_as(Expr::cust("COUNT(*)"), id("count"))
        .from(id("workers"))
        .and_where(Expr::col(id("stopped_at")).is_null())
        .and_where(Expr::cust_with_values(
            "last_heartbeat_at >= now() - ($1 * interval '1 second')",
            [super::WORKER_OFFLINE_AFTER_SECONDS],
        ))
        .to_owned();
    let row = tx
        .query_one(&q)
        .await
        .map_err(|_| failure())?
        .ok_or_else(failure)?;
    let count: i64 = row.try_get("", "count").map_err(|_| failure())?;
    u64::try_from(count).map_err(|_| failure())
}

fn envelope(row: &QueryResult, prefix: &str) -> Result<EncryptedSecretEnvelope> {
    Ok(EncryptedSecretEnvelope {
        ciphertext: row
            .try_get("", &format!("{prefix}_ciphertext"))
            .map_err(|_| failure())?,
        nonce: row
            .try_get("", &format!("{prefix}_nonce"))
            .map_err(|_| failure())?,
        key_version: row
            .try_get("", &format!("{prefix}_key_version"))
            .map_err(|_| failure())?,
    })
}

fn rekey_json<T: serde::de::DeserializeOwned>(
    envelope: &EncryptedSecretEnvelope,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<EncryptedSecretEnvelope> {
    let plaintext = old
        .decrypt(envelope)
        .map_err(|_| anyhow!("invalid encrypted envelope"))?;
    let _: T =
        serde_json::from_str(&plaintext).map_err(|_| anyhow!("invalid encrypted payload"))?;
    rekey_envelope(envelope, old, new)
}

fn optional_envelope(row: &QueryResult, prefix: &str) -> Result<Option<EncryptedSecretEnvelope>> {
    let ciphertext: Option<Vec<u8>> = row
        .try_get("", &format!("{prefix}_ciphertext"))
        .map_err(|_| failure())?;
    let nonce: Option<Vec<u8>> = row
        .try_get("", &format!("{prefix}_nonce"))
        .map_err(|_| failure())?;
    let version: Option<i16> = row
        .try_get("", &format!("{prefix}_key_version"))
        .map_err(|_| failure())?;
    optional_parts(ciphertext, nonce, version)
}

fn optional_parts(
    ciphertext: Option<Vec<u8>>,
    nonce: Option<Vec<u8>>,
    version: Option<i16>,
) -> Result<Option<EncryptedSecretEnvelope>> {
    match (ciphertext, nonce, version) {
        (None, None, None) => Ok(None),
        (Some(ciphertext), Some(nonce), Some(key_version)) => Ok(Some(EncryptedSecretEnvelope {
            ciphertext,
            nonce,
            key_version,
        })),
        _ => bail!("incomplete optional encrypted envelope"),
    }
}

fn columns(query: &mut sea_orm::sea_query::SelectStatement, prefix: &str) {
    for suffix in ["ciphertext", "nonce", "key_version"] {
        query.column(id(&format!("{prefix}_{suffix}")));
    }
}
fn assign(
    query: &mut sea_orm::sea_query::UpdateStatement,
    prefix: &str,
    envelope: EncryptedSecretEnvelope,
) {
    query.value(id(&format!("{prefix}_ciphertext")), envelope.ciphertext);
    query.value(id(&format!("{prefix}_nonce")), envelope.nonce);
    query.value(id(&format!("{prefix}_key_version")), envelope.key_version);
}
async fn update(
    tx: &DatabaseTransaction,
    query: &sea_orm::sea_query::UpdateStatement,
) -> Result<()> {
    if tx
        .execute(query)
        .await
        .map_err(|_| failure())?
        .rows_affected()
        != 1
    {
        bail!("unexpected rekey update count");
    }
    Ok(())
}

async fn credentials(
    tx: &DatabaseTransaction,
    apply: bool,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
    report: &mut OwnerRekeyReport,
) -> Result<()> {
    let mut cursor: Option<(Uuid, i64)> = None;
    loop {
        let mut q = Query::select();
        q.from(id("mcp_oauth_credentials"))
            .column(id("server_id"))
            .column(id("owner_user_id"));
        columns(&mut q, "registration");
        columns(&mut q, "credentials");
        if let Some((server, owner)) = cursor {
            q.cond_where(
                Cond::any().add(Expr::col(id("server_id")).gt(server)).add(
                    Cond::all()
                        .add(Expr::col(id("server_id")).eq(server))
                        .add(Expr::col(id("owner_user_id")).gt(owner)),
                ),
            );
        }
        q.order_by(id("server_id"), Order::Asc)
            .order_by(id("owner_user_id"), Order::Asc)
            .limit(PAGE_SIZE);
        let rows = tx.query_all(&q).await.map_err(|_| failure())?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let server: Uuid = row.try_get("", "server_id").map_err(|_| failure())?;
            let owner: i64 = row.try_get("", "owner_user_id").map_err(|_| failure())?;
            let registration = rekey_json::<super::McpOAuthRegistration>(
                &envelope(&row, "registration")?,
                old,
                new,
            )?;
            let credentials = rekey_json::<super::McpOAuthStoredCredentials>(
                &envelope(&row, "credentials")?,
                old,
                new,
            )?;
            if apply {
                let mut q = Query::update();
                q.table(id("mcp_oauth_credentials"));
                assign(&mut q, "registration", registration);
                assign(&mut q, "credentials", credentials);
                q.and_where(Expr::col(id("server_id")).eq(server))
                    .and_where(Expr::col(id("owner_user_id")).eq(owner));
                update(tx, &q).await?;
            }
            cursor = Some((server, owner));
            report.rows += 1;
            report.envelopes += 2;
        }
    }
    Ok(())
}

async fn states(
    tx: &DatabaseTransaction,
    apply: bool,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
    report: &mut OwnerRekeyReport,
) -> Result<()> {
    let mut cursor: Option<String> = None;
    loop {
        let mut q = Query::select();
        q.from(id("mcp_oauth_states")).column(id("state_key_hash"));
        columns(&mut q, "authorization");
        columns(&mut q, "pending_client");
        if let Some(ref cursor) = cursor {
            q.and_where(Expr::col(id("state_key_hash")).gt(cursor.clone()));
        }
        q.order_by(id("state_key_hash"), Order::Asc)
            .limit(PAGE_SIZE);
        let rows = tx.query_all(&q).await.map_err(|_| failure())?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let key: String = row.try_get("", "state_key_hash").map_err(|_| failure())?;
            let authorization = rekey_json::<super::McpOAuthAuthorizationState>(
                &envelope(&row, "authorization")?,
                old,
                new,
            )?;
            let pending = optional_envelope(&row, "pending_client")?
                .map(|e| rekey_json::<super::McpOAuthRegistration>(&e, old, new))
                .transpose()?;
            report.envelopes += 1 + u64::from(pending.is_some());
            if apply {
                let mut q = Query::update();
                q.table(id("mcp_oauth_states"));
                assign(&mut q, "authorization", authorization);
                if let Some(pending) = pending {
                    assign(&mut q, "pending_client", pending);
                }
                q.and_where(Expr::col(id("state_key_hash")).eq(key.clone()));
                update(tx, &q).await?;
            }
            cursor = Some(key);
            report.rows += 1;
        }
    }
    Ok(())
}

async fn sessions(
    tx: &DatabaseTransaction,
    apply: bool,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
    report: &mut OwnerRekeyReport,
) -> Result<()> {
    let mut cursor: Option<Uuid> = None;
    loop {
        let mut q = Query::select();
        q.from(id("conversation_redaction_sessions"))
            .column(id("conversation_id"));
        columns(&mut q, "session");
        if let Some(cursor) = cursor {
            q.and_where(Expr::col(id("conversation_id")).gt(cursor));
        }
        q.order_by(id("conversation_id"), Order::Asc)
            .limit(PAGE_SIZE);
        let rows = tx.query_all(&q).await.map_err(|_| failure())?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let key: Uuid = row.try_get("", "conversation_id").map_err(|_| failure())?;
            let session = rekey_json::<crate::redact_upstream::UpstreamRedactionSession>(
                &envelope(&row, "session")?,
                old,
                new,
            )?;
            if apply {
                let mut q = Query::update();
                q.table(id("conversation_redaction_sessions"));
                assign(&mut q, "session", session);
                q.and_where(Expr::col(id("conversation_id")).eq(key));
                update(tx, &q).await?;
            }
            cursor = Some(key);
            report.rows += 1;
            report.envelopes += 1;
        }
    }
    Ok(())
}

async fn settings(
    tx: &DatabaseTransaction,
    apply: bool,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
    report: &mut OwnerRekeyReport,
) -> Result<()> {
    let q = Query::select()
        .from(id("worker_settings"))
        .column(id("setting_key"))
        .column(id("setting_value"))
        .and_where(Expr::col(id("setting_key")).eq("shared_valkey"))
        .to_owned();
    for row in tx.query_all(&q).await.map_err(|_| failure())? {
        let key: String = row.try_get("", "setting_key").map_err(|_| failure())?;
        let mut value: serde_json::Value =
            row.try_get("", "setting_value").map_err(|_| failure())?;
        let count = match key.as_str() {
            "shared_valkey" => rekey_shared_valkey_value(&mut value, old, new)?,
            _ => bail!("unexpected settings key"),
        };
        if apply && count > 0 {
            let mut q = Query::update();
            q.table(id("worker_settings"))
                .value(id("setting_value"), value)
                .and_where(Expr::col(id("setting_key")).eq(key));
            update(tx, &q).await?;
        }
        report.rows += 1;
        report.envelopes += count as u64;
    }
    Ok(())
}

pub async fn rekey_database(
    pool: &PgPool,
    apply: bool,
    acknowledged: bool,
    old: &RelaySecretManager,
    new: &RelaySecretManager,
) -> Result<RekeyReport> {
    if old.same_key(new) {
        return Ok(RekeyReport {
            same_key: true,
            fencing: FENCING,
            ..Default::default()
        });
    }
    if apply && !acknowledged {
        bail!("apply requires acknowledgement that old nodes are stopped");
    }
    let db = super::seaorm::postgres_connection(pool);
    let tx = db
        .begin_with_config(
            Some(if apply {
                IsolationLevel::ReadCommitted
            } else {
                IsolationLevel::RepeatableRead
            }),
            Some(if apply {
                AccessMode::ReadWrite
            } else {
                AccessMode::ReadOnly
            }),
        )
        .await
        .map_err(|_| failure())?;
    acquire(&tx).await?;
    if apply {
        lock(&tx, "SET LOCAL lock_timeout = '2s'").await?;
    }
    let workers = exists(&tx, "workers").await?;
    let setting = exists(&tx, "worker_settings").await?;
    let credential = exists(&tx, "mcp_oauth_credentials").await?;
    let state = exists(&tx, "mcp_oauth_states").await?;
    let session = exists(&tx, "conversation_redaction_sessions").await?;
    if exists(&tx, "managed_relays").await? || legacy_columns_exist(&tx).await? {
        bail!("unsupported legacy encrypted schema");
    }
    if apply {
        if !workers || !setting {
            bail!("required rekey tables are missing");
        }
        lock(&tx, "LOCK TABLE workers IN ACCESS EXCLUSIVE MODE").await?;
        if credential {
            lock(
                &tx,
                "LOCK TABLE mcp_oauth_credentials IN ACCESS EXCLUSIVE MODE",
            )
            .await?;
        }
        if state {
            lock(&tx, "LOCK TABLE mcp_oauth_states IN ACCESS EXCLUSIVE MODE").await?;
        }
        lock(&tx, "LOCK TABLE worker_settings IN ACCESS EXCLUSIVE MODE").await?;
        if session {
            lock(
                &tx,
                "LOCK TABLE conversation_redaction_sessions IN ACCESS EXCLUSIVE MODE",
            )
            .await?;
        }
    }
    let live = if workers {
        Some(live_count(&tx).await?)
    } else {
        None
    };
    decide_rekey(false, apply, acknowledged, live)?;
    let mut report = RekeyReport {
        same_key: false,
        live_workers: live,
        owners: Vec::new(),
        fencing: FENCING,
    };
    let mut item = OwnerRekeyReport {
        table: "mcp_oauth_credentials",
        present: credential,
        ..Default::default()
    };
    if credential {
        credentials(&tx, apply, old, new, &mut item).await?;
    }
    report.owners.push(item);
    let mut item = OwnerRekeyReport {
        table: "mcp_oauth_states",
        present: state,
        ..Default::default()
    };
    if state {
        states(&tx, apply, old, new, &mut item).await?;
    }
    report.owners.push(item);
    let mut item = OwnerRekeyReport {
        table: "worker_settings",
        present: setting,
        ..Default::default()
    };
    if setting {
        settings(&tx, apply, old, new, &mut item).await?;
    }
    report.owners.push(item);
    let mut item = OwnerRekeyReport {
        table: "conversation_redaction_sessions",
        present: session,
        ..Default::default()
    };
    if session {
        sessions(&tx, apply, old, new, &mut item).await?;
    }
    report.owners.push(item);
    tx.commit().await.map_err(|_| failure())?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_envelopes_require_complete_triples() {
        assert!(optional_parts(None, None, None).unwrap().is_none());
        assert!(optional_parts(Some(vec![1]), None, Some(1)).is_err());
        assert!(
            optional_parts(Some(vec![1]), Some(vec![2]), Some(1))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn json_payload_validation_is_sanitized() {
        let old = RelaySecretManager::from_key_bytes([1; 32]);
        let new = RelaySecretManager::from_key_bytes([2; 32]);
        let secret = "sensitive-not-json";
        let envelope = old.encrypt(secret).unwrap();
        let error =
            rekey_json::<super::super::McpOAuthRegistration>(&envelope, &old, &new).unwrap_err();
        assert!(!format!("{error:#}").contains(secret));
    }
}
