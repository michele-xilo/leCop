use super::episodes::GroupExtra;
use super::numeric::{DimStats, merge_stats};
use super::{PHASE_DATA, Severity, Tagged, tag};
use crate::dataset::Dataset;
use anyhow::Result;
use std::collections::BTreeMap;

/// A bound for every dimension of a feature, or one value per dimension.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum Bound {
    All(f64),
    PerDim(Vec<f64>),
}

/// Limits a user declares for one feature in `lecop.toml`; a breach is an error because the user set the bar.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureLimits {
    pub min: Option<Bound>,
    pub max: Option<Bound>,
    /// Largest allowed change of any action dimension between two frames; only the action has it.
    pub max_step: Option<f64>,
}

/// Limits by feature name: `action` or an `observation.*` feature.
pub type Limits = BTreeMap<String, FeatureLimits>;

/// One message per dimension whose data runs past `bound` (`name` is `min` or `max`), or the width error.
pub(super) fn bound_breaches(
    feature: &str,
    st: &[DimStats],
    bound: &Bound,
    name: &str,
) -> Result<Vec<String>, String> {
    if let Bound::PerDim(v) = bound
        && v.len() != st.len()
    {
        return Err(format!(
            "{feature}: {name} has {} values but the feature has {} dimensions",
            v.len(),
            st.len()
        ));
    }
    let at = |j: usize| match bound {
        Bound::All(x) => *x,
        Bound::PerDim(v) => v[j],
    };
    Ok((0..st.len())
        .filter_map(|j| {
            let (b, x) = (at(j), if name == "min" { st[j].min } else { st[j].max });
            let breach = if name == "min" { x < b } else { x > b };
            breach.then(|| {
                let side = if name == "min" { "below" } else { "above" };
                format!("dimension {j} reaches {x}, {side} {name} {b}")
            })
        })
        .collect())
}

/// Findings for the limits a user declared; a dataset-wide finding per feature and kind, never one per value.
pub(super) fn limit_issues(
    ds: &Dataset,
    action: Option<&str>,
    extras: &[GroupExtra],
    limits: &Limits,
) -> Vec<Tagged> {
    if limits.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut push = |pos, severity, episode, code, message: String| {
        out.push(tag(pos, PHASE_DATA, severity, episode, code, message));
    };
    // The config calls the action `action`, whichever of `action` and `actions` the dataset uses.
    let lookup = |key: &str| {
        limits.get(key).or_else(|| {
            (Some(key) == action)
                .then(|| limits.get("action"))
                .flatten()
        })
    };
    for (key, st) in merge_stats(extras) {
        let Some(lim) = lookup(key) else { continue };
        for (name, bound) in [("min", &lim.min), ("max", &lim.max)] {
            let Some(bound) = bound else { continue };
            match bound_breaches(key, &st, bound, name) {
                Err(m) => push(None, Severity::Error, None, "limit_width", m),
                Ok(b) if b.is_empty() => {}
                Ok(b) => {
                    let more = b.len().saturating_sub(5);
                    let tail = if more > 0 {
                        format!("; and {more} more")
                    } else {
                        String::new()
                    };
                    let shown: Vec<&str> = b.iter().take(5).map(String::as_str).collect();
                    push(
                        None,
                        Severity::Error,
                        None,
                        "limit_out_of_range",
                        format!("{key}: {}{tail}", shown.join("; ")),
                    );
                }
            }
        }
    }
    if let (Some(key), Some(max)) = (
        action,
        lookup(action.unwrap_or_default()).and_then(|l| l.max_step),
    ) {
        // (position, dimension, change) of every episode with a step beyond the limit.
        let over: Vec<(usize, usize, f64)> = extras
            .iter()
            .flat_map(|g| &g.episodes)
            .filter_map(|e| {
                let s = e.steps.as_ref()?;
                let (j, v) = (0..s.min.len())
                    .map(|j| (j, s.max[j].max(-s.min[j])))
                    .max_by(|a, b| a.1.total_cmp(&b.1))?;
                (v > max).then_some((e.pos, j, v))
            })
            .collect();
        if let Some(&(pos, j, v)) = over.iter().min_by_key(|o| o.0) {
            let more = over.len() - 1;
            let tail = if more > 0 {
                format!("; and {more} more episodes")
            } else {
                String::new()
            };
            push(
                Some(pos),
                Severity::Error,
                Some(ds.episodes[pos].index),
                "limit_step",
                format!(
                    "{key}: dimension {j} changes by {v} in one frame, above max_step {max}{tail}"
                ),
            );
        }
    }
    out
}
