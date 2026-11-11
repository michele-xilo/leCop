use crate::dataset::Dataset;
use anyhow::Result;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_std_ignores_statistics_that_are_not_an_object() {
        assert!(zero_std_dims(&serde_json::json!([1, 2])).is_empty());
    }
}
