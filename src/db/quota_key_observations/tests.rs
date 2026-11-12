use super::{TREND_SERIES_MAX, TrendMetric, TrendRange, TrendSnapshot, project_trend};
use crate::db::types::{QuotaObservationCreate, QuotaObservationProvider, QuotaObservationSource};
use crate::worker_admin_types::{
    CanonicalQuotaWindow, CanonicalQuotaWindowKind, SubscriptionWindowAvailability,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

fn observed_at(hour: u32) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&format!("2026-10-10T{hour:02}:00:00Z"))
        .expect("fixed test timestamp parses")
        .with_timezone(&Utc)
}

fn known_window(source_id: &str, remaining_percent: f64) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Rolling,
        source_model: Some("test-model".to_string()),
        unit: Some("tokens".to_string()),
        duration_seconds: Some(300),
        is_default_scope: true,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: Some(remaining_percent),
        current: Some(100.0 - remaining_percent),
        limit: Some(100.0),
        remaining: None,
        reset_at: None,
    }
}

fn unknown_window(source_id: &str) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Rolling,
        source_model: None,
        unit: None,
        duration_seconds: None,
        is_default_scope: true,
        availability: SubscriptionWindowAvailability::Unknown,
        remaining_percent: None,
        current: None,
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

fn balance_window(source_id: &str, remaining: f64) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Balance,
        source_model: None,
        unit: Some("credits".to_string()),
        duration_seconds: None,
        is_default_scope: false,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: None,
        current: None,
        limit: None,
        remaining: Some(remaining),
        reset_at: None,
    }
}

fn success_create(
    endpoint_id: Uuid,
    key_id: Uuid,
    observed_at: DateTime<Utc>,
) -> QuotaObservationCreate {
    QuotaObservationCreate::new(
        endpoint_id,
        key_id,
        QuotaObservationProvider::Minimax,
        "test-key".to_string(),
        QuotaObservationSource::Request,
        observed_at,
        true,
        None,
        None,
        Some("team".to_string()),
        Some(false),
        vec![
            known_window("usage", 75.0),
            known_window("empty-slot", 0.0),
            unknown_window("unsupported-slot"),
            balance_window("credits", 50.0),
        ],
    )
    .expect("valid success observation builds")
}

#[tokio::test]
async fn constructor_rejects_invalid_observations() {
    let endpoint_id = Uuid::new_v4();
    let key_id = Uuid::new_v4();
    let valid_window = known_window("usage", 75.0);
    let build = |windows: Vec<CanonicalQuotaWindow>| {
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "test-key".to_string(),
            QuotaObservationSource::Request,
            observed_at(10),
            true,
            None,
            None,
            None,
            None,
            windows,
        )
    };

    let mut non_finite = valid_window.clone();
    non_finite.remaining_percent = Some(f64::INFINITY);
    let mut over_percent = valid_window.clone();
    over_percent.remaining_percent = Some(100.5);
    let mut synthetic_balance = balance_window("credits", 50.0);
    synthetic_balance.remaining_percent = Some(10.0);
    let mut synthetic_unknown = unknown_window("slot");
    synthetic_unknown.remaining_percent = Some(0.0);
    let mut empty_source = valid_window.clone();
    empty_source.source_id = String::new();
    let mut long_source = valid_window.clone();
    long_source.source_id = "s".repeat(129);
    let mut zero_duration = valid_window.clone();
    zero_duration.duration_seconds = Some(0);

    for (label, windows) in [
        ("non-finite percent", vec![non_finite]),
        ("percent over 100", vec![over_percent]),
        ("balance with percent", vec![synthetic_balance]),
        ("unknown with percent", vec![synthetic_unknown]),
        ("empty source id", vec![empty_source]),
        ("129-char source id", vec![long_source]),
        ("zero duration", vec![zero_duration]),
        (
            "65 windows",
            (0..65)
                .map(|index| known_window(&format!("slot-{index}"), 50.0))
                .collect(),
        ),
    ] {
        assert!(
            build(windows).is_err(),
            "{label} must be rejected without a database"
        );
    }

    let mut failed_with_windows = success_create(endpoint_id, key_id, observed_at(10));
    failed_with_windows.ok = false;
    assert!(
        failed_with_windows.validate().is_err(),
        "a failed observation carrying windows must be rejected"
    );
    assert!(
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "k".repeat(201),
            QuotaObservationSource::Request,
            observed_at(10),
            true,
            None,
            None,
            None,
            None,
            vec![valid_window.clone()],
        )
        .is_err(),
        "a 201-char key label must be rejected"
    );
    assert!(
        QuotaObservationCreate::new(
            endpoint_id,
            key_id,
            QuotaObservationProvider::Minimax,
            "test-key".to_string(),
            QuotaObservationSource::Request,
            observed_at(10),
            false,
            None,
            Some("BAD CODE!".to_string()),
            None,
            None,
            vec![],
        )
        .is_err(),
        "a non-stable error code must be rejected"
    );
}

