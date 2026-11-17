//! Shared Valkey settings owned by the database.
//!
//! The URL is a persisted secret encrypted with the same worker-configuration
//! envelope as the raw object store credentials. Only the database copy is
//! authoritative: a process resolves it once at startup and keeps the applied
//! revision for the worker registry, so an edit requires a coordinated
//! restart instead of a silent hot swap.

use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::relay_secrets::{EncryptedSecretEnvelope, RelaySecretManager};

use super::entities::postgres::{worker_settings, workers};

/// `worker_settings` key holding [`SharedValkeyPersisted`].
pub const SHARED_VALKEY_SETTINGS_KEY: &str = "shared_valkey";

/// Replay snapshots, MCP catalog entries, sessions and request leases keep the
/// historical one-day default unless the shared setting says otherwise.
pub const DEFAULT_SHARED_VALKEY_TTL_SECONDS: u64 = 24 * 60 * 60;
pub const MIN_SHARED_VALKEY_TTL_SECONDS: u64 = 60;
pub const MAX_SHARED_VALKEY_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

/// Persisted database representation: the URL is encrypted, never plaintext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedValkeyPersisted {
    #[serde(default)]
    pub revision: i64,
    #[serde(default = "default_ttl_seconds")]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub url: Option<EncryptedSecretEnvelope>,
}

fn default_ttl_seconds() -> u64 {
    DEFAULT_SHARED_VALKEY_TTL_SECONDS
}

impl SharedValkeyPersisted {
    /// Encrypt a candidate plaintext URL for persistence. The caller owns both
    /// validation and revision increment so a failed write never advertises a
    /// new applied state.
    pub fn seal(
        revision: i64,
        ttl_seconds: u64,
        url: Option<&str>,
        manager: &RelaySecretManager,
    ) -> Result<Self> {
        let url = match url.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => Some(manager.encrypt(value)?),
            None => None,
        };
        Ok(Self {
            revision,
            ttl_seconds,
            url,
        })
    }
}

/// Resolved runtime view. It intentionally has no `Deserialize`
/// implementation: a worker cannot inject a local Valkey override through the
/// configuration file or environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedValkeyConfig {
    url: Option<String>,
    ttl_seconds: u64,
    revision: Option<i64>,
}

impl SharedValkeyConfig {
    /// A missing database row means the worker runs in the documented
    /// single-worker degraded mode, not that a local fallback was configured.
    pub fn not_configured() -> Self {
        Self {
            url: None,
            ttl_seconds: DEFAULT_SHARED_VALKEY_TTL_SECONDS,
            revision: None,
        }
    }

    pub fn from_persisted(
        persisted: Option<SharedValkeyPersisted>,
        manager: &RelaySecretManager,
    ) -> Result<Self> {
        let Some(persisted) = persisted else {
            return Ok(Self::not_configured());
        };
        let url = match persisted.url {
            Some(envelope) => {
                // The decryption failure never carries the encrypted value or
                // a dependency message; only the fixed reason is reported.
                let plaintext = manager
                    .decrypt(&envelope)
                    .map_err(|_| anyhow!("failed to decrypt the shared Valkey URL"))?;
                let trimmed = plaintext.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    validate_shared_valkey_url(trimmed)?;
                    Some(trimmed.to_string())
                }
            }
            None => None,
        };
        Ok(Self {
            url,
            ttl_seconds: persisted.ttl_seconds.max(1),
            revision: Some(persisted.revision),
        })
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn url_or_empty(&self) -> &str {
        self.url.as_deref().unwrap_or("")
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.ttl_seconds.max(1)
    }

    pub fn revision(&self) -> Option<i64> {
        self.revision
    }

    pub fn is_configured(&self) -> bool {
        self.url.is_some()
    }

    /// Build a resolved configuration from known values.
    ///
    /// The production path is [`Self::from_persisted`]; this constructor
    /// exists for callers that already resolved the values themselves.
    pub fn resolved(url: Option<String>, ttl_seconds: u64, revision: Option<i64>) -> Self {
        Self {
            url,
            ttl_seconds,
            revision,
        }
    }
}

/// Read the persisted settings without touching the secret envelope. A
/// decryption or parse failure is an error, never an implicit fallback.
pub async fn get_shared_valkey_persisted(pool: &PgPool) -> Result<Option<SharedValkeyPersisted>> {
    super::get_json_setting::<SharedValkeyPersisted>(pool, SHARED_VALKEY_SETTINGS_KEY).await
}

