//! Bounded per-key quota observation trend projection (issue #911 P1).
//!
//! The database does the bounding: the key list is capped before the bucket
//! query runs, and every (selected key, relative bucket) resolves to exactly
//! one latest observation. This module owns the pure projection that turns
//! those snapshots into display series: one series per
//! `(key, source_id, kind, unit, metric)` identity, no merging across units or
//! meanings, and a `null` point for a missing, failed, or unavailable value
//! instead of a carried-forward or fabricated number.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::worker_admin_types::{
    CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
};

/// Display cap for selected keys; the key query asks for one more to report
/// truncation.
pub const TREND_KEY_LIMIT: usize = 24;
/// Display cap for projected series; the projection keeps one more candidate to
/// report truncation.
pub const TREND_SERIES_MAX: usize = 96;

/// Fixed server range for the trend. The bucket grid is relative to the
/// request's `from`, never an epoch floor, so `from + index * step` is the
/// bucket timestamp and the last bucket stays strictly before `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendRange {
    Last24h,
    Last7d,
    Last30d,
}

impl TrendRange {
    pub const DEFAULT: Self = Self::Last24h;

    /// Parses the wire `range`; `None` (absent) defaults to 24h and any other
    /// literal is rejected so the handler can answer 400.
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        match raw.unwrap_or("24h") {
            "24h" => Some(Self::Last24h),
            "7d" => Some(Self::Last7d),
            "30d" => Some(Self::Last30d),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Last24h => "24h",
            Self::Last7d => "7d",
            Self::Last30d => "30d",
        }
    }

    pub const fn bucket_seconds(self) -> i64 {
        match self {
            Self::Last24h => 900,
            Self::Last7d => 7_200,
            Self::Last30d => 28_800,
        }
    }

    pub const fn duration_seconds(self) -> i64 {
        match self {
            Self::Last24h => 86_400,
            Self::Last7d => 604_800,
            Self::Last30d => 2_592_000,
        }
    }

    pub const fn bucket_count(self) -> usize {
        match self {
            Self::Last24h => 96,
            Self::Last7d => 84,
            Self::Last30d => 90,
        }
    }

    pub fn duration(self) -> Duration {
        Duration::seconds(self.duration_seconds())
    }
}

/// The numeric meaning of a projected series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrendMetric {
    Amount,
    RemainingPercent,
}

/// One selected snapshot: the latest observation of a key inside one bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendSnapshot {
    pub key_id: Uuid,
    pub key_label: String,
    pub observed_at: DateTime<Utc>,
    pub ok: bool,
    pub bucket_index: usize,
    pub windows: Vec<CanonicalQuotaWindow>,
}

/// One projected point value; the caller attaches the bucket timestamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendPointValue {
    pub observed_at: Option<DateTime<Utc>>,
    pub value: Option<f64>,
}

/// One projected series with a fixed-length, ascending bucket point run.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendSeries {
    pub key_id: Uuid,
    pub key_label: String,
    pub source_id: String,
    pub kind: CanonicalQuotaWindowKind,
    pub unit: String,
    pub metric: TrendMetric,
    pub points: Vec<TrendPointValue>,
}

/// The bounded projection plus the honest truncation and sampling counters.
#[derive(Debug, Clone, PartialEq)]
pub struct TrendProjection {
    pub series: Vec<TrendSeries>,
    pub series_truncated: bool,
    pub sampled_observations: usize,
    pub failed_observations: usize,
}

#[derive(Debug)]
struct TrendKeyRow {
    key_id: Uuid,
}

#[derive(Debug)]
struct TrendBucketRow {
    key_id: Uuid,
    key_label: String,
    observed_at: DateTime<Utc>,
    ok: bool,
    windows: serde_json::Value,
    bucket_index: i64,
}

