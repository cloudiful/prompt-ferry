#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Observation {
    pub quota_pp: f64,
    pub mixed_tokens: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Capacity {
    pub median: f64,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Unavailable {
    InsufficientSignal,
    LowConfidence,
    AttributionUnstable,
}

fn weighted_quantile(values: &mut [(f64, f64)], quantile: f64) -> Option<f64> {
    values.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = values.iter().map(|(_, weight)| weight).sum();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let target = total * quantile;
    let mut cumulative = 0.0;
    for &(value, weight) in values.iter() {
        cumulative += weight;
        if cumulative >= target {
            return Some(value);
        }
    }
    values.last().map(|(value, _)| *value)
}

pub(super) fn account_capacity(blocks: &[Observation]) -> Result<Capacity, Unavailable> {
    if blocks.len() < 3
        || blocks.iter().any(|block| {
            !block.quota_pp.is_finite()
                || block.quota_pp <= 0.0
                || !block.mixed_tokens.is_finite()
                || block.mixed_tokens < 0.0
        })
    {
        return Err(Unavailable::InsufficientSignal);
    }
    let total_pp: f64 = blocks.iter().map(|block| block.quota_pp).sum();
    if !total_pp.is_finite() || total_pp < 15.0 {
        return Err(Unavailable::InsufficientSignal);
    }
    let mut ratios: Vec<_> = blocks
        .iter()
        .map(|block| (block.mixed_tokens / block.quota_pp, block.quota_pp))
        .collect();
    if ratios.iter().any(|(ratio, _)| !ratio.is_finite()) {
        return Err(Unavailable::InsufficientSignal);
    }
    let median = weighted_quantile(&mut ratios, 0.5).ok_or(Unavailable::InsufficientSignal)?;
    let lower = weighted_quantile(&mut ratios, 0.25).ok_or(Unavailable::InsufficientSignal)?;
    let upper = weighted_quantile(&mut ratios, 0.75).ok_or(Unavailable::InsufficientSignal)?;
    if median <= 0.0 {
        return Err(Unavailable::InsufficientSignal);
    }
    if (upper - lower) / median > 0.5 {
        return Err(Unavailable::LowConfidence);
    }
    let capacity = Capacity {
        median: median * 100.0,
        lower: lower * 100.0,
        upper: upper * 100.0,
    };
    if !capacity.upper.is_finite() {
        return Err(Unavailable::InsufficientSignal);
    }
    Ok(capacity)
}

pub(super) fn account_with_attribution(
    base: &[Observation],
    earlier: &[Observation],
    later: &[Observation],
) -> Result<Capacity, Unavailable> {
    let result = account_capacity(base)?;
    for shifted in [earlier, later] {
        if shifted.len() != base.len()
            || shifted
                .iter()
                .zip(base)
                .any(|(shifted, base)| shifted.quota_pp != base.quota_pp)
        {
            return Err(Unavailable::AttributionUnstable);
        }
        let shifted = account_capacity(shifted).map_err(|_| Unavailable::AttributionUnstable)?;
        if (shifted.median / result.median - 1.0).abs() > 0.35 {
            return Err(Unavailable::AttributionUnstable);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(ratios: &[f64]) -> Vec<Observation> {
        ratios
            .iter()
            .map(|ratio| Observation {
                quota_pp: 5.0,
                mixed_tokens: ratio * 5.0,
            })
            .collect()
    }

    #[test]
    fn account_capacity_uses_weighted_empirical_quantiles() {
        let observations = [
            Observation {
                quota_pp: 5.0,
                mixed_tokens: 500.0,
            },
            Observation {
                quota_pp: 10.0,
                mixed_tokens: 1200.0,
            },
            Observation {
                quota_pp: 5.0,
                mixed_tokens: 700.0,
            },
        ];
        assert_eq!(
            account_capacity(&observations),
            Ok(Capacity {
                median: 12000.0,
                lower: 10000.0,
                upper: 12000.0
            })
        );
    }

    #[test]
    fn account_capacity_rejects_insufficient_or_wide_samples() {
        assert_eq!(
            account_capacity(&blocks(&[100.0, 100.0])),
            Err(Unavailable::InsufficientSignal)
        );
        assert_eq!(
            account_capacity(&blocks(&[10.0, 100.0, 200.0])),
            Err(Unavailable::LowConfidence)
        );
        assert_eq!(
            account_capacity(&blocks(&[0.0, 0.0, 0.0])),
            Err(Unavailable::InsufficientSignal)
        );
    }

    #[test]
    fn shifted_boundaries_must_preserve_denominators_and_capacity() {
        let base = blocks(&[100.0, 100.0, 100.0]);
        assert!(account_with_attribution(&base, &blocks(&[90.0; 3]), &blocks(&[110.0; 3])).is_ok());
        assert_eq!(
            account_with_attribution(&base, &blocks(&[50.0; 3]), &base),
            Err(Unavailable::AttributionUnstable)
        );
        let mut changed = base.clone();
        changed[0].quota_pp = 6.0;
        assert_eq!(
            account_with_attribution(&base, &changed, &base),
            Err(Unavailable::AttributionUnstable)
        );
    }
}