fn trend_percent_window(
    source_id: &str,
    kind: CanonicalQuotaWindowKind,
    remaining_percent: Option<f64>,
    availability: SubscriptionWindowAvailability,
) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind,
        source_model: None,
        unit: Some("tokens".to_string()),
        duration_seconds: Some(300),
        is_default_scope: true,
        availability,
        remaining_percent,
        current: None,
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

fn trend_balance_window(
    source_id: &str,
    unit: Option<&str>,
    current: Option<f64>,
    availability: SubscriptionWindowAvailability,
) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Balance,
        source_model: None,
        unit: unit.map(str::to_string),
        duration_seconds: None,
        is_default_scope: false,
        availability,
        remaining_percent: None,
        current,
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

fn trend_unlimited_window(source_id: &str) -> CanonicalQuotaWindow {
    CanonicalQuotaWindow {
        source_id: source_id.to_string(),
        kind: CanonicalQuotaWindowKind::Unlimited,
        source_model: None,
        unit: Some("credits".to_string()),
        duration_seconds: None,
        is_default_scope: false,
        availability: SubscriptionWindowAvailability::Known,
        remaining_percent: None,
        current: Some(1.0),
        limit: None,
        remaining: None,
        reset_at: None,
    }
}

fn trend_snapshot(
    key_id: Uuid,
    key_label: &str,
    bucket_index: usize,
    observed_at: DateTime<Utc>,
    ok: bool,
    windows: Vec<CanonicalQuotaWindow>,
) -> TrendSnapshot {
    TrendSnapshot {
        key_id,
        key_label: key_label.to_string(),
        observed_at,
        ok,
        bucket_index,
        windows,
    }
}

#[test]
fn trend_range_maps_to_fixed_buckets() {
    assert_eq!(TrendRange::parse(None), Some(TrendRange::Last24h));
    assert_eq!(TrendRange::parse(Some("24h")), Some(TrendRange::Last24h));
    assert_eq!(TrendRange::parse(Some("7d")), Some(TrendRange::Last7d));
    assert_eq!(TrendRange::parse(Some("30d")), Some(TrendRange::Last30d));
    for unknown in ["1h", "24H", "", " 24h", "week"] {
        assert_eq!(TrendRange::parse(Some(unknown)), None, "range={unknown}");
    }
    for (range, seconds, count) in [
        (TrendRange::Last24h, 900, 96),
        (TrendRange::Last7d, 7_200, 84),
        (TrendRange::Last30d, 3_600, 720),
    ] {
        assert_eq!(range.bucket_seconds(), seconds);
        assert_eq!(range.bucket_count(), count);
        assert_eq!(
            range.duration_seconds() / range.bucket_seconds(),
            count as i64
        );
    }
}

