use super::{
    cycle::Cycle,
    estimate::{self, Observation, Unavailable},
    model::{self, ModelBlock, ModelUnavailable},
    response::{Estimate, InferenceResponse, ModelRow},
};
use crate::db::quota_inference::{QuotaInferenceAggregate, SnapshotEvidence};
use chrono::{DateTime, Duration, Utc};
use std::collections::{BTreeMap, BTreeSet};

type Key = (String, Option<String>);
#[derive(Clone, Copy, Default)]
struct Meters {
    fresh: i64,
    read: i64,
    write: i64,
    output: i64,
}
impl Meters {
    fn mixed(self) -> i64 {
        self.fresh
            .saturating_add(self.read)
            .saturating_add(self.write)
            .saturating_add(self.output)
    }
    fn hit(self) -> Option<f64> {
        let input = self
            .fresh
            .saturating_add(self.read)
            .saturating_add(self.write);
        (input > 0).then(|| self.read as f64 / input as f64)
    }
    fn output_share(self) -> Option<f64> {
        let mixed = self.mixed();
        (mixed > 0).then(|| self.output as f64 / mixed as f64)
    }
}
fn meters(row: &QuotaInferenceAggregate) -> Meters {
    Meters {
        fresh: row.fresh_tokens,
        read: row.cache_read_tokens,
        write: row.cache_write_tokens,
        output: row.output_tokens,
    }
}
fn interval(rows: &[QuotaInferenceAggregate], index: usize) -> BTreeMap<Key, Meters> {
    rows.iter()
        .filter(|r| r.interval_index == index as i64 + 1)
        .map(|r| ((r.bucket.clone(), r.model.clone()), meters(r)))
        .collect()
}
fn reason(e: ModelUnavailable) -> &'static str {
    match e {
        ModelUnavailable::InsufficientSignal => "insufficient_signal",
        ModelUnavailable::Complexity => "complexity",
        ModelUnavailable::ModelMixFlat => "model_mix_flat",
        ModelUnavailable::IllConditioned => "ill_conditioned",
        ModelUnavailable::PoorFit => "poor_fit",
        ModelUnavailable::Unstable => "unstable",
    }
}
fn account_reason(e: Unavailable) -> &'static str {
    match e {
        Unavailable::InsufficientSignal => "insufficient_signal",
        Unavailable::LowConfidence => "low_confidence",
        Unavailable::AttributionUnstable => "attribution_unstable",
    }
}
fn observations(cycle: &Cycle, rows: &[QuotaInferenceAggregate]) -> Vec<Observation> {
    cycle
        .blocks
        .iter()
        .enumerate()
        .map(|(i, b)| Observation {
            quota_pp: b.quota_pp,
            mixed_tokens: interval(rows, i + 1)
                .values()
                .map(|m| m.mixed() as f64)
                .sum(),
        })
        .collect()
}
fn model_blocks(
    cycle: &Cycle,
    rows: &[QuotaInferenceAggregate],
    keys: &[Key],
    selected: &[usize],
) -> Vec<ModelBlock> {
    selected
        .iter()
        .map(|&i| {
            let map = interval(rows, i + 1);
            ModelBlock {
                quota_pp: cycle.blocks[i].quota_pp,
                mixed_millions: keys
                    .iter()
                    .map(|k| map.get(k).copied().unwrap_or_default().mixed() as f64 / 1_000_000.0)
                    .collect(),
            }
        })
        .collect()
}
fn fit(
    cycle: &Cycle,
    sets: [&[QuotaInferenceAggregate]; 3],
    keys: &[Key],
    selected: &[usize],
) -> Result<Vec<estimate::Capacity>, &'static str> {
    let base = model::model_capacities(&model_blocks(cycle, sets[0], keys, selected), keys.len())
        .map_err(reason)?;
    for rows in &sets[1..] {
        let shifted =
            model::model_capacities(&model_blocks(cycle, rows, keys, selected), keys.len())
                .map_err(|_| "attribution_unstable")?;
        if base
            .iter()
            .zip(shifted)
            .any(|(a, b)| (b.median / a.median - 1.0).abs() > 0.35)
        {
            return Err("attribution_unstable");
        }
    }
    Ok(base)
}
fn required_blocks(contributors: usize) -> usize {
    8.max(2 * contributors + 2)
}
fn near97(cycle: &Cycle, rows: &[QuotaInferenceAggregate], keys: &[Key]) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut ranges: Vec<Option<(f64, f64)>> = vec![None; keys.len()];
    for i in 0..cycle.blocks.len() {
        let map = interval(rows, i + 1);
        let mut shares = Vec::new();
        let mut valid = true;
        for key in keys {
            let m = map.get(key).copied().unwrap_or_default();
            if m.mixed() == 0 {
                shares.push(None);
                continue;
            }
            if !m.hit().is_some_and(|h| (0.96..=0.98).contains(&h)) {
                valid = false;
                break;
            }
            shares.push(m.output_share());
        }
        if !valid {
            continue;
        }
        let candidate: Vec<_> = ranges
            .iter()
            .zip(shares)
            .map(|(range, share)| match (*range, share) {
                (Some((lo, hi)), Some(v)) => Some((lo.min(v), hi.max(v))),
                (None, Some(v)) => Some((v, v)),
                (r, None) => r,
            })
            .collect();
        if candidate.iter().flatten().any(|(lo, hi)| hi - lo > 0.01) {
            continue;
        }
        ranges = candidate;
        indices.push(i);
    }
    indices
}