pub async fn load_shared_valkey_config(
    pool: &PgPool,
    manager: &RelaySecretManager,
) -> Result<SharedValkeyConfig> {
    let persisted = get_shared_valkey_persisted(pool).await?;
    SharedValkeyConfig::from_persisted(persisted, manager)
}

/// Desired revision straight from `worker_settings`. It stays readable without
/// the encryption key because only the revision and TTL are plain fields.
pub async fn desired_shared_valkey_revision(pool: &PgPool) -> Result<Option<i64>> {
    let db = super::seaorm::postgres_connection(pool);
    use sea_orm::EntityTrait;
    let row = worker_settings::Entity::find_by_id(SHARED_VALKEY_SETTINGS_KEY.to_string())
        .one(&db)
        .await
        .map_err(super::seaorm::native_error)?;
    Ok(row.and_then(|row| {
        row.setting_value
            .get("revision")
            .and_then(serde_json::Value::as_i64)
    }))
}

/// Advisory lock that serializes shared Valkey setting writes.
///
/// Every writer takes this lock inside its transaction, so the read of the
/// previous revision and the write of the next one are one atomic step.
pub const SHARED_VALKEY_SETTING_LOCK: i64 = 8_675_309_001;

/// Take the shared Valkey setting lock inside an open transaction.
///
/// The lock is transaction scoped, so a crashed writer never leaves the
/// setting locked. Writers take this lock before any other coordination lock
/// and never take another one while holding it.
pub async fn lock_shared_valkey_setting(conn: &mut sqlx::PgConnection) -> Result<()> {
    sqlx::query_file!(
        "src/sql/settings/lock_shared_valkey_setting.sql",
        SHARED_VALKEY_SETTING_LOCK
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Read the current shared Valkey revision inside an open transaction.
///
/// The lock must be held by the caller.
pub async fn read_shared_valkey_revision(conn: &mut sqlx::PgConnection) -> Result<Option<i64>> {
    let revision: Option<i64> = sqlx::query_file_scalar!(
        "src/sql/settings/get_shared_valkey_revision.sql",
        SHARED_VALKEY_SETTINGS_KEY
    )
    .fetch_optional(&mut *conn)
    .await?
    .flatten();
    Ok(revision)
}

/// Result of a serialized shared Valkey settings write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedValkeyWriteOutcome {
    /// The write was applied; the persisted row carries this revision.
    Written(SharedValkeyPersisted),
    /// Another writer changed the setting first, so the caller's view was
    /// stale and nothing was written.
    Conflict { current_revision: Option<i64> },
}

/// The revision a shared Valkey settings write produces.
///
/// The revision is monotonic: a write never reuses or lowers the stored
/// revision, so a worker can compare its applied revision with the desired
/// one.
pub fn next_shared_valkey_revision(current: Option<i64>) -> i64 {
    current.unwrap_or(0).saturating_add(1)
}

/// Persist the shared Valkey settings under a database lock.
///
/// The revision is read and incremented inside the same transaction as the
/// write, and the caller's expected revision is compared with the stored one.
/// A concurrent writer therefore either wins the lock first (the second
/// writer receives a conflict instead of overwriting it) or loses it.
pub async fn save_shared_valkey_settings(
    pool: &PgPool,
    manager: &RelaySecretManager,
    url_plaintext: Option<&str>,
    ttl_seconds: u64,
    expected_revision: Option<i64>,
) -> Result<SharedValkeyWriteOutcome> {
    validate_shared_valkey_ttl(ttl_seconds)?;
    if let Some(url) = url_plaintext {
        validate_shared_valkey_url(url.trim())?;
    }
    let mut transaction = pool.begin().await?;
    sqlx::query_file!(
        "src/sql/settings/lock_shared_valkey_setting.sql",
        SHARED_VALKEY_SETTING_LOCK
    )
    .execute(&mut *transaction)
    .await?;
    let current_revision: Option<i64> = sqlx::query_file!(
        "src/sql/settings/get_shared_valkey_revision.sql",
        SHARED_VALKEY_SETTINGS_KEY
    )
    .fetch_optional(&mut *transaction)
    .await?
    .map(|row| row.revision)
    .flatten();
    if current_revision != expected_revision {
        transaction.rollback().await?;
        return Ok(SharedValkeyWriteOutcome::Conflict { current_revision });
    }
    let revision = next_shared_valkey_revision(current_revision);
    let persisted = SharedValkeyPersisted::seal(revision, ttl_seconds, url_plaintext, manager)?;
    let encoded = serde_json::to_value(&persisted)?;
    sqlx::query_file!(
        "src/sql/settings/set_shared_valkey_setting.sql",
        SHARED_VALKEY_SETTINGS_KEY,
        encoded
    )
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(SharedValkeyWriteOutcome::Written(persisted))
}

/// Count live workers whose applied shared revision differs from `desired`.
/// Only a currently online row can masquerade as coherent running state; a
/// stopped or expired row is reported offline instead.
pub async fn count_workers_requiring_restart(pool: &PgPool, desired: Option<i64>) -> Result<i64> {
    let Some(desired) = desired else {
        return Ok(0);
    };
    let db = super::seaorm::postgres_connection(pool);
    use sea_orm::{ColumnTrait, Condition, EntityTrait, PaginatorTrait, QueryFilter};
    let db_now: chrono::DateTime<chrono::Utc> =
        sqlx::query_file_scalar!("src/sql/workers/database_now.sql")
            .fetch_one(pool)
            .await?
            .ok_or_else(|| anyhow!("database time query returned no row"))?;
    let offline_before = db_now - chrono::Duration::seconds(super::WORKER_OFFLINE_AFTER_SECONDS);
    let count = workers::Entity::find()
        .filter(workers::Column::StoppedAt.is_null())
        .filter(workers::Column::LastHeartbeatAt.gte(offline_before))
        .filter(
            Condition::any()
                .add(workers::Column::ConfigAppliedRevision.is_null())
                .add(workers::Column::ConfigAppliedRevision.ne(desired)),
        )
        .count(&db)
        .await
        .map_err(super::seaorm::native_error)?;
    Ok(count as i64)
}

/// Reject values a worker cannot use instead of persisting a broken shared
/// setting.
///
/// The scheme, the authority and the port are checked, so a value whose
/// authority only looks like a userinfo (`redis://user:secret`) is rejected
/// instead of being persisted and echoed later. The URL itself is parsed with
/// the parser of the HTTP client, so a malformed authority never reaches the
/// database. The error never carries the submitted value.
pub fn validate_shared_valkey_url(value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("shared Valkey URL cannot be empty");
    }
    if trimmed
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("shared Valkey URL cannot contain whitespace or control characters");
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        bail!("shared Valkey URL must include a scheme such as redis://");
    };
    if !matches!(scheme, "redis" | "rediss" | "unix") {
        bail!("shared Valkey URL scheme must be redis, rediss or unix");
    }
    // The parser rejects a malformed authority: a port that is not a number,
    // an unterminated IPv6 host or a control character. A value like
    // `redis://user:secret` is such a malformed authority: it carries a
    // non-numeric port.
    reqwest::Url::parse(trimmed)
        .map_err(|_| anyhow!("shared Valkey URL is not a valid connection URL"))?;
    // The fragment and the query never carry the authority, so both are
    // dropped before the authority is read.
    let rest = rest.split('#').next().unwrap_or("");
    let rest = match rest.split_once('?') {
        Some((before_query, _query)) => before_query,
        None => rest,
    };
    let (userinfo, authority, path) = split_shared_valkey_authority(rest);
    if let Some(userinfo) = userinfo {
        if userinfo.is_empty() {
            bail!("shared Valkey URL userinfo cannot be empty");
        }
        if userinfo.contains('/') {
            bail!("shared Valkey URL userinfo cannot contain a slash");
        }
    }
    if scheme == "unix" {
        if authority.is_empty() && path.unwrap_or_default().is_empty() {
            bail!("shared Valkey unix URL must include a socket path");
        }
        return Ok(());
    }
    validate_shared_valkey_authority(authority)?;
    Ok(())
}

