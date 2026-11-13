use super::{compute, cycle};
use crate::db::quota_inference::{
    QuotaInferenceAggregate, QuotaInferenceSnapshot, SnapshotEvidence,
};
use chrono::{Duration, TimeZone, Utc};
use serde_json::json;

fn evidence(values: &[(i64, f64)]) -> SnapshotEvidence {
    let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    SnapshotEvidence { snapshots: values.iter().map(|(minutes,remaining)| QuotaInferenceSnapshot { observed_at: base + Duration::minutes(*minutes), plan_type: Some("plus".into()), limit_reached: Some(false), windows: json!([{ "window_seconds": 604800, "availability": "known", "remaining_percent": remaining, "reset_at": "2026-01-08T00:00:00Z" }]) }).collect(), overflow:false, observation_provenance:"success_only", account_history_assumed_stable:true, refresh_state:None }
}
fn row(index: i64, model: &str, fresh: i64, read: i64, output: i64) -> QuotaInferenceAggregate {
    QuotaInferenceAggregate {
        interval_index: index,
        bucket: "model".into(),
        model: Some(model.into()),
        request_count: 1,
        invalid_duration_count: 0,
        error_count: 0,
        missing_usage_count: 0,
        fresh_tokens: fresh,
        cache_read_tokens: read,
        cache_write_tokens: 0,
        output_tokens: output,
        distinct_models: 1,
    }
}
#[test]
fn blocks_split_gaps_and_corrections() {
    let data = evidence(&[(0, 90.0), (5, 85.0), (25, 80.0), (30, 75.0)]);
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    assert_eq!(cycle.blocks.len(), 2);
    assert_eq!(cycle.blocks[1].start, data.snapshots[2].observed_at);
    let correction = evidence(&[(0, 90.0), (5, 85.0), (10, 86.0), (15, 81.0)]);
    let cycle = cycle::build(&correction, correction.snapshots[0].observed_at).unwrap();
    assert_eq!(cycle.start, correction.snapshots[2].observed_at);
    assert_eq!(cycle.blocks.len(), 1);
    let mut unknown = evidence(&[(0, 90.0), (5, 85.0), (10, 84.0), (15, 80.0), (20, 75.0)]);
    unknown.snapshots[2].windows = json!([]);
    let cycle = cycle::build(&unknown, unknown.snapshots[0].observed_at).unwrap();
    assert_eq!(cycle.blocks.len(), 2);
    assert_eq!(cycle.blocks[1].start, unknown.snapshots[3].observed_at);
}
#[test]
fn arbitrary_model_without_signal_remains_visible() {
    let data = evidence(&[(0, 90.0), (5, 90.0)]);
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let rows = vec![row(1, "gpt-6-luna", 30, 970, 100)];
    let response = compute::compute(
        &data,
        Some(&cycle),
        [&rows, &rows, &rows],
        data.snapshots[1].observed_at,
    );
    assert_eq!(response.models[0].model, "gpt-6-luna");
    assert_eq!(
        response.models[0].actual.reason.as_deref(),
        Some("insufficient_signal")
    );
    assert_eq!(
        response.models[0].observed_near_97.reason.as_deref(),
        Some("at97_no_sample")
    );
}
#[test]
fn observed_near97_requires_observed_composition() {
    let data = evidence(
        &(0..=8)
            .map(|i| (i * 5, 95.0 - i as f64 * 5.0))
            .collect::<Vec<_>>(),
    );
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let mut rows = vec![row(1, "gpt-6-luna", 240, 7760, 800)];
    rows.extend((2..=9).map(|i| row(i, "gpt-6-luna", 30, 970, 100)));
    let now = data.snapshots.last().unwrap().observed_at;
    let response = compute::compute(&data, Some(&cycle), [&rows, &rows, &rows], now);
    assert!(response.models[0].actual.capacity_millions.is_some());
    assert!(
        response.models[0]
            .observed_near_97
            .capacity_millions
            .is_some()
    );
    rows[3].cache_read_tokens = 500;
    rows[3].fresh_tokens = 500;
    let response = compute::compute(&data, Some(&cycle), [&rows, &rows, &rows], now);
    assert!(
        response.models[0]
            .observed_near_97
            .capacity_millions
            .is_none()
    );
}