pub(crate) fn compute(
    evidence: &SnapshotEvidence,
    cycle: Option<&Cycle>,
    sets: [&[QuotaInferenceAggregate]; 3],
    now: DateTime<Utc>,
) -> InferenceResponse {
    let as_of = (!evidence.overflow)
        .then(|| evidence.snapshots.last().map(|s| s.observed_at))
        .flatten();
    let failed = evidence
        .refresh_state
        .as_ref()
        .is_some_and(|s| s.consecutive_failures > 0 && s.last_attempt_at > as_of);
    let stale = failed || as_of.is_none_or(|at| now - at > Duration::minutes(10));
    let mut result = InferenceResponse {
        method_version: "observed-v1",
        status: if stale {
            "stale"
        } else {
            "insufficient_signal"
        },
        as_of,
        latest_snapshot_at: as_of,
        calculated_at: now,
        span_start: cycle.map(|c| c.start),
        consumed_pp: 0.0,
        usable_blocks: 0,
        account: Estimate::unavailable(if evidence.overflow {
            "snapshot_overflow"
        } else {
            "insufficient_signal"
        }),
        models: Vec::new(),
        models_truncated: false,
        unknown_tokens: 0,
        overflow_tokens: 0,
        omitted_rows: 0,
        observation_provenance: evidence.observation_provenance,
        account_history_assumed_stable: evidence.account_history_assumed_stable,
        current_refresh_failed: failed,
        account_identity_known: false,
        cached: false,
    };
    let Some(cycle) = cycle else {
        return result;
    };
    let totals = interval(sets[0], 0);
    result.omitted_rows = sets[0]
        .iter()
        .filter(|r| r.interval_index == 1)
        .map(|r| {
            r.invalid_duration_count
                .saturating_add(r.error_count)
                .saturating_add(r.missing_usage_count)
        })
        .sum();
    result.models_truncated = sets[0]
        .iter()
        .any(|r| r.distinct_models > 32 || (r.bucket == "overflow" && meters(r).mixed() > 0));
    result.unknown_tokens = totals
        .get(&("unknown".into(), None))
        .copied()
        .unwrap_or_default()
        .mixed();
    result.overflow_tokens = totals
        .get(&("overflow".into(), None))
        .copied()
        .unwrap_or_default()
        .mixed();
    result.usable_blocks = cycle.blocks.len();
    result.consumed_pp = cycle.blocks.iter().map(|b| b.quota_pp).sum();
    let account = if cycle.overflow {
        Err(Unavailable::InsufficientSignal)
    } else {
        estimate::account_with_attribution(
            &observations(cycle, sets[0]),
            &observations(cycle, sets[1]),
            &observations(cycle, sets[2]),
        )
    };
    result.account = match account {
        Ok(c) => Estimate::available(c, 1_000_000.0),
        Err(e) => Estimate::unavailable(if cycle.overflow {
            "block_overflow"
        } else {
            account_reason(e)
        }),
    };
    if result.account.reason.is_none() && !stale {
        result.status = "available";
    }
    let mut key_set = BTreeSet::new();
    for rows in sets {
        for r in rows
            .iter()
            .filter(|r| r.interval_index > 1 && meters(r).mixed() > 0)
        {
            key_set.insert((r.bucket.clone(), r.model.clone()));
        }
    }
    let keys: Vec<_> = key_set.into_iter().collect();
    let selected: Vec<_> = (0..cycle.blocks.len()).collect();
    let actual = if result.models_truncated {
        Err("model_truncated")
    } else if cycle.overflow {
        Err("block_overflow")
    } else {
        fit(cycle, sets, &keys, &selected)
    };
    let near_indices = near97(cycle, sets[0], &keys);
    let near_blocks = near_indices.len();
    let near = if near_indices.is_empty() {
        Err("at97_no_sample")
    } else if result.models_truncated || cycle.overflow {
        Err("at97_unidentifiable")
    } else {
        fit(cycle, sets, &keys, &near_indices)
    };
    let required = required_blocks(keys.len());
    let actual_blocks = selected.len();
    for ((bucket, name), m) in totals {
        if bucket != "model" || m.mixed() <= 0 {
            continue;
        }
        let Some(name) = name else {
            continue;
        };
        let pos = keys
            .iter()
            .position(|key| key == &(bucket.clone(), Some(name.clone())));
        let map = |fit: &Result<Vec<estimate::Capacity>, &'static str>, missing| match (fit, pos) {
            (Ok(values), Some(i)) => Estimate::available(values[i], 1.0),
            (Err(e), _) => Estimate::unavailable(e),
            _ => Estimate::unavailable(missing),
        };
        result.models.push(ModelRow {
            model: name,
            mixed_tokens: m.mixed(),
            fresh_tokens: m.fresh,
            cache_read_tokens: m.read,
            cache_write_tokens: m.write,
            output_tokens: m.output,
            cache_hit: m.hit(),
            output_share: m.output_share(),
            actual: map(&actual, "insufficient_signal").with_blocks(actual_blocks, required),
            observed_near_97: map(&near, "at97_no_sample").with_blocks(near_blocks, required),
        });
    }
    result
}