/// Split the part of a shared Valkey URL that follows the scheme into its
/// userinfo, authority and path.
///
/// The userinfo ends at the last `@`, so a password may contain an `@`. A
/// value without an `@` has no userinfo at all: its authority is the host
/// with an optional port, which is why `redis://user:secret` is read as a
/// host with a non-numeric port and rejected.
fn split_shared_valkey_authority(rest: &str) -> (Option<&str>, &str, Option<&str>) {
    if let Some((userinfo, host_part)) = rest.rsplit_once('@') {
        return match host_part.split_once('/') {
            Some((host, path)) => (Some(userinfo), host, Some(path)),
            None => (Some(userinfo), host_part, None),
        };
    }
    match rest.split_once('/') {
        Some((authority, path)) => (None, authority, Some(path)),
        None => (None, rest, None),
    }
}

/// Check the host and the port of a shared Valkey authority.
fn validate_shared_valkey_authority(authority: &str) -> Result<()> {
    if authority.is_empty() {
        bail!("shared Valkey URL must include a host");
    }
    let (host, port, bracketed) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, after)) = bracketed.split_once(']') else {
            bail!("shared Valkey URL host is malformed");
        };
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None => {
                if !after.is_empty() {
                    bail!("shared Valkey URL host is malformed");
                }
                None
            }
        };
        (host, port, true)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port), false),
            None => (authority, None, false),
        }
    };
    if host.is_empty() {
        bail!("shared Valkey URL must include a host");
    }
    if !bracketed && host.contains(':') {
        bail!("shared Valkey URL host must be bracketed when it is an IPv6 address");
    }
    if let Some(port) = port {
        if port.is_empty() || !port.chars().all(|character| character.is_ascii_digit()) {
            bail!("shared Valkey URL port must be a number");
        }
        match port.parse::<u32>() {
            Ok(value) if value <= 65_535 => {}
            _ => bail!("shared Valkey URL port is out of range"),
        }
    }
    Ok(())
}

