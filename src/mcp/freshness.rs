//! Freshness policy for a cached MCP catalog.
//!
//! Every cached catalog carries two independent timestamps: the configuration
//! generation it was fetched for (`mcp_servers.updated_at`) and the moment it
//! was acquired from the upstream (`fetched_at`). The first one fences a
//! snapshot to exactly one server configuration, the second one decides
//! whether the snapshot may still be served.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

use super::cache::ServerCatalogSnapshot;

/// A snapshot younger than this is served without contacting the upstream.
pub const DEFAULT_SOFT_FRESHNESS: Duration = Duration::from_secs(5 * 60);

/// A snapshot older than this is never served: the read has to recover the
/// catalog from the upstream first.
pub const DEFAULT_MAX_SERVING_AGE: Duration = Duration::from_secs(30 * 60);

/// How fresh one cached catalog is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogFreshness {
    /// No snapshot exists for the current server configuration.
    Missing,
    /// The snapshot is younger than the soft freshness window.
    Fresh,
    /// The snapshot is older than the soft freshness window but still inside
    /// the maximum serving age. It may be served while one refresh runs.
    Stale,
    /// The snapshot is older than the maximum serving age, or it predates the
    /// freshness envelope and therefore carries no acquisition timestamp.
    Expired,
}

impl CatalogFreshness {
    /// Whether a snapshot in this state may be served to a caller.
    pub fn is_servable(self) -> bool {
        matches!(self, Self::Fresh | Self::Stale)
    }
}

/// The serving-age policy every catalog backend enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogFreshnessPolicy {
    soft_freshness: Duration,
    max_serving_age: Duration,
}

impl CatalogFreshnessPolicy {
    /// The production policy: five minutes of soft freshness and thirty
    /// minutes as the maximum age of a successfully served catalog.
    pub const fn production() -> Self {
        Self {
            soft_freshness: DEFAULT_SOFT_FRESHNESS,
            max_serving_age: DEFAULT_MAX_SERVING_AGE,
        }
    }

    pub const fn new(soft_freshness: Duration, max_serving_age: Duration) -> Self {
        Self {
            soft_freshness,
            max_serving_age,
        }
    }

    pub const fn soft_freshness(&self) -> Duration {
        self.soft_freshness
    }

    pub const fn max_serving_age(&self) -> Duration {
        self.max_serving_age
    }

    /// Classify one acquisition timestamp against the policy.
    ///
    /// A payload without an acquisition timestamp predates the freshness
    /// envelope: it is expired, never fresh.
    pub fn evaluate(
        &self,
        fetched_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> CatalogFreshness {
        let Some(fetched_at) = fetched_at else {
            return CatalogFreshness::Expired;
        };
        let age_millis = now
            .timestamp_millis()
            .saturating_sub(fetched_at.timestamp_millis())
            .max(0);
        let age = Duration::from_millis(age_millis as u64);
        if age < self.soft_freshness {
            CatalogFreshness::Fresh
        } else if age <= self.max_serving_age {
            CatalogFreshness::Stale
        } else {
            CatalogFreshness::Expired
        }
    }
}

impl Default for CatalogFreshnessPolicy {
    fn default() -> Self {
        Self::production()
    }
}

/// The canonical fingerprint of one catalog payload.
///
/// The fingerprint covers the catalog content only, never the acquisition
/// timestamp: a refresh that returns the same upstream content keeps the same
/// fingerprint, so a metadata-only change stays distinguishable from a real
/// content change.
pub fn content_fingerprint(snapshot: &ServerCatalogSnapshot) -> String {
    let payload = serde_json::to_vec(snapshot).unwrap_or_default();
    hex_digest(&Sha256::digest(&payload))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn snapshot(name: &str) -> ServerCatalogSnapshot {
        ServerCatalogSnapshot {
            tools: vec![serde_json::json!({ "name": name })],
            resources: Vec::new(),
            resource_templates: Vec::new(),
            prompts: Vec::new(),
        }
    }

    #[test]
    fn production_policy_uses_the_documented_windows() {
        let policy = CatalogFreshnessPolicy::production();
        assert_eq!(policy.soft_freshness(), Duration::from_secs(5 * 60));
        assert_eq!(policy.max_serving_age(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn a_missing_acquisition_timestamp_is_expired() {
        let policy = CatalogFreshnessPolicy::production();
        assert_eq!(policy.evaluate(None, Utc::now()), CatalogFreshness::Expired);
    }

    #[test]
    fn every_age_boundary_is_classified() {
        let policy =
            CatalogFreshnessPolicy::new(Duration::from_secs(300), Duration::from_secs(1800));
        let now = Utc::now();

        let at = |seconds: i64| now - TimeDelta::seconds(seconds);
        assert_eq!(policy.evaluate(Some(at(0)), now), CatalogFreshness::Fresh);
        assert_eq!(policy.evaluate(Some(at(299)), now), CatalogFreshness::Fresh);
        assert_eq!(policy.evaluate(Some(at(300)), now), CatalogFreshness::Stale);
        assert_eq!(
            policy.evaluate(Some(at(1800)), now),
            CatalogFreshness::Stale
        );
        assert_eq!(
            policy.evaluate(Some(at(1801)), now),
            CatalogFreshness::Expired
        );
    }

    #[test]
    fn a_future_acquisition_timestamp_is_treated_as_fresh() {
        let policy = CatalogFreshnessPolicy::production();
        let now = Utc::now();
        assert_eq!(
            policy.evaluate(Some(now + TimeDelta::seconds(30)), now),
            CatalogFreshness::Fresh
        );
    }

    #[test]
    fn the_fingerprint_covers_content_only() {
        let first = snapshot("tool");
        let second = snapshot("tool");
        let other = snapshot("other");

        assert_eq!(
            content_fingerprint(&first),
            content_fingerprint(&second),
            "identical content keeps the same fingerprint"
        );
        assert_ne!(content_fingerprint(&first), content_fingerprint(&other));
    }
}