/// Selects up to one key past the display cap, ordered latest observed first.
pub async fn load_trend_keys(
    pool: &PgPool,
    endpoint_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    key_id: Option<Uuid>,
) -> Result<Vec<Uuid>> {
    let rows = sqlx::query_file_as!(
        TrendKeyRow,
        "src/sql/quota_key_observations/trend_keys.sql",
        endpoint_id,
        from,
        to,
        key_id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|row| row.key_id).collect())
}

/// Loads the latest observation per (key, bucket) for the selected keys.
pub async fn load_trend_buckets(
    pool: &PgPool,
    endpoint_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    bucket_seconds: i64,
    key_ids: &[Uuid],
) -> Result<Vec<TrendSnapshot>> {
    let rows = sqlx::query_file_as!(
        TrendBucketRow,
        "src/sql/quota_key_observations/trend_buckets.sql",
        endpoint_id,
        from,
        to,
        key_ids,
        bucket_seconds,
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let windows: Vec<CanonicalQuotaWindow> = serde_json::from_value(row.windows).context(
                "database contains quota observation windows outside the canonical shape",
            )?;
            Ok(TrendSnapshot {
                key_id: row.key_id,
                key_label: row.key_label,
                observed_at: row.observed_at,
                ok: row.ok,
                bucket_index: row.bucket_index.max(0) as usize,
                windows,
            })
        })
        .collect()
}

/// Projects selected snapshots into bounded, identity-separated series.
///
/// The projection is pure: it never carries a value forward, never fabricates
/// a number from a ratio, and never merges two units or meanings.
pub fn project_trend(bucket_count: usize, snapshots: Vec<TrendSnapshot>) -> TrendProjection {
    // Defensive: a snapshot outside the fixed grid cannot be plotted and must
    // not seed a phantom series.
    let snapshots: Vec<TrendSnapshot> = snapshots
        .into_iter()
        .filter(|snapshot| snapshot.bucket_index < bucket_count)
        .collect();
    let sampled_observations = snapshots.len();
    let failed_observations = snapshots.iter().filter(|snapshot| !snapshot.ok).count();

    let mut latest_labels: BTreeMap<Uuid, (DateTime<Utc>, String)> = BTreeMap::new();
    for snapshot in &snapshots {
        let replace = latest_labels
            .get(&snapshot.key_id)
            .map(|(observed_at, _)| snapshot.observed_at > *observed_at)
            .unwrap_or(true);
        if replace {
            latest_labels.insert(
                snapshot.key_id,
                (snapshot.observed_at, snapshot.key_label.clone()),
            );
        }
    }

    let mut identities: BTreeSet<SeriesIdentity> = BTreeSet::new();
    for snapshot in &snapshots {
        for window in &snapshot.windows {
            if let Some(identity) = series_identity(snapshot.key_id, window) {
                identities.insert(identity);
            }
        }
    }

    let mut selected: BTreeMap<(Uuid, usize), &TrendSnapshot> = BTreeMap::new();
    for snapshot in &snapshots {
        selected.insert((snapshot.key_id, snapshot.bucket_index), snapshot);
    }

    let mut series = Vec::with_capacity(identities.len());
    for identity in identities {
        let key_label = latest_labels
            .get(&identity.key_id)
            .map(|(_, label)| label.clone())
            .unwrap_or_default();
        let mut points = Vec::with_capacity(bucket_count);
        for bucket_index in 0..bucket_count {
            points.push(match selected.get(&(identity.key_id, bucket_index)) {
                Some(snapshot) => TrendPointValue {
                    observed_at: Some(snapshot.observed_at),
                    value: if snapshot.ok {
                        series_value(&identity, snapshot)
                    } else {
                        None
                    },
                },
                None => TrendPointValue {
                    observed_at: None,
                    value: None,
                },
            });
        }
        series.push(TrendSeries {
            key_id: identity.key_id,
            key_label,
            source_id: identity.source_id,
            kind: identity.kind,
            unit: identity.unit,
            metric: identity.metric,
            points,
        });
    }

    let series_truncated = series.len() > TREND_SERIES_MAX;
    series.truncate(TREND_SERIES_MAX);
    TrendProjection {
        series,
        series_truncated,
        sampled_observations,
        failed_observations,
    }
}