pub fn validate_shared_valkey_ttl(ttl_seconds: u64) -> Result<()> {
    if !(MIN_SHARED_VALKEY_TTL_SECONDS..=MAX_SHARED_VALKEY_TTL_SECONDS).contains(&ttl_seconds) {
        bail!(
            "shared Valkey TTL must be between {MIN_SHARED_VALKEY_TTL_SECONDS} and \
             {MAX_SHARED_VALKEY_TTL_SECONDS} seconds"
        );
    }
    Ok(())
}

/// The fixed marker that replaces every secret-bearing part of a shared
/// Valkey URL.
pub const SECRET_MASK: &str = "***";

/// Mask a URL for read responses and logs.
///
/// The marker is fixed: it never carries the scheme, the authority, the path
/// or the query of the configured value. Every part of a shared Valkey URL can
/// carry a secret (the userinfo, a database name, a socket path, an ACL
/// token), so a display value is never derived from the configured one.
pub fn mask_shared_valkey_url(_value: &str) -> String {
    SECRET_MASK.to_string()
}

/// The safe, fixed reason a shared Valkey client could not be used.
///
/// A backend error can echo the connection string or a part of it, so the
/// reason is a fixed category and never the dependency message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedValkeyFailure {
    /// The client could not be created from the configured URL.
    ClientOpen,
    /// The client could not connect to the configured Valkey.
    Connect,
    /// The connection attempt did not finish inside the bounded probe.
    Timeout,
}

impl SharedValkeyFailure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ClientOpen => "client_open_failed",
            Self::Connect => "connect_failed",
            Self::Timeout => "connect_timeout",
        }
    }
}