#[test]
fn trend_projection_separates_identities_and_blanks_gaps() {
    let key = Uuid::new_v4();
    let snapshots = vec![trend_snapshot(
        key,
        "first",
        0,
        observed_at(10),
        true,
        vec![
            trend_percent_window(
                "usage",
                CanonicalQuotaWindowKind::Rolling,
                Some(75.0),
                SubscriptionWindowAvailability::Known,
            ),
            trend_balance_window(
                "credits",
                Some("credits"),
                Some(50.0),
                SubscriptionWindowAvailability::Known,
            ),
            trend_balance_window(
                "fiat",
                Some("usd"),
                Some(-5.0),
                SubscriptionWindowAvailability::Known,
            ),
            trend_balance_window(
                "unitless",
                None,
                Some(1.0),
                SubscriptionWindowAvailability::Known,
            ),
            trend_unlimited_window("unlimited"),
        ],
    )];
    let projection = project_trend(3, snapshots, None);
    assert_eq!(projection.sampled_observations, 1);
    assert_eq!(projection.failed_observations, 0);
    assert!(!projection.series_truncated);
    // Rolling percent + two distinct balance units; an unknown-kind limited
    // window, a unitless balance, and `unlimited` are not plotted.
    assert_eq!(projection.series.len(), 3);

    let usage = projection
        .series
        .iter()
        .find(|series| series.source_id == "usage")
        .expect("rolling series present");
    assert_eq!(usage.metric, TrendMetric::RemainingPercent);
    assert_eq!(usage.unit, "percent");
    assert_eq!(usage.points.len(), 3);
    assert_eq!(usage.points[0].value, Some(75.0));
    assert!(usage.points[1].value.is_none());
    assert!(usage.points[1].observed_at.is_none());

    let credits = projection
        .series
        .iter()
        .find(|series| series.source_id == "credits")
        .expect("credits series present");
    assert_eq!(credits.metric, TrendMetric::Amount);
    assert_eq!(credits.unit, "credits");
    assert_eq!(credits.points[0].value, Some(50.0));

    let fiat = projection
        .series
        .iter()
        .find(|series| series.source_id == "fiat")
        .expect("fiat series present");
    assert_eq!(fiat.unit, "usd");
    assert_eq!(
        fiat.points[0].value,
        Some(-5.0),
        "negative balances are preserved"
    );
}

#[test]
fn trend_projection_blanks_failed_and_absent_windows() {
    let key = Uuid::new_v4();
    let snapshots = vec![
        trend_snapshot(
            key,
            "oldest",
            0,
            observed_at(9),
            true,
            vec![trend_percent_window(
                "usage",
                CanonicalQuotaWindowKind::Rolling,
                Some(80.0),
                SubscriptionWindowAvailability::Known,
            )],
        ),
        // The latest selected row in bucket 1 is a failure carrying no windows.
        trend_snapshot(key, "middle", 1, observed_at(10), false, vec![]),
        // A later success selects a different source window only.
        trend_snapshot(
            key,
            "latest",
            2,
            observed_at(11),
            true,
            vec![trend_percent_window(
                "other",
                CanonicalQuotaWindowKind::Rolling,
                Some(10.0),
                SubscriptionWindowAvailability::Known,
            )],
        ),
    ];
    let projection = project_trend(4, snapshots, None);
    assert_eq!(projection.sampled_observations, 3);
    assert_eq!(projection.failed_observations, 1);

    let usage = projection
        .series
        .iter()
        .find(|series| series.source_id == "usage")
        .expect("usage series survives a bucket failure");
    assert_eq!(usage.key_label, "latest", "latest selected snapshot label");
    assert_eq!(usage.points[0].value, Some(80.0));
    // A failed selected row blanks the bucket and keeps its observed_at.
    assert!(usage.points[1].value.is_none());
    assert_eq!(usage.points[1].observed_at, Some(observed_at(10)));
    // The chosen success lacks the usage window, so the value stays null
    // without falling back to an older success.
    assert!(usage.points[2].value.is_none());
    assert_eq!(usage.points[2].observed_at, Some(observed_at(11)));
    // A bucket with no selected row has no value and no observed_at.
    assert!(usage.points[3].value.is_none());
    assert!(usage.points[3].observed_at.is_none());

    let other = projection
        .series
        .iter()
        .find(|series| series.source_id == "other")
        .expect("second series present");
    assert!(other.points[0].value.is_none());
    assert_eq!(other.points[2].value, Some(10.0));
}

