use super::episodes::GroupExtra;
use super::numeric::DimStats;
use super::{PHASE_DATA, Severity, Tagged, tag};
use crate::dataset::{Dataset, Episode};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;

/// Feature names in the statistics file of this format version (`meta/stats.json` for v2.0 and v3,
/// `meta/episodes_stats.jsonl` for v2.1), or why there are none.
pub(super) fn stats_keys(ds: &Dataset) -> Result<Vec<String>, String> {
    let stats = stats_object(ds)?;
    Ok(stats
        .as_object()
        .map_or(vec![], |o| o.keys().cloned().collect()))
}

/// The object mapping feature names to their statistics, or why there is none.
pub(super) fn stats_object(ds: &Dataset) -> Result<serde_json::Value, String> {
    let v2_1 = ds.info.codebase_version.starts_with("v2.1");
    let name = if v2_1 {
        "episodes_stats.jsonl"
    } else {
        "stats.json"
    };
    let text = std::fs::read_to_string(ds.root.join("meta").join(name))
        .map_err(|_| format!("meta/{name} is missing or unreadable"))?;
    // ponytail: v2.1 reads the first episode's entry only; a feature dropped from later episodes is not seen.
    let json = if v2_1 {
        text.lines().next().unwrap_or("")
    } else {
        &text
    };
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("meta/{name} is not valid JSON: {e}"))?;
    let stats = if v2_1 { &value["stats"] } else { &value };
    if stats.is_object() {
        Ok(stats.clone())
    } else {
        Err(format!("meta/{name} has no statistics object"))
    }
}

/// The dimensions whose standard deviation is zero, per action or observation vector. Normalising by a zero std
/// gives NaN or Inf. v2.1 keeps statistics per episode, where a constant stretch is ordinary, so it is not judged.
pub(super) fn zero_std_dims(stats: &serde_json::Value) -> Vec<(String, Vec<usize>)> {
    let Some(map) = stats.as_object() else {
        return vec![];
    };
    map.iter()
        .filter(|(k, _)| k.starts_with("action") || k.starts_with("observation."))
        .filter_map(|(k, v)| {
            let std = v.get("std")?.as_array()?;
            let zeros: Vec<usize> = std
                .iter()
                .enumerate()
                .filter(|(_, x)| x.as_f64() == Some(0.0))
                .map(|(j, _)| j)
                .collect();
            (!zeros.is_empty()).then(|| (k.clone(), zeros))
        })
        .collect()
}

pub(super) fn zero_std_findings(ds: &Dataset) -> Vec<(&'static str, String)> {
    if ds.info.codebase_version.starts_with("v2.1") {
        return vec![];
    }
    let Ok(stats) = stats_object(ds) else {
        return vec![];
    };
    zero_std_dims(&stats)
        .into_iter()
        .map(|(k, zeros)| {
            let dims: Vec<String> = zeros.iter().map(|j| format!("dimension {j}")).collect();
            (
                "stats_zero_std",
                format!(
                    "{k}: {} has zero std in stats.json, normalising divides by zero",
                    dims.join(", ")
                ),
            )
        })
        .collect()
}

pub(super) fn stats_findings(ds: &Dataset) -> Vec<(&'static str, String)> {
    match stats_keys(ds) {
        Err(m) => vec![("stats_missing", m)],
        Ok(keys) => {
            let missing: Vec<&str> = ds
                .info
                .features
                .iter()
                .filter(|(k, f)| f.dtype != "string" && !keys.contains(k))
                .map(|(k, _)| k.as_str())
                .collect();
            if missing.is_empty() {
                vec![]
            } else {
                vec![(
                    "stats_feature_missing",
                    format!("no statistics for {}", missing.join(", ")),
                )]
            }
        }
    }
}

/// Tolerance of the mean and std, as a fraction of the std: far above float32 rounding (8e-5 at most on the real
/// datasets) and far below a stale file, where whole episodes are added or removed.
const MOMENT_TOL: f64 = 1e-3;
/// Tolerance of the mean and std as a fraction of the value's magnitude. LeRobot accumulates in float32: stored means
/// of a constant 173.496 column are off by 8e-6 relative, and its std is stored as 0.0014 instead of 0
/// (c_so100_press_red_button). A stale file differs by far more.
const FLOAT32_SLACK: f64 = 5e-5;
/// Tolerance of min and max, as a fraction of the range or the value. Real files match exactly.
const EXTREME_TOL: f64 = 1e-6;

