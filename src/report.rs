//! Render validation results as text, full text, JSON or GitHub workflow commands.
use crate::validate::{Issue, Severity, explain};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    /// Text plus where each issue is and what its code means.
    Full,
    Json,
    Github,
}

pub struct Options {
    pub format: Format,
    pub color: bool,
    pub statistics: bool,
    pub quiet: bool,
    /// The video frames were decoded (`--decode`), so the output says so.
    pub decoded: bool,
}

/// The outcome of validating one dataset.
#[derive(Debug, Clone)]
pub struct DatasetReport {
    pub path: PathBuf,
    pub issues: Vec<Issue>,
    /// Findings dropped by the config, per issue code.
    pub ignored: BTreeMap<String, usize>,
    /// Set when the dataset could not be opened at all.
    pub open_error: Option<String>,
}

impl DatasetReport {
    pub fn new(path: PathBuf, issues: Vec<Issue>) -> Self {
        DatasetReport {
            path,
            issues,
            ignored: BTreeMap::new(),
            open_error: None,
        }
    }

    fn counts(&self) -> (usize, usize) {
        let errors = self
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
            + usize::from(self.open_error.is_some());
        let warnings = self
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count();
        (errors, warnings)
    }
}

/// 0 when no errors, 1 when some dataset has errors, 2 when a dataset could not be opened.
pub fn exit_code(reports: &[DatasetReport]) -> u8 {
    if reports.iter().any(|r| r.open_error.is_some()) {
        2
    } else if reports.iter().any(|r| r.counts().0 > 0) {
        1
    } else {
        0
    }
}

fn totals(reports: &[DatasetReport]) -> (usize, usize, usize) {
    let (mut e, mut w, mut d) = (0, 0, 0);
    for r in reports {
        let (re, rw) = r.counts();
        e += re;
        w += rw;
        d += usize::from(re + rw > 0);
    }
    (e, w, d)
}

fn ignored_totals(reports: &[DatasetReport]) -> BTreeMap<&str, usize> {
    let mut m = BTreeMap::new();
    for r in reports {
        for (k, n) in &r.ignored {
            *m.entry(k.as_str()).or_insert(0) += n;
        }
    }
    m
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn render(reports: &[DatasetReport], o: &Options) -> String {
    match o.format {
        Format::Json => json(reports),
        Format::Github => github(reports, o),
        Format::Text | Format::Full => text(reports, o),
    }
}

fn paint(on: bool, code: &str, s: &str) -> String {
    if on {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

fn summary(reports: &[DatasetReport], o: &Options) -> String {
    let (e, w, d) = totals(reports);
    let mut s = if e + w == 0 {
        if o.quiet {
            String::new()
        } else {
            paint(o.color, "32", "All checks passed!") + "\n"
        }
    } else {
        let line = format!(
            "Found {} and {} in {}.",
            plural(e, "error", "errors"),
            plural(w, "warning", "warnings"),
            plural(d, "dataset", "datasets")
        );
        paint(o.color, if e > 0 { "31" } else { "33" }, &line) + "\n"
    };
    let ignored = ignored_totals(reports);
    if !ignored.is_empty() {
        let list: Vec<String> = ignored.iter().map(|(k, n)| format!("{k} ({n})")).collect();
        s += &format!("Ignored by config: {}\n", list.join(", "));
    }
    if o.decoded && !o.quiet {
        s += "Video frames were decoded (--decode).\n";
    }
    s
}

fn statistics(reports: &[DatasetReport]) -> String {
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for i in reports.iter().flat_map(|r| &r.issues) {
        *count.entry(i.code).or_insert(0) += 1;
    }
    let mut rows: Vec<_> = count.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    rows.iter().map(|(c, n)| format!("{n:>5}  {c}\n")).collect()
}

fn text(reports: &[DatasetReport], o: &Options) -> String {
    let mut s = String::new();
    if !o.quiet {
        let multi = reports.len() > 1;
        for r in reports {
            let prefix = if multi {
                format!("{}: ", r.path.display())
            } else {
                String::new()
            };
            if let Some(err) = &r.open_error {
                let sev = paint(o.color, "31", "ERROR");
                s += &format!(
                    "{}: {sev}   ep -      {:<24} {err}\n",
                    r.path.display(),
                    "cannot_open"
                );
                if o.format == Format::Full {
                    s += &format!("        --> {}\n", r.path.display());
                    s += &format!("        help: {}\n", explain("cannot_open"));
                }
            }
            for x in &r.issues {
                let ep = x.episode.map_or("-".into(), |e| e.to_string());
                let (label, color) = match x.severity {
                    Severity::Error => ("ERROR", "31"),
                    Severity::Warning => ("WARNING", "33"),
                };
                let pad = " ".repeat(7 - label.len());
                s += &format!(
                    "{prefix}{}{pad} ep {ep:<6} {:<24} {}\n",
                    paint(o.color, color, label),
                    x.code,
                    x.message
                );
                if o.format == Format::Full {
                    let at = x
                        .episode
                        .map_or(String::new(), |e| format!(", episode {e}"));
                    s += &format!("        --> {}{at}\n", r.path.display());
                    s += &format!("        help: {}\n", explain(x.code));
                }
            }
        }
    }
    if o.statistics {
        s += &statistics(reports);
    }
    s + &summary(reports, o)
}

#[derive(Serialize)]
struct JsonIssue<'a> {
    dataset: String,
    severity: Severity,
    episode: Option<usize>,
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    errors: usize,
    warnings: usize,
    ignored: BTreeMap<&'a str, usize>,
    issues: Vec<JsonIssue<'a>>,
}

fn json(reports: &[DatasetReport]) -> String {
    let (errors, warnings, _) = totals(reports);
    let mut issues = Vec::new();
    for r in reports {
        if let Some(err) = &r.open_error {
            issues.push(JsonIssue {
                dataset: r.path.display().to_string(),
                severity: Severity::Error,
                episode: None,
                code: "cannot_open",
                message: err,
            });
        }
        for x in &r.issues {
            issues.push(JsonIssue {
                dataset: r.path.display().to_string(),
                severity: x.severity,
                episode: x.episode,
                code: x.code,
                message: &x.message,
            });
        }
    }
    let mut s = serde_json::to_string_pretty(&JsonReport {
        errors,
        warnings,
        ignored: ignored_totals(reports),
        issues,
    })
    .unwrap_or_default();
    s.push('\n');
    s
}

/// Workflow commands are line-based, so `%`, CR and LF in the message must be escaped.
fn escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn github(reports: &[DatasetReport], o: &Options) -> String {
    let multi = reports.len() > 1;
    let mut s = String::new();
    for r in reports {
        let dataset = if multi {
            format!("{}: ", r.path.display())
        } else {
            String::new()
        };
        if let Some(err) = &r.open_error {
            s += &format!(
                "::error title=cannot_open::{}\n",
                escape(&format!("{}: {err}", r.path.display()))
            );
        }
        for x in &r.issues {
            let level = match x.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            let msg = match x.episode {
                Some(e) => format!("{dataset}episode {e}: {}", x.message),
                None => format!("{dataset}{}", x.message),
            };
            s += &format!("::{level} title={}::{}\n", x.code, escape(&msg));
        }
    }
    s + &summary(reports, &Options { color: false, ..*o })
}
