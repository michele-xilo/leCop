use super::episodes::GroupExtra;
use super::schema::{episode_rows, episode_runs, parse_file};
use super::{PHASE_DATA, Severity, Tagged, tag};
use crate::dataset::Dataset;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::Path;

pub(super) const JUMP_SIGMAS: f64 = 8.0;

pub(super) const SHIFT_SIGMAS: f64 = 3.0;

pub(super) const SHIFT_MIN_EPISODES: usize = 10;

pub(super) const MAX_JUMP_EPISODES: usize = 5;

/// The shift check compares means of thousands of frames; a 1-in-4 sample is as good and a quarter of the work.
pub(super) const SHIFT_STRIDE: usize = 4;

/// Per-dimension count, sum and sum of squares of the rows of one episode.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moments {
    pub n: u64,
    pub sum: Vec<f64>,
    pub sumsq: Vec<f64>,
}

impl Moments {
    pub(super) fn add(&mut self, o: &Moments) {
        if self.sum.is_empty() {
            *self = o.clone();
            return;
        }
        self.n += o.n;
        for (a, b) in self.sum.iter_mut().zip(&o.sum) {
            *a += b;
        }
        for (a, b) in self.sumsq.iter_mut().zip(&o.sumsq) {
            *a += b;
        }
    }

    /// Population mean and standard deviation per dimension.
    pub(super) fn mean_std(&self) -> (Vec<f64>, Vec<f64>) {
        let n = self.n as f64;
        let mean: Vec<f64> = self.sum.iter().map(|s| s / n).collect();
        let std = self
            .sumsq
            .iter()
            .zip(&mean)
            .map(|(q, m)| (q / n - m * m).max(0.0).sqrt())
            .collect();
        (mean, std)
    }
}

/// Moments of every `stride`-th row.
pub fn moments(vals: &[f64], width: usize, stride: usize) -> Moments {
    let (mut sum, mut sumsq) = (vec![0.0; width], vec![0.0; width]);
    let mut n = 0;
    for row in vals.chunks_exact(width).step_by(stride) {
        n += 1;
        // Zipped slices rather than indices: no bounds checks, so the loop vectorizes.
        for ((s, q), &x) in sum.iter_mut().zip(&mut sumsq).zip(row) {
            *s += x;
            *q += x * x;
        }
    }
    Moments { n, sum, sumsq }
}

/// Moments of the frame-to-frame differences of one episode, and the smallest and largest step per dimension.
#[derive(Clone, Debug)]
pub struct StepMoments {
    pub steps: Moments,
    pub min: Vec<f64>,
    pub max: Vec<f64>,
}

/// `None` under 3 frames, where steps say nothing.
pub fn step_moments(vals: &[f64], width: usize) -> Option<StepMoments> {
    if vals.len() < 3 * width {
        return None;
    }
    let (mut sum, mut sumsq) = (vec![0.0; width], vec![0.0; width]);
    let (mut min, mut max) = (vec![f64::INFINITY; width], vec![f64::NEG_INFINITY; width]);
    for (a, b) in vals
        .chunks_exact(width)
        .zip(vals.chunks_exact(width).skip(1))
    {
        let steps = a.iter().zip(b).map(|(x, y)| y - x);
        for ((((s, q), lo), hi), d) in sum
            .iter_mut()
            .zip(&mut sumsq)
            .zip(&mut min)
            .zip(&mut max)
            .zip(steps)
        {
            *s += d;
            *q += d * d;
            if d < *lo {
                *lo = d;
            }
            if d > *hi {
                *hi = d;
            }
        }
    }
    Some(StepMoments {
        steps: Moments {
            n: (vals.len() / width - 1) as u64,
            sum,
            sumsq,
        },
        min,
        max,
    })
}

/// The usual step per dimension: its mean and standard deviation (1 when it never varies).
pub struct StepScale {
    pub(super) mean: Vec<f64>,
    pub(super) std: Vec<f64>,
}

impl StepScale {
    pub(super) fn new(steps: &Moments) -> Self {
        let (mean, std) = steps.mean_std();
        // A step is measured from the mean step, not from zero: a constant-velocity action has a step deviation
        // of float noise, and judged from zero every ordinary step would be a jump.
        let std = std
            .into_iter()
            .map(|s| if s > 0.0 { s } else { 1.0 })
            .collect();
        Self { mean, std }
    }

    pub(super) fn z(&self, j: usize, step: f64) -> f64 {
        (step - self.mean[j]).abs() / self.std[j]
    }
}