/// A number with at most 6 decimals and no trailing zeros, so a message stays readable.
fn short(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// How the statistics of one feature stored in `file` differ from the values measured in the data, one sentence
/// each. A statistic that is not stored is not compared.
pub(super) fn stats_differences(
    stored: &serde_json::Value,
    measured: &[DimStats],
    file: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let n = measured.first().map_or(0, |d| d.n);
    if n == 0 {
        return out;
    }
    let count = stored.get("count").and_then(|c| match c {
        serde_json::Value::Array(a) => a.first()?.as_f64(),
        other => other.as_f64(),
    });
    if let Some(c) = count.filter(|c| *c != n as f64) {
        out.push(format!(
            "count is {c} in {file} but the data has {n} frames"
        ));
    }
    let moments: Vec<(f64, f64)> = measured
        .iter()
        .map(|d| {
            let mean = d.sum / n as f64;
            (mean, (d.sumsq / n as f64 - mean * mean).max(0.0).sqrt())
        })
        .collect();
    for stat in ["min", "max", "mean", "std"] {
        let Some(values) = stored.get(stat).and_then(|v| v.as_array()) else {
            continue;
        };
        if values.len() != measured.len() {
            out.push(format!(
                "{stat} has {} values in {file} but the feature has {} dimensions",
                values.len(),
                measured.len()
            ));
            continue;
        }
        for (j, (v, d)) in values.iter().zip(measured).enumerate() {
            let Some(stored) = v.as_f64() else { continue };
            let (mean, std) = moments[j];
            let (found, tol) = match stat {
                "min" => (
                    d.min,
                    EXTREME_TOL * (d.max - d.min).max(d.min.abs()).max(1e-12),
                ),
                "max" => (
                    d.max,
                    EXTREME_TOL * (d.max - d.min).max(d.max.abs()).max(1e-12),
                ),
                "mean" => (mean, MOMENT_TOL * std + FLOAT32_SLACK * mean.abs().max(1.0)),
                _ => (std, MOMENT_TOL * std + FLOAT32_SLACK * mean.abs().max(1.0)),
            };
            if (stored - found).abs() > tol {
                out.push(format!(
                    "dimension {j} {stat} is {} in {file} but {} in the data",
                    short(stored),
                    short(found)
                ));
            }
        }
    }
    out
}

/// The first difference, how many more there are, and what to do about them; None when the feature matches.
fn mismatch_message(key: &str, diffs: &[String], out_of_date: &str) -> Option<String> {
    let (first, rest) = diffs.split_first()?;
    let more = match rest.len() {
        0 => String::new(),
        1 => " (and 1 more difference)".to_string(),
        n => format!(" (and {n} more differences)"),
    };
    Some(format!("{key}: {first}{more}; {out_of_date}"))
}

/// `stats_mismatch` findings: global `stats.json` (v3.x and v2.0) against the data of the analysed float features.
/// Skipped when the data read does not add up to the episode metadata, where other findings already explain it.
pub(super) fn stats_mismatch_findings(
    ds: &Dataset,
    measured: &std::collections::BTreeMap<&str, Vec<DimStats>>,
) -> Vec<(&'static str, String)> {
    if ds.info.codebase_version.starts_with("v2.1") {
        return vec![];
    }
    let Ok(stats) = stats_object(ds) else {
        return vec![];
    };
    let expected: u64 = ds.episodes.iter().map(|e| e.length as u64).sum();
    measured
        .iter()
        .filter(|(_, st)| st.first().is_some_and(|d| d.n == expected))
        .filter_map(|(key, st)| {
            let diffs = stats_differences(stats.get(*key)?, st, "stats.json");
            let m = mismatch_message(key, &diffs, "stats.json is out of date with the data")?;
            Some(("stats_mismatch", m))
        })
        .collect()
}

/// One line of `meta/episodes_stats.jsonl`. The statistics of a feature stay unparsed until they are compared: the
/// image features' nested arrays are most of the file and no check looks at them.
#[derive(serde::Deserialize)]
struct StatsLine<'a> {
    episode_index: i64,
    #[serde(borrow)]
    stats: HashMap<&'a str, &'a serde_json::value::RawValue>,
}

/// The lines of `meta/episodes_stats.jsonl` (v2.1) that parse, by episode index. A line that does not is left out:
/// `stats_keys` already reports an unreadable file, and a bad later line only costs that episode its check.
fn parse_episode_stats(text: &str) -> HashMap<i64, StatsLine<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .into_par_iter()
        .filter_map(|l| serde_json::from_str::<StatsLine>(l).ok())
        .map(|l| (l.episode_index, l))
        .collect()
}

