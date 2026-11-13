use super::estimate::{Capacity, Observation, Unavailable, account_capacity};

#[derive(Clone, Debug)]
pub(super) struct ModelBlock {
    pub quota_pp: f64,
    pub mixed_millions: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelUnavailable {
    InsufficientSignal,
    Complexity,
    ModelMixFlat,
    IllConditioned,
    PoorFit,
    Unstable,
}

fn solve(blocks: &[ModelBlock], columns: usize) -> Result<Vec<f64>, ModelUnavailable> {
    let rows = blocks.len();
    let mut vectors: Vec<Vec<f64>> = (0..columns)
        .map(|j| blocks.iter().map(|b| b.mixed_millions[j]).collect())
        .collect();
    let scales: Vec<f64> = vectors
        .iter()
        .map(|v| v.iter().map(|x| x * x).sum::<f64>().sqrt())
        .collect();
    if scales.iter().any(|s| !s.is_finite() || *s <= 0.0) {
        return Err(ModelUnavailable::IllConditioned);
    }
    for (vector, scale) in vectors.iter_mut().zip(&scales) {
        for value in vector {
            *value /= scale;
        }
    }
    let mut permutation: Vec<usize> = (0..columns).collect();
    let mut upper = vec![vec![0.0; columns]; columns];
    let mut projection = vec![0.0; columns];
    for k in 0..columns {
        let pivot = (k..columns)
            .max_by(|&a, &b| {
                let norm = |index: usize| vectors[index].iter().map(|x| x * x).sum::<f64>();
                norm(a).total_cmp(&norm(b))
            })
            .ok_or(ModelUnavailable::IllConditioned)?;
        vectors.swap(k, pivot);
        permutation.swap(k, pivot);
        for row in upper.iter_mut().take(k) {
            row.swap(k, pivot);
        }
        let norm = vectors[k].iter().map(|x| x * x).sum::<f64>().sqrt();
        if !norm.is_finite() || norm < 1e-3 {
            return Err(ModelUnavailable::IllConditioned);
        }
        upper[k][k] = norm;
        for value in &mut vectors[k] {
            *value /= norm;
        }
        projection[k] = vectors[k]
            .iter()
            .zip(blocks)
            .map(|(x, b)| x * b.quota_pp)
            .sum();
        for j in (k + 1)..columns {
            let dot: f64 = (0..rows).map(|i| vectors[k][i] * vectors[j][i]).sum();
            upper[k][j] = dot;
            for i in 0..rows {
                vectors[j][i] -= dot * vectors[k][i];
            }
        }
    }
    let mut coefficients = vec![0.0; columns];
    let mut ordered = vec![0.0; columns];
    for i in (0..columns).rev() {
        let remainder: f64 = ((i + 1)..columns).map(|j| upper[i][j] * ordered[j]).sum();
        ordered[i] = (projection[i] - remainder) / upper[i][i];
        coefficients[permutation[i]] = ordered[i] / scales[permutation[i]];
    }
    if coefficients.iter().any(|x| !x.is_finite()) {
        return Err(ModelUnavailable::IllConditioned);
    }
    Ok(coefficients)
}

fn weighted_residual(blocks: &[ModelBlock], coefficients: &[f64]) -> f64 {
    let mut residuals: Vec<_> = blocks
        .iter()
        .map(|block| {
            let predicted: f64 = block
                .mixed_millions
                .iter()
                .zip(coefficients)
                .map(|(x, c)| x * c)
                .sum();
            (
                ((predicted - block.quota_pp) / block.quota_pp).abs(),
                block.quota_pp,
            )
        })
        .collect();
    residuals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let half = residuals.iter().map(|(_, w)| w).sum::<f64>() / 2.0;
    let mut cumulative = 0.0;
    for (residual, weight) in residuals {
        cumulative += weight;
        if cumulative >= half {
            return residual;
        }
    }
    f64::INFINITY
}

fn stable_refit_capacity(base: f64, refit: f64) -> Result<f64, ModelUnavailable> {
    let base_capacity = 100.0 / base;
    let refit_capacity = 100.0 / refit;
    if base <= 0.0
        || refit <= 0.0
        || !base_capacity.is_finite()
        || !refit_capacity.is_finite()
        || (refit_capacity / base_capacity - 1.0).abs() > 0.35
    {
        return Err(ModelUnavailable::Unstable);
    }
    Ok(refit_capacity)
}

pub(super) fn model_capacities(
    blocks: &[ModelBlock],
    contributors: usize,
) -> Result<Vec<Capacity>, ModelUnavailable> {
    if contributors == 0 {
        return Err(ModelUnavailable::InsufficientSignal);
    }
    if contributors > 8 {
        return Err(ModelUnavailable::Complexity);
    }
    if blocks.len() < 8.max(2 * contributors + 2)
        || blocks.iter().any(|b| {
            !b.quota_pp.is_finite()
                || b.quota_pp <= 0.0
                || b.mixed_millions.len() != contributors
                || b.mixed_millions.iter().any(|x| !x.is_finite() || *x < 0.0)
        })
    {
        return Err(ModelUnavailable::InsufficientSignal);
    }
    if (0..contributors).any(|j| blocks.iter().filter(|b| b.mixed_millions[j] > 0.0).count() < 4) {
        return Err(ModelUnavailable::InsufficientSignal);
    }
    if contributors == 1 {
        let observations: Vec<_> = blocks
            .iter()
            .map(|b| Observation {
                quota_pp: b.quota_pp,
                mixed_tokens: b.mixed_millions[0],
            })
            .collect();
        let result = account_capacity(&observations).map_err(|e| match e {
            Unavailable::LowConfidence => ModelUnavailable::Unstable,
            _ => ModelUnavailable::InsufficientSignal,
        })?;
        let mut lower = result.median;
        let mut upper = result.median;
        for omitted in 0..observations.len() {
            let subset: Vec<_> = observations
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != omitted)
                .map(|(_, b)| *b)
                .collect();
            if let Ok(refit) = account_capacity(&subset) {
                if (refit.median / result.median - 1.0).abs() > 0.35 {
                    return Err(ModelUnavailable::Unstable);
                }
                lower = lower.min(refit.median);
                upper = upper.max(refit.median);
            }
        }
        return Ok(vec![Capacity {
            median: result.median,
            lower,
            upper,
        }]);
    }
    for j in 0..contributors {
        let fractions: Vec<_> = blocks
            .iter()
            .map(|b| {
                let total: f64 = b.mixed_millions.iter().sum();
                if total > 0.0 {
                    b.mixed_millions[j] / total
                } else {
                    0.0
                }
            })
            .collect();
        let mean = fractions.iter().sum::<f64>() / fractions.len() as f64;
        let variance =
            fractions.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / fractions.len() as f64;
        if mean <= 0.0 || variance.sqrt() / mean < 0.25 {
            return Err(ModelUnavailable::ModelMixFlat);
        }
    }
    let coefficients = solve(blocks, contributors)?;
    if coefficients.iter().any(|c| *c <= 0.0) {
        return Err(ModelUnavailable::IllConditioned);
    }
    if weighted_residual(blocks, &coefficients) > 0.25 {
        return Err(ModelUnavailable::PoorFit);
    }
    let mut lower: Vec<f64> = coefficients.iter().map(|c| 100.0 / c).collect();
    let mut upper = lower.clone();
    for omitted in 0..blocks.len() {
        let subset: Vec<_> = blocks
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != omitted)
            .map(|(_, b)| b.clone())
            .collect();
        let Ok(refit) = solve(&subset, contributors) else {
            continue;
        };
        for j in 0..contributors {
            let capacity = stable_refit_capacity(coefficients[j], refit[j])?;
            lower[j] = lower[j].min(capacity);
            upper[j] = upper[j].max(capacity);
        }
    }
    Ok(coefficients
        .iter()
        .enumerate()
        .map(|(j, c)| Capacity {
            median: 100.0 / c,
            lower: lower[j],
            upper: upper[j],
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refit_stability_uses_reciprocal_capacity_change() {
        assert_eq!(
            stable_refit_capacity(1.0, 0.70),
            Err(ModelUnavailable::Unstable)
        );
        assert!(stable_refit_capacity(1.0, 1.40).is_ok());
    }
    #[test]
    fn recovers_two_arbitrary_models() {
        let blocks: Vec<_> = (0..12)
            .map(|i| {
                let a = 1.0 + (i % 3) as f64;
                let b = 1.0 + ((i * 7) % 5) as f64;
                ModelBlock {
                    quota_pp: 2.0 * a + 4.0 * b,
                    mixed_millions: vec![a, b],
                }
            })
            .collect();
        let result = model_capacities(&blocks, 2).unwrap();
        assert!((result[0].median - 50.0).abs() < 1e-8);
        assert!((result[1].median - 25.0).abs() < 1e-8);
    }
    #[test]
    fn rejects_flat_composition() {
        let blocks: Vec<_> = (1..=10)
            .map(|i| ModelBlock {
                quota_pp: i as f64 * 6.0,
                mixed_millions: vec![i as f64, i as f64],
            })
            .collect();
        assert_eq!(
            model_capacities(&blocks, 2),
            Err(ModelUnavailable::ModelMixFlat)
        );
    }
}
