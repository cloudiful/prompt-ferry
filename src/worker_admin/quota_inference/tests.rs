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
fn aggregate(
    index: i64,
    bucket: &str,
    model: Option<&str>,
    fresh: i64,
    read: i64,
    output: i64,
) -> QuotaInferenceAggregate {
    QuotaInferenceAggregate {
        interval_index: index,
        bucket: bucket.into(),
        model: model.map(Into::into),
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
fn row(index: i64, model: &str, fresh: i64, read: i64, output: i64) -> QuotaInferenceAggregate {
    aggregate(index, "model", Some(model), fresh, read, output)
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
#[test]
fn zero_token_alias_rows_do_not_truncate_models() {
    let data = evidence(&[(0, 90.0), (5, 90.0)]);
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let mut aliases = aggregate(1, "unranked", None, 0, 0, 0);
    aliases.error_count = 104;
    let mut legacy_overflow = aggregate(1, "overflow", None, 0, 0, 0);
    legacy_overflow.error_count = 7;
    let rows = vec![
        row(1, "gpt-5.1-codex-max", 30, 970, 100),
        aggregate(1, "model", Some("gpt-5-zero"), 0, 0, 0),
        aliases,
        legacy_overflow,
    ];
    let response = compute::compute(
        &data,
        Some(&cycle),
        [&rows, &rows, &rows],
        data.snapshots[1].observed_at,
    );
    assert!(!response.models_truncated);
    assert_eq!(response.overflow_tokens, 0);
    assert_eq!(response.omitted_rows, 111);
    assert_eq!(response.models.len(), 1);
    assert_eq!(response.models[0].model, "gpt-5.1-codex-max");
}
#[test]
fn positive_overflow_still_blocks_models() {
    let data = evidence(&[(0, 90.0), (5, 85.0), (10, 80.0)]);
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let mut overflow = aggregate(1, "overflow", None, 600, 0, 0);
    overflow.distinct_models = 2;
    let rows = vec![row(1, "gpt-6-luna", 240, 7760, 800), overflow];
    let response = compute::compute(
        &data,
        Some(&cycle),
        [&rows, &rows, &rows],
        data.snapshots[2].observed_at,
    );
    assert!(response.models_truncated);
    assert_eq!(response.overflow_tokens, 600);
    assert_eq!(
        response.models[0].actual.reason.as_deref(),
        Some("model_truncated")
    );
}
#[test]
fn ranked_identity_count_above_limit_truncates_models() {
    let data = evidence(&[(0, 90.0), (5, 85.0), (10, 80.0)]);
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let mut first = row(1, "gpt-6-luna", 240, 7760, 800);
    first.distinct_models = 33;
    let rows = vec![first, row(2, "gpt-6-luna", 240, 7760, 800)];
    let response = compute::compute(
        &data,
        Some(&cycle),
        [&rows, &rows, &rows],
        data.snapshots[2].observed_at,
    );
    assert!(response.models_truncated);
    assert_eq!(
        response.models[0].actual.reason.as_deref(),
        Some("model_truncated")
    );
}
#[test]
fn positive_unranked_tokens_remain_in_account_totals() {
    let data = evidence(
        &(0..=6)
            .map(|i| (i * 5, 95.0 - i as f64 * 5.0))
            .collect::<Vec<_>>(),
    );
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let now = data.snapshots.last().unwrap().observed_at;
    let mut rows: Vec<_> = (1..=6).map(|i| row(i, "gpt-6-luna", 900, 0, 100)).collect();
    let base = compute::compute(&data, Some(&cycle), [&rows, &rows, &rows], now);
    assert!(base.account.capacity_millions.is_some());
    rows.extend((2..=6).map(|i| aggregate(i, "unranked", None, 9000, 0, 0)));
    let retained = compute::compute(&data, Some(&cycle), [&rows, &rows, &rows], now);
    assert!(retained.account.capacity_millions.is_some());
    assert!(retained.account.capacity_millions.unwrap() > base.account.capacity_millions.unwrap());
    assert!(!retained.models_truncated);
    assert_eq!(retained.models.len(), 1);
}
#[test]
fn near97_preserves_fit_reason_and_reports_block_requirement() {
    let data = evidence(
        &(0..=12)
            .map(|i| (i * 5, 95.0 - i as f64 * 5.0))
            .collect::<Vec<_>>(),
    );
    let cycle = cycle::build(&data, data.snapshots[0].observed_at).unwrap();
    let mut rows: Vec<_> = (1..=4)
        .map(|i| row(i, "gpt-6-luna", 30, 970, 100))
        .collect();
    rows.extend((5..=12).map(|i| row(i, "gpt-6-luna", 1000, 0, 100)));
    let now = data.snapshots.last().unwrap().observed_at;
    let response = compute::compute(&data, Some(&cycle), [&rows, &rows, &rows], now);
    let model = &response.models[0];
    assert!(model.actual.capacity_millions.is_some());
    assert_eq!(model.actual.blocks, Some(12));
    assert_eq!(model.actual.required_blocks, Some(8));
    assert_eq!(
        model.observed_near_97.reason.as_deref(),
        Some("insufficient_signal")
    );
    assert_eq!(model.observed_near_97.blocks, Some(4));
    assert_eq!(model.observed_near_97.required_blocks, Some(8));
}