/// `stats_mismatch` findings for v2.1: each episode's entry in `episodes_stats.jsonl` against that episode's rows.
/// A v2.1 data file holds one episode, so the statistics of a file are the episode's. `groups` and `extras` are the
/// data pass's runs of episodes and what it measured in each, in the same order.
pub(super) fn episode_stats_findings(
    ds: &Dataset,
    groups: &[(usize, &[Episode])],
    extras: &[GroupExtra],
) -> Vec<Tagged> {
    if !ds.info.codebase_version.starts_with("v2.1") {
        return vec![];
    }
    let Ok(text) = std::fs::read_to_string(ds.root.join("meta/episodes_stats.jsonl")) else {
        return vec![];
    };
    let stored = parse_episode_stats(&text);
    groups
        .iter()
        .zip(extras)
        .filter_map(|(&(pos, eps), extra)| {
            let [e] = eps else { return None };
            let entry = &stored.get(&(e.index as i64))?.stats;
            let found: Vec<String> = extra
                .stats
                .iter()
                // Rows that do not add up to the episode's length are explained by other findings.
                .filter(|(_, st)| st.first().is_some_and(|d| d.n == e.length as u64))
                .filter_map(|(key, st)| {
                    let stat: serde_json::Value =
                        serde_json::from_str(entry.get(key.as_str())?.get()).ok()?;
                    let diffs = stats_differences(&stat, st, "episodes_stats.jsonl");
                    mismatch_message(
                        key,
                        &diffs,
                        "episodes_stats.jsonl is out of date for this episode",
                    )
                })
                .collect();
            Some(found.into_iter().map(move |m| {
                tag(
                    Some(pos),
                    PHASE_DATA,
                    Severity::Warning,
                    Some(e.index),
                    "stats_mismatch",
                    m,
                )
            }))
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::dim_stats;

    /// The statistics of 4 rows of width 2, as LeRobot stores them.
    fn stored() -> serde_json::Value {
        serde_json::json!({
            "min": [1.0, 2.0], "max": [7.0, 8.0], "mean": [4.0, 5.0],
            "std": [2.23606797749979, 2.23606797749979], "count": [4]
        })
    }

    fn measured() -> Vec<DimStats> {
        dim_stats(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0], 2)
    }

    #[test]
    fn matching_statistics_differ_nowhere() {
        assert_eq!(
            stats_differences(&stored(), &measured(), "stats.json"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn an_entry_without_statistics_has_nothing_to_compare() {
        assert!(stats_differences(&serde_json::json!({}), &measured(), "stats.json").is_empty());
    }

    #[test]
    fn each_stale_statistic_is_named_with_both_values() {
        for (stat, dim, value) in [
            ("min", 0, 0.0),
            ("max", 1, 9.0),
            ("mean", 1, 6.0),
            ("std", 0, 3.0),
        ] {
            let mut s = stored();
            s[stat][dim] = value.into();
            let d = stats_differences(&s, &measured(), "stats.json");
            assert_eq!(d.len(), 1, "{stat}: {d:?}");
            assert!(
                d[0].contains(&format!("dimension {dim} {stat}"))
                    && d[0].contains(&format!("{value}")),
                "{stat}: {d:?}"
            );
        }
    }

    #[test]
    fn a_stale_count_is_named_with_both_values() {
        let mut s = stored();
        s["count"] = serde_json::json!([5]);
        let d = stats_differences(&s, &measured(), "stats.json");
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(
            d[0].contains("count") && d[0].contains('5') && d[0].contains('4'),
            "{d:?}"
        );
    }

    #[test]
    fn a_count_stored_as_a_plain_number_is_compared_too() {
        let mut s = stored();
        s["count"] = serde_json::json!(5);
        let d = stats_differences(&s, &measured(), "stats.json");
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].contains("count is 5"), "{d:?}");
    }

    #[test]
    fn a_feature_with_no_measured_rows_has_nothing_to_compare() {
        assert!(stats_differences(&stored(), &[], "stats.json").is_empty());
        assert!(stats_differences(&stored(), &dim_stats(&[], 2), "stats.json").is_empty());
    }

    #[test]
    fn a_different_width_is_reported_once() {
        let mut s = stored();
        s["mean"] = serde_json::json!([4.0, 5.0, 6.0]);
        let d = stats_differences(&s, &measured(), "stats.json");
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(
            d[0].contains("mean") && d[0].contains('3') && d[0].contains('2'),
            "{d:?}"
        );
    }

    #[test]
    fn float32_rounding_is_not_a_difference() {
        let mut s = stored();
        s["mean"] = serde_json::json!([4.0000002, 5.0000002]);
        s["std"] = serde_json::json!([2.2360682, 2.2360682]);
        assert!(stats_differences(&s, &measured(), "stats.json").is_empty());
    }

    #[test]
    fn float32_noise_in_a_near_constant_feature_is_not_a_difference() {
        // c_so100_press_red_button episode 0, observation.state dimension 8: constant at 173.496094 in 594 rows, stored
        // with mean 173.497498 and std 0.0014 (float32 accumulation in the tool that wrote it).
        let measured = dim_stats(&vec![173.496094; 594], 1);
        let stored = serde_json::json!({
            "min": [173.496094], "max": [173.496094], "mean": [173.497498], "std": [0.00140381], "count": [594]
        });
        assert_eq!(
            stats_differences(&stored, &measured, "episodes_stats.jsonl"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_shifted_mean_of_a_large_valued_feature_is_still_a_difference() {
        let measured = dim_stats(&vec![173.496094; 594], 1);
        let stored = serde_json::json!({"mean": [173.6], "count": [594]});
        let d = stats_differences(&stored, &measured, "episodes_stats.jsonl");
        assert_eq!(d.len(), 1, "{d:?}");
    }

    #[test]
    fn zero_std_ignores_statistics_that_are_not_an_object() {
        assert!(zero_std_dims(&serde_json::json!([1, 2])).is_empty());
    }
}