/// Build the error of a failed shared Valkey access.
///
/// The message carries the fixed category and the masked URL only; no
/// dependency message and no part of the configured value is included.
pub fn shared_valkey_failure(failure: SharedValkeyFailure, url: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "shared Valkey {} for {}",
        failure.as_str(),
        mask_shared_valkey_url(url)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    fn manager() -> RelaySecretManager {
        let key = base64::engine::general_purpose::STANDARD.encode([9_u8; 32]);
        RelaySecretManager::from_base64(&key).expect("key")
    }

    #[test]
    fn persisted_envelope_round_trips_and_never_exposes_the_url() {
        let manager = manager();
        let persisted = SharedValkeyPersisted::seal(
            3,
            120,
            Some("redis://user:secret@valkey:6379/0"),
            &manager,
        )
        .expect("seal");
        let encoded = serde_json::to_string(&persisted).expect("json");
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("valkey:6379"));

        let config =
            SharedValkeyConfig::from_persisted(Some(persisted), &manager).expect("resolve");
        assert_eq!(config.url(), Some("redis://user:secret@valkey:6379/0"));
        assert_eq!(config.ttl_seconds(), 120);
        assert_eq!(config.revision(), Some(3));
        assert!(config.is_configured());
    }

    #[test]
    fn a_missing_row_is_the_degraded_single_worker_state() {
        let config = SharedValkeyConfig::from_persisted(None, &manager()).expect("resolve");
        assert!(!config.is_configured());
        assert_eq!(config.revision(), None);
        assert_eq!(config.ttl_seconds(), DEFAULT_SHARED_VALKEY_TTL_SECONDS);
    }

    #[test]
    fn an_undecryptable_url_is_an_error_not_a_fallback() {
        let persisted = SharedValkeyPersisted {
            revision: 1,
            ttl_seconds: 60,
            url: Some(EncryptedSecretEnvelope {
                ciphertext: vec![1, 2, 3],
                nonce: vec![0; 12],
                key_version: 1,
            }),
        };
        assert!(SharedValkeyConfig::from_persisted(Some(persisted), &manager()).is_err());
    }

    #[test]
    fn masking_replaces_every_form_with_the_fixed_marker() {
        for value in [
            "redis://user:password@valkey:6379/2",
            "rediss://valkey:6380",
            "redis://host?password=secret",
            "redis://valkey.internal:6379/2?token=s3cret",
            "unix:///run/valkey/secret.sock",
            "redis://user:secret",
            "host:6379",
            "://host",
            "",
        ] {
            let masked = mask_shared_valkey_url(value);
            assert_eq!(masked, SECRET_MASK, "value {value:?}");
            // The marker never carries a part of the configured value.
            for secret in ["user", "password", "secret", "valkey", "token", "s3cret"] {
                assert!(
                    !masked.to_lowercase().contains(secret),
                    "value {value:?} leaked {secret}"
                );
            }
        }
    }

    #[test]
    fn a_failure_carries_the_fixed_category_and_the_masked_url_only() {
        let error = shared_valkey_failure(
            SharedValkeyFailure::Connect,
            "redis://user:password@valkey:6379/2?password=secret",
        );
        let message = error.to_string();
        assert!(message.contains("connect_failed"));
        assert!(!message.contains("password"));
        assert!(!message.contains("secret"));
        assert_eq!(
            SharedValkeyFailure::ClientOpen.as_str(),
            "client_open_failed"
        );
        assert_eq!(SharedValkeyFailure::Timeout.as_str(), "connect_timeout");
    }

    #[test]
    fn the_revision_of_a_write_is_monotonic() {
        assert_eq!(next_shared_valkey_revision(None), 1);
        assert_eq!(next_shared_valkey_revision(Some(4)), 5);
        assert_eq!(next_shared_valkey_revision(Some(i64::MAX)), i64::MAX);
    }

    #[test]
    fn url_validation_rejects_unsupported_or_malformed_values() {
        assert!(validate_shared_valkey_url("redis://valkey:6379").is_ok());
        assert!(validate_shared_valkey_url("rediss://valkey:6379").is_ok());
        assert!(validate_shared_valkey_url("unix:///tmp/valkey.sock").is_ok());
        assert!(validate_shared_valkey_url("redis://[::1]:6379").is_ok());
        assert!(validate_shared_valkey_url("redis://user:password@valkey:6379/2").is_ok());
        assert!(validate_shared_valkey_url("http://valkey").is_err());
        assert!(validate_shared_valkey_url("redis://val key").is_err());
        assert!(validate_shared_valkey_url("").is_err());
    }

    #[test]
    fn url_validation_rejects_an_authority_that_only_looks_like_a_userinfo() {
        // `redis://user:secret` has no `@`, so it is a host with a port. The
        // port is not a number, so the value is rejected instead of being
        // persisted and echoed later.
        assert!(validate_shared_valkey_url("redis://user:secret").is_err());
        assert!(validate_shared_valkey_url("redis://host:port").is_err());
        assert!(validate_shared_valkey_url("redis://host:").is_err());
        assert!(validate_shared_valkey_url("redis://host:99999").is_err());
        assert!(validate_shared_valkey_url("redis://").is_err());
        assert!(validate_shared_valkey_url("redis://@valkey:6379").is_err());
        assert!(validate_shared_valkey_url("unix://").is_err());
    }

    #[test]
    fn ttl_validation_holds_the_documented_range() {
        assert!(validate_shared_valkey_ttl(MIN_SHARED_VALKEY_TTL_SECONDS).is_ok());
        assert!(validate_shared_valkey_ttl(MAX_SHARED_VALKEY_TTL_SECONDS).is_ok());
        assert!(validate_shared_valkey_ttl(MIN_SHARED_VALKEY_TTL_SECONDS - 1).is_err());
        assert!(validate_shared_valkey_ttl(MAX_SHARED_VALKEY_TTL_SECONDS + 1).is_err());
    }
}
