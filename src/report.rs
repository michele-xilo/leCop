//! Render a list of issues as text, JSON or GitHub workflow commands.
use crate::validate::{Issue, Severity};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Github,
}

#[derive(Serialize)]
struct Report<'a> {
    errors: usize,
    warnings: usize,
    issues: &'a [Issue],
}

/// Number of (errors, warnings).
pub fn counts(issues: &[Issue]) -> (usize, usize) {
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    (errors, issues.len() - errors)
}

pub fn render(issues: &[Issue], format: Format) -> String {
    let (errors, warnings) = counts(issues);
    match format {
        Format::Json => {
            let mut s = serde_json::to_string_pretty(&Report {
                errors,
                warnings,
                issues,
            })
            .unwrap_or_default();
            s.push('\n');
            s
        }
        Format::Text => {
            let mut s = String::new();
            for x in issues {
                let ep = x.episode.map_or("-".into(), |e| e.to_string());
                let sev = match x.severity {
                    Severity::Error => "ERROR",
                    Severity::Warning => "WARNING",
                };
                s += &format!("{sev:<7} ep {ep:<6} {:<24} {}\n", x.code, x.message);
            }
            s + &format!("{errors} errors, {warnings} warnings\n")
        }
        Format::Github => {
            let mut s = String::new();
            for x in issues {
                let level = match x.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                let msg = match x.episode {
                    Some(e) => format!("episode {e}: {}", x.message),
                    None => x.message.clone(),
                };
                s += &format!("::{level} title={}::{}\n", x.code, escape(&msg));
            }
            s + &format!("{errors} errors, {warnings} warnings\n")
        }
    }
}

/// Workflow commands are line-based, so `%`, CR and LF in the message must be escaped.
fn escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}