#[test]
fn trend_projection_establishes_unknown_series_as_null() {
    let key = Uuid::new_v4();
    let snapshots = vec![trend_snapshot(
        key,
        "key",
        0,
        observed_at(10),
        true,
        vec![
            trend_percent_window(
                "usage",
                CanonicalQuotaWindowKind::Weekly,
                None,
                SubscriptionWindowAvailability::Unknown,
            ),
            trend_balance_window(
                "credits",
                Some("credits"),
                Some(50.0),
                SubscriptionWindowAvailability::Unknown,
            ),
        ],
    )];
    let projection = project_trend(2, snapshots, None);
    assert_eq!(
        projection.series.len(),
        2,
        "an unknown-availability window still establishes its identity"
    );
    for series in &projection.series {
        assert!(
            series.points[0].value.is_none(),
            "{} must stay null while unavailable",
            series.source_id
        );
        assert_eq!(series.points[0].observed_at, Some(observed_at(10)));
    }
}

#[test]
fn trend_projection_caps_series_and_reports_truncation() {
    let key = Uuid::new_v4();
    let windows = (0..TREND_SERIES_MAX + 1)
        .map(|index| {
            trend_balance_window(
                &format!("wallet-{index:03}"),
                Some("credits"),
                Some(1.0),
                SubscriptionWindowAvailability::Known,
            )
        })
        .collect();
    let projection = project_trend(
        1,
        vec![trend_snapshot(
            key,
            "key",
            0,
            observed_at(10),
            true,
            windows,
        )],
        None,
    );
    assert_eq!(projection.series.len(), TREND_SERIES_MAX);
    assert!(projection.series_truncated);
}

#[test]
fn trend_projection_filters_kinds_before_the_series_cap() {
    let key = Uuid::new_v4();
    let mut windows: Vec<CanonicalQuotaWindow> = (0..TREND_SERIES_MAX)
        .map(|index| {
            trend_percent_window(
                &format!("a{index:03}"),
                CanonicalQuotaWindowKind::Rolling,
                Some(1.0),
                SubscriptionWindowAvailability::Known,
            )
        })
        .collect();
    windows.push(trend_percent_window(
        "z-weekly",
        CanonicalQuotaWindowKind::Weekly,
        Some(2.0),
        SubscriptionWindowAvailability::Known,
    ));
    let snapshot = trend_snapshot(key, "key", 0, observed_at(10), true, windows);

    let unfiltered = project_trend(1, vec![snapshot.clone()], None);
    assert!(unfiltered.series_truncated);
    assert!(
        unfiltered
            .series
            .iter()
            .all(|series| series.kind != CanonicalQuotaWindowKind::Weekly),
        "the trailing weekly identity is dropped by the plain cap"
    );

    let allowed = [CanonicalQuotaWindowKind::Weekly];
    let filtered = project_trend(1, vec![snapshot], Some(&allowed[..]));
    assert_eq!(filtered.series.len(), 1);
    assert_eq!(filtered.series[0].kind, CanonicalQuotaWindowKind::Weekly);
    assert!(!filtered.series_truncated);
}

#[test]
fn trend_projection_ignores_snapshots_outside_the_grid() {
    let key = Uuid::new_v4();
    let projection = project_trend(
        2,
        vec![trend_snapshot(
            key,
            "key",
            5,
            observed_at(10),
            true,
            vec![trend_percent_window(
                "ghost",
                CanonicalQuotaWindowKind::Rolling,
                Some(1.0),
                SubscriptionWindowAvailability::Known,
            )],
        )],
        None,
    );
    assert_eq!(projection.sampled_observations, 0);
    assert!(projection.series.is_empty());
}