/// The stable series identity: no unit or meaning is ever merged.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SeriesIdentity {
    key_id: Uuid,
    source_id: String,
    kind: CanonicalQuotaWindowKind,
    unit: String,
    metric: TrendMetric,
}

impl Ord for SeriesIdentity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key_id
            .cmp(&other.key_id)
            .then_with(|| self.source_id.cmp(&other.source_id))
            .then_with(|| kind_rank(self.kind).cmp(&kind_rank(other.kind)))
            .then_with(|| self.unit.cmp(&other.unit))
            .then_with(|| self.metric.cmp(&other.metric))
    }
}

impl PartialOrd for SeriesIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

const fn kind_rank(kind: CanonicalQuotaWindowKind) -> u8 {
    match kind {
        CanonicalQuotaWindowKind::FiveHour => 0,
        CanonicalQuotaWindowKind::Rolling => 1,
        CanonicalQuotaWindowKind::Weekly => 2,
        CanonicalQuotaWindowKind::Monthly => 3,
        CanonicalQuotaWindowKind::Balance => 4,
        CanonicalQuotaWindowKind::Unlimited => 5,
        CanonicalQuotaWindowKind::Custom => 6,
    }
}

/// Which window kinds can seed a plot and under which metric. Only a balance
/// window with a native non-empty unit and a time-window kind are plottable;
/// `unlimited` carries no numeric meaning.
fn series_identity(key_id: Uuid, window: &CanonicalQuotaWindow) -> Option<SeriesIdentity> {
    if window.source_id.is_empty() {
        return None;
    }
    match window.kind {
        CanonicalQuotaWindowKind::Balance => {
            let unit = window.unit.as_deref().filter(|unit| !unit.is_empty())?;
            Some(SeriesIdentity {
                key_id,
                source_id: window.source_id.clone(),
                kind: window.kind,
                unit: unit.to_string(),
                metric: TrendMetric::Amount,
            })
        }
        CanonicalQuotaWindowKind::FiveHour
        | CanonicalQuotaWindowKind::Rolling
        | CanonicalQuotaWindowKind::Weekly
        | CanonicalQuotaWindowKind::Monthly
        | CanonicalQuotaWindowKind::Custom => Some(SeriesIdentity {
            key_id,
            source_id: window.source_id.clone(),
            kind: window.kind,
            unit: "percent".to_string(),
            metric: TrendMetric::RemainingPercent,
        }),
        CanonicalQuotaWindowKind::Unlimited => None,
    }
}

/// The value one snapshot contributes to a series, or `None` when the window
/// is absent, unknown, or outside the metric's usable domain.
fn series_value(identity: &SeriesIdentity, snapshot: &TrendSnapshot) -> Option<f64> {
    let window = snapshot
        .windows
        .iter()
        .find(|window| window_matches(identity, window))?;
    if window.availability != SubscriptionWindowAvailability::Known {
        return None;
    }
    match identity.metric {
        // Balances keep zero and negative amounts; only a non-finite value is
        // absent.
        TrendMetric::Amount => window.current.filter(|value| value.is_finite()),
        // A real reported/derived share only; anything outside `0..=100` is
        // treated as not usable rather than clamped.
        TrendMetric::RemainingPercent => window
            .remaining_percent
            .filter(|value| value.is_finite() && (0.0..=100.0).contains(value)),
    }
}

fn window_matches(identity: &SeriesIdentity, window: &CanonicalQuotaWindow) -> bool {
    if window.source_id != identity.source_id || window.kind != identity.kind {
        return false;
    }
    match identity.metric {
        TrendMetric::Amount => window.unit.as_deref() == Some(identity.unit.as_str()),
        TrendMetric::RemainingPercent => true,
    }
}