/// Steps whose mean deviation over the dimensions, in step standard deviations, exceeds the limit.
pub fn count_jumps(vals: &[f64], width: usize, scale: &StepScale) -> usize {
    vals.chunks_exact(width)
        .zip(vals.chunks_exact(width).skip(1))
        .filter(|(a, b)| {
            let z: f64 = (0..width).map(|j| scale.z(j, b[j] - a[j])).sum();
            z / width as f64 > JUMP_SIGMAS
        })
        .count()
}

/// The first dimension and size of the largest shift of the mean from the first to the last quarter, in standard
/// deviations of the first quarter, when it exceeds the limit.
pub fn shift_finding(first: &Moments, last: &Moments) -> Option<(usize, f64)> {
    if first.n == 0 || last.n == 0 {
        return None;
    }
    let (m0, s0) = first.mean_std();
    let (m1, _) = last.mean_std();
    let (j, z) = (0..m0.len())
        .map(|j| {
            (
                j,
                (m1[j] - m0[j]).abs() / if s0[j] > 0.0 { s0[j] } else { 1.0 },
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    (z > SHIFT_SIGMAS).then_some((j, z))
}

/// Columns compared across the dataset: the action and the state.
pub(super) fn is_shift_key(key: &str, action: Option<&str>) -> bool {
    Some(key) == action || key.starts_with("action") || key.contains("state")
}

pub(super) fn column_rows(col: &(Vec<f64>, usize), s: usize, t: usize) -> Option<(&[f64], usize)> {
    let (vals, w) = col;
    vals.get(s * w..t * w).map(|v| (v, *w))
}

/// What one episode contributes to the jump and shift checks.
pub(super) struct EpisodeNumbers {
    pub(super) pos: usize,
    pub(super) moments: Vec<(String, Moments)>,
    pub(super) steps: Option<StepMoments>,
}

/// A warning for each of the first episodes with action jumps, found by reading their file again: the first pass
/// only knows each episode's largest step, enough to rule episodes out, not to count.
pub(super) fn jump_issues(
    ds: &Dataset,
    action: Option<&str>,
    extras: &[GroupExtra],
) -> Vec<Tagged> {
    let Some(key) = action else {
        return vec![];
    };
    let mut all = Moments::default();
    for e in extras.iter().flat_map(|g| &g.episodes) {
        if let Some(s) = &e.steps {
            all.add(&s.steps);
        }
    }
    if all.n == 0 {
        return vec![];
    }
    let scale = StepScale::new(&all);
    let mut by_file: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
    for e in extras.iter().flat_map(|g| &g.episodes) {
        // Each step's mean deviation is at most the mean over dimensions of the episode's largest deviation, so
        // an episode below the limit on that bound cannot hold a jump.
        let big = e.steps.as_ref().is_some_and(|s| {
            let sum: f64 = (0..s.min.len())
                .map(|j| scale.z(j, s.min[j]).max(scale.z(j, s.max[j])))
                .sum();
            sum / s.min.len() as f64 > JUMP_SIGMAS
        });
        if big {
            by_file
                .entry(&ds.episodes[e.pos].data_file)
                .or_default()
                .push(e.pos);
        }
    }
    let mut found: Vec<(usize, usize)> = by_file
        .into_par_iter()
        .flat_map(|(file, positions)| {
            let Ok(bytes) = std::fs::read(file) else {
                return vec![];
            };
            let Ok(d) = parse_file(bytes.into(), ds) else {
                return vec![];
            };
            let (runs, Some((vals, w))) = (episode_runs(&d), d.floats.get(key)) else {
                return vec![];
            };
            positions
                .into_iter()
                .filter_map(|pos| {
                    let (s, t) = episode_rows(&d, &runs, ds.episodes[pos].index)?;
                    let n = count_jumps(vals.get(s * w..t * w)?, *w, &scale);
                    (n > 0).then_some((pos, n))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    found.sort_unstable();
    let more = found.len().saturating_sub(MAX_JUMP_EPISODES);
    let mut out: Vec<Tagged> = found
        .iter()
        .take(MAX_JUMP_EPISODES)
        .map(|&(pos, n)| {
            tag(
                Some(pos),
                PHASE_DATA,
                Severity::Warning,
                Some(ds.episodes[pos].index),
                "action_jump",
                format!(
                    "{n} frame-to-frame jump{} in `{key}`, each over {JUMP_SIGMAS} standard deviations of the usual step",
                    if n == 1 { "" } else { "s" }
                ),
            )
        })
        .collect();
    if more > 0 {
        out.push(tag(
            None,
            PHASE_DATA,
            Severity::Warning,
            None,
            "action_jump",
            format!("and {more} more episodes with action jumps"),
        ));
    }
    out
}

/// Whether an episode is in the first or the last quarter, where the shift check needs its numbers. Datasets under 10
/// episodes have none. Quarters count positions, so an episode without rows only thins its quarter.
pub(super) fn in_quarter(pos: usize, total: usize) -> bool {
    total >= SHIFT_MIN_EPISODES && (pos < total / 4 || pos >= total - total / 4)
}

/// A warning for each action or state column whose mean differs between the first and the last quarter of the
/// episodes, from 10 episodes up.
pub(super) fn shift_issues(ds: &Dataset, extras: &[GroupExtra]) -> Vec<Tagged> {
    let total = ds.episodes.len();
    let mut quarters: BTreeMap<&str, (Moments, Moments)> = BTreeMap::new();
    for e in extras.iter().flat_map(|g| &g.episodes) {
        for (key, m) in &e.moments {
            let (first, last) = quarters.entry(key).or_default();
            if e.pos < total / 4 {
                first.add(m);
            } else {
                last.add(m);
            }
        }
    }
    quarters
        .iter()
        .filter_map(|(key, (first, last))| {
            let (j, z) = shift_finding(first, last)?;
            Some(tag(
                None,
                PHASE_DATA,
                Severity::Warning,
                None,
                "distribution_shift",
                format!(
                    "{key}: dimension {j} of the last quarter of the episodes is {z:.1} standard deviations from the first quarter"
                ),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moments_and_steps_of_a_small_series() {
        let m = moments(&[1.0, 10.0, 3.0, 10.0], 2, 1);
        assert_eq!(
            (m.n, m.sum, m.sumsq),
            (2, vec![4.0, 20.0], vec![10.0, 200.0])
        );
        let st = step_moments(&[0.0, 1.0, 3.0, 6.0], 1).unwrap();
        assert_eq!((st.steps.n, st.steps.sum), (3, vec![6.0]));
        assert_eq!((st.min, st.max), (vec![1.0], vec![3.0]));
        assert!(step_moments(&[0.0, 1.0], 1).is_none());
    }

    #[test]
    fn moments_can_sample_every_nth_row() {
        let m = moments(&[1.0, 2.0, 3.0, 4.0, 5.0], 1, 2);
        assert_eq!((m.n, m.sum), (3, vec![9.0]));
    }

    #[test]
    fn a_jump_is_judged_by_the_mean_over_dimensions_in_step_sigmas() {
        let scale = StepScale {
            mean: vec![0.0, 0.0],
            std: vec![1.0, 1.0],
        };
        let row = |a: f64, b: f64| [a, b];
        let calm: Vec<f64> = [row(0.0, 0.0), row(1.0, 1.0), row(2.0, 2.0)].concat();
        assert_eq!(count_jumps(&calm, 2, &scale), 0);
        // One dimension at 20 and one at 0 averages to 10 > 8; at 10 and 0 it averages to 5.
        let big: Vec<f64> = [row(0.0, 0.0), row(20.0, 0.0), row(20.0, 0.0)].concat();
        assert_eq!(count_jumps(&big, 2, &scale), 1);
        let mild: Vec<f64> = [row(0.0, 0.0), row(10.0, 0.0), row(10.0, 0.0)].concat();
        assert_eq!(count_jumps(&mild, 2, &scale), 0);
    }

    #[test]
    fn shift_needs_more_than_three_first_quarter_sigmas() {
        let quarter = |vals: &[f64]| moments(vals, 1, 1);
        let first = quarter(&[0.0, 2.0, 0.0, 2.0]); // mean 1, std 1
        assert_eq!(shift_finding(&first, &quarter(&[4.0, 4.0])), None);
        let (dim, z) = shift_finding(&first, &quarter(&[5.0, 5.0])).unwrap();
        assert_eq!(dim, 0);
        assert!((z - 4.0).abs() < 1e-9);
    }

    #[test]
    fn shift_judges_a_constant_first_quarter_in_raw_units() {
        let first = moments(&[1.0, 1.0, 1.0], 1, 1);
        assert!(shift_finding(&first, &moments(&[5.0], 1, 1)).is_some());
        assert!(shift_finding(&first, &moments(&[2.0], 1, 1)).is_none());
    }
}
