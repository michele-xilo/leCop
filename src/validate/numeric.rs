use super::episodes::GroupExtra;
use super::{PHASE_DATA, Severity, Tagged, tag};
use std::collections::BTreeMap;

/// Running statistics of one dimension of a float feature over the whole dataset.
#[derive(Clone, Copy, Debug)]
pub struct DimStats {
    pub n: u64,
    pub min: f64,
    pub max: f64,
    pub sum: f64,
    pub sumsq: f64,
    pub at_min: u64,
    pub at_max: u64,
}

impl DimStats {
    pub(super) fn empty() -> Self {
        Self {
            n: 0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            sum: 0.0,
            sumsq: 0.0,
            at_min: 0,
            at_max: 0,
        }
    }

    pub(super) fn merge(&mut self, o: &Self) {
        self.n += o.n;
        self.sum += o.sum;
        self.sumsq += o.sumsq;
        match o.min.partial_cmp(&self.min) {
            Some(std::cmp::Ordering::Less) => (self.min, self.at_min) = (o.min, o.at_min),
            Some(std::cmp::Ordering::Equal) => self.at_min += o.at_min,
            _ => {}
        }
        match o.max.partial_cmp(&self.max) {
            Some(std::cmp::Ordering::Greater) => (self.max, self.at_max) = (o.max, o.at_max),
            Some(std::cmp::Ordering::Equal) => self.at_max += o.at_max,
            _ => {}
        }
    }
}

/// Per-dimension statistics of `rows * width` values, row-major. The counts at the extremes are only needed for
/// dimensions that sit on one (clipping), so a sample of rows decides which dimensions get an exact count pass:
/// counting every value cost about a third of the whole statistics time (criterion, synthetic 600k frames).
// ponytail: a dimension counts as possibly clipped when over 90% of every 16th row is on an extreme; a file where
// that holds for under 90% of rows but the whole dataset is over 99% on the extreme is missed. Count exactly if it matters.
pub fn dim_stats(vals: &[f64], width: usize) -> Vec<DimStats> {
    let mut st = vec![DimStats::empty(); width];
    if width == 0 {
        return st;
    }
    // Separate arrays rather than a Vec<DimStats>: the loop vectorizes over dimensions.
    let (mut mn, mut mx) = (vec![f64::INFINITY; width], vec![f64::NEG_INFINITY; width]);
    let (mut sum, mut sumsq) = (vec![0.0f64; width], vec![0.0f64; width]);
    for row in vals.chunks_exact(width) {
        for j in 0..width {
            let x = row[j];
            mn[j] = mn[j].min(x);
            mx[j] = mx[j].max(x);
            sum[j] += x;
            sumsq[j] += x * x;
        }
    }
    for (j, s) in st.iter_mut().enumerate() {
        (s.min, s.max, s.sum, s.sumsq) = (mn[j], mx[j], sum[j], sumsq[j]);
    }
    let rows = vals.len() / width;
    let (mut lo, mut hi, mut sampled) = (vec![0u64; width], vec![0u64; width], 0u64);
    for row in vals.chunks_exact(width).step_by(16) {
        sampled += 1;
        for (j, &x) in row.iter().enumerate() {
            lo[j] += u64::from(x == st[j].min);
            hi[j] += u64::from(x == st[j].max);
        }
    }
    for (j, s) in st.iter_mut().enumerate() {
        s.n = rows as u64;
        if lo[j] * 10 > sampled * 9 || hi[j] * 10 > sampled * 9 {
            for row in vals.chunks_exact(width) {
                s.at_min += u64::from(row[j] == s.min);
                s.at_max += u64::from(row[j] == s.max);
            }
        }
    }
    st
}

/// Dataset-wide warnings for one feature from its merged dimension statistics: dimensions that never change, sit on
/// one extreme, or reach values far from the mean. Judged from 100 frames up. `(code, message)`.
pub fn numeric_findings(
    feature: &str,
    st: &[DimStats],
    is_action: bool,
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let n = st.first().map_or(0, |s| s.n);
    if n < 100 {
        return out;
    }
    let constant: Vec<usize> = (0..st.len()).filter(|&j| st[j].min == st[j].max).collect();
    // A fully constant action is already reported as `action_constant`.
    if !constant.is_empty() && !(is_action && constant.len() == st.len()) {
        let dims: Vec<String> = constant
            .iter()
            .map(|j| format!("{j} (= {})", st[*j].min))
            .collect();
        out.push((
            "constant_dimension",
            format!("{feature}: dimension {} never changes", dims.join(", ")),
        ));
    }
    for (j, s) in st.iter().enumerate().filter(|(_, s)| s.min < s.max) {
        let nf = s.n as f64;
        let (at, edge, what) = if s.at_max >= s.at_min {
            (s.at_max, s.max, "maximum")
        } else {
            (s.at_min, s.min, "minimum")
        };
        if at as f64 / nf > 0.99 {
            out.push((
                "clipped_dimension",
                format!(
                    "{feature}: dimension {j} is at its {what} ({edge}) in {:.1}% of the frames",
                    100.0 * at as f64 / nf
                ),
            ));
            continue;
        }
        let mean = s.sum / nf;
        let std = (s.sumsq / nf - mean * mean).max(0.0).sqrt();
        if !std.is_finite() || std == 0.0 {
            continue;
        }
        let z = ((s.max - mean) / std).max((mean - s.min) / std);
        if z > 10.0 {
            out.push((
                "outlier_values",
                format!(
                    "{feature}: dimension {j} reaches {z:.0} standard deviations from its mean"
                ),
            ));
        }
    }
    out
}

/// The action and the observation vectors; ids, timestamps and `next.*` flags are legitimately constant or binary.
pub(super) fn is_analyzed(key: &str, action: Option<&str>) -> bool {
    Some(key) == action || key.starts_with("observation.")
}

pub(super) fn merge_stats(extras: &[GroupExtra]) -> BTreeMap<&str, Vec<DimStats>> {
    let mut merged: BTreeMap<&str, Vec<DimStats>> = BTreeMap::new();
    for (key, st) in extras.iter().flat_map(|e| &e.stats) {
        match merged.entry(key) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(st.clone());
            }
            std::collections::btree_map::Entry::Occupied(mut o) => {
                for (a, b) in o.get_mut().iter_mut().zip(st) {
                    a.merge(b);
                }
            }
        }
    }
    merged
}

pub(super) fn numeric_issues(extras: &[GroupExtra], action: Option<&str>) -> Vec<Tagged> {
    merge_stats(extras)
        .iter()
        .flat_map(|(key, st)| {
            numeric_findings(key, st, Some(*key) == action)
                .into_iter()
                .map(|(code, message)| {
                    tag(None, PHASE_DATA, Severity::Warning, None, code, message)
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_width_columns_have_no_dimensions() {
        assert!(dim_stats(&[1.0, 2.0], 0).is_empty());
    }
}
