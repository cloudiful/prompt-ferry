//! Snapshot publication glue for the unified configuration repository.
//!
//! The worker pushes a bounded runtime snapshot (client routes plus the relay
//! IP whitelist) to every connected relay bridge. Relay connection targets are
//! local worker configuration, so no relay management row participates here.

use anyhow::Result;

use super::relay_snapshot_map;
use super::{PostgresConfigRepository, SqliteConfigRepository};
use crate::bridge::protocol::{ClientRoute, RelayIpPolicy};

impl super::ConfigRepository {
    /// Look up the relay IP whitelist setting (PostgreSQL or SQLite).
    pub async fn get_relay_ip_policy(&self) -> Result<RelayIpPolicy> {
        match self {
            Self::Postgres(pg) => {
                crate::db::get_json_setting::<RelayIpPolicy>(pg.pool(), "relay_ip_whitelist")
                    .await
                    .map(|policy| policy.unwrap_or_default())
            }
            Self::Sqlite(sqlite) => {
                let setting = sqlite
                    .store
                    .get_setting("relay_ip_whitelist")
                    .await
                    .map_err(|err| anyhow::anyhow!("{err}"))?;
                Ok(match setting {
                    Some(setting) => {
                        relay_snapshot_map::policy_from_settings(std::slice::from_ref(&setting))
                    }
                    None => RelayIpPolicy::default(),
                })
            }
        }
    }
}

/// Snapshot payload returned by the unified snapshot publisher.
#[derive(Debug, Clone)]
pub struct UnifiedSnapshot {
    pub version: i64,
    pub keys: Vec<ClientRoute>,
    pub relay_ip_policy: RelayIpPolicy,
}

pub async fn build_unified_snapshot(
    repo: &super::ConfigRepository,
    version: i64,
) -> Result<UnifiedSnapshot> {
    let keys = build_snapshot_keys(repo).await?;
    let relay_ip_policy = repo.get_relay_ip_policy().await?;
    Ok(UnifiedSnapshot {
        version,
        keys,
        relay_ip_policy,
    })
}

/// Stable fingerprint of the published snapshot content.
///
/// Two workers that read the same configuration produce the same fingerprint,
/// so a refresh can tell whether the content it just read was already
/// published.
pub fn snapshot_fingerprint(snapshot: &UnifiedSnapshot) -> Result<u64> {
    content_fingerprint(&(&snapshot.keys, &snapshot.relay_ip_policy))
}

/// Whether the built content differs from the content that was published
/// last.
///
/// An unchanged content is not published again; a changed content always is.
pub fn should_publish(last_published: Option<u64>, current: u64) -> bool {
    last_published != Some(current)
}

/// Stable fingerprint of a serializable value.
pub fn content_fingerprint<T: serde::Serialize>(value: &T) -> Result<u64> {
    let encoded = serde_json::to_string(value)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in encoded.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Ok(hash)
}

pub async fn build_snapshot_keys(repo: &super::ConfigRepository) -> Result<Vec<ClientRoute>> {
    match repo {
        super::ConfigRepository::Postgres(pg) => build_snapshot_keys_postgres(pg).await,
        super::ConfigRepository::Sqlite(sqlite) => build_snapshot_keys_sqlite(sqlite).await,
    }
}

async fn build_snapshot_keys_postgres(pg: &PostgresConfigRepository) -> Result<Vec<ClientRoute>> {
    let keys = crate::db::snapshot_keys(pg.pool()).await?;
    Ok(keys
        .into_iter()
        .map(|key| ClientRoute {
            key_hash: key.key_hash,
            key_prefix: key.key_prefix,
            user_id: key.user_id,
            route_id: key.route_id.to_string(),
        })
        .collect())
}

async fn build_snapshot_keys_sqlite(sqlite: &SqliteConfigRepository) -> Result<Vec<ClientRoute>> {
    let snapshot = sqlite
        .store
        .load_snapshot(&sqlite.manager)
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))?;
    Ok(relay_snapshot_map::build_snapshot_keys_sqlite(&snapshot))
}

#[cfg(test)]
mod tests {
    use super::{UnifiedSnapshot, content_fingerprint, should_publish, snapshot_fingerprint};

    #[test]
    fn an_unchanged_content_is_not_published_again_and_a_changed_one_is() {
        assert!(!should_publish(Some(7), 7));
        assert!(should_publish(Some(7), 8));
        assert!(should_publish(None, 7));
    }

    #[test]
    fn the_fingerprint_only_depends_on_the_content() {
        let snapshot = UnifiedSnapshot {
            version: 3,
            keys: Vec::new(),
            relay_ip_policy: Default::default(),
        };
        assert_eq!(
            snapshot_fingerprint(&snapshot).expect("fingerprint"),
            content_fingerprint(&(&snapshot.keys, &snapshot.relay_ip_policy)).expect("fingerprint")
        );
    }
}
