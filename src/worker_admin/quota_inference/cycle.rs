use crate::db::quota_inference::{QuotaInferenceSnapshot, SnapshotEvidence};
use chrono::{DateTime, Duration, Utc};

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub quota_pp: f64,
}

#[derive(Debug)]
pub(crate) struct Cycle {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub reset: DateTime<Utc>,
    pub plan: Option<String>,
    pub last_remaining: f64,
    pub blocks: Vec<Block>,
    pub overflow: bool,
}

struct Point {
    at: DateTime<Utc>,
    reset: DateTime<Utc>,
    remaining: f64,
    plan: Option<String>,
}

fn point(snapshot: &QuotaInferenceSnapshot) -> Option<Point> {
    if snapshot.limit_reached == Some(true) {
        return None;
    }
    let windows = snapshot.windows.as_array()?;
    let mut weekly = windows
        .iter()
        .filter(|w| w.get("window_seconds").and_then(|v| v.as_i64()) == Some(604800));
    let window = weekly.next()?;
    if weekly.next().is_some() || window.get("availability")?.as_str()? != "known" {
        return None;
    }
    let remaining = window.get("remaining_percent")?.as_f64()?;
    if !remaining.is_finite() || !(0.0..100.0).contains(&remaining) || remaining == 0.0 {
        return None;
    }
    let reset = window
        .get("reset_at")
        .and_then(|v| v.as_str())
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|v| v.with_timezone(&Utc))
        .or_else(|| {
            window
                .get("reset_after_seconds")
                .and_then(|v| v.as_i64())
                .filter(|v| *v >= 0)
                .and_then(|v| {
                    snapshot
                        .observed_at
                        .checked_add_signed(Duration::seconds(v))
                })
        })?;
    if reset <= snapshot.observed_at {
        return None;
    }
    Some(Point {
        at: snapshot.observed_at,
        reset,
        remaining,
        plan: snapshot.plan_type.clone(),
    })
}

pub(crate) fn build(evidence: &SnapshotEvidence, range_start: DateTime<Utc>) -> Option<Cycle> {
    if evidence.overflow {
        return None;
    }
    let last = point(evidence.snapshots.last()?)?;
    let mut start_index = evidence.snapshots.len() - 1;
    let mut next = point(evidence.snapshots.last()?)?;
    let mut discontinuity = false;
    for i in (0..start_index).rev() {
        let Some(p) = point(&evidence.snapshots[i]) else {
            continue;
        };
        if p.plan != last.plan
            || (p.reset - last.reset).num_seconds().abs() > 120
            || p.at >= next.at
            || p.remaining < next.remaining
        {
            discontinuity = true;
            break;
        }
        start_index = i;
        next = p;
    }
    let points: Vec<_> = evidence.snapshots[start_index..]
        .iter()
        .map(point)
        .collect();
    let mut blocks = Vec::new();
    let mut anchor: Option<&Point> = None;
    let mut previous: Option<&Point> = None;
    for current in &points {
        let Some(current) = current.as_ref() else {
            anchor = None;
            previous = None;
            continue;
        };
        if previous.is_none_or(|p| current.at - p.at > Duration::minutes(10)) {
            anchor = Some(current);
        }
        let start = anchor.expect("anchor set for first point");
        let delta = start.remaining - current.remaining;
        if delta >= 5.0 {
            blocks.push(Block {
                start: start.at,
                end: current.at,
                quota_pp: delta,
            });
            anchor = Some(current);
        }
        previous = Some(current);
    }
    let overflow = blocks.len() > 64;
    if overflow {
        blocks.clear();
    }
    Some(Cycle {
        start: if discontinuity {
            points[0].as_ref()?.at
        } else {
            (last.reset - Duration::days(7)).max(range_start)
        },
        end: last.at,
        reset: last.reset,
        plan: last.plan,
        last_remaining: last.remaining,
        blocks,
        overflow,
    })
}
