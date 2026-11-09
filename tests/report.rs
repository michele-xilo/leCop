use lecop::{
    report::{Format, render},
    validate::{Issue, Severity},
};

fn issue(severity: Severity, episode: Option<usize>, code: &'static str, message: &str) -> Issue {
    Issue {
        severity,
        episode,
        code,
        message: message.into(),
    }
}

fn sample() -> Vec<Issue> {
    vec![
        issue(Severity::Error, None, "frame_count", "info.json says 5"),
        issue(Severity::Warning, Some(3), "idle_episode", "60% idle"),
    ]
}

#[test]
fn text_format_keeps_the_existing_lines_and_summary() {
    let out = render(&sample(), Format::Text);
    assert_eq!(
        out,
        "ERROR   ep -      frame_count              info.json says 5\n\
         WARNING ep 3      idle_episode             60% idle\n\
         1 errors, 1 warnings\n"
    );
}

#[test]
fn json_format_has_stable_fields_and_counts() {
    let out = render(&sample(), Format::Json);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["errors"], 1);
    assert_eq!(v["warnings"], 1);
    assert_eq!(
        v["issues"][0],
        serde_json::json!({"severity":"error","episode":null,"code":"frame_count","message":"info.json says 5"})
    );
    assert_eq!(v["issues"][1]["severity"], "warning");
    assert_eq!(v["issues"][1]["episode"], 3);
}

#[test]
fn json_of_a_healthy_dataset_is_an_empty_list() {
    let v: serde_json::Value = serde_json::from_str(&render(&[], Format::Json)).unwrap();
    assert_eq!(v["issues"], serde_json::json!([]));
    assert_eq!(v["errors"], 0);
}

#[test]
fn github_format_emits_workflow_commands() {
    let out = render(&sample(), Format::Github);
    assert_eq!(
        out,
        "::error title=frame_count::info.json says 5\n\
         ::warning title=idle_episode::episode 3: 60%25 idle\n\
         1 errors, 1 warnings\n"
    );
}

#[test]
fn github_format_escapes_newlines_and_percent() {
    let out = render(
        &[issue(
            Severity::Error,
            None,
            "data_unreadable",
            "100%\nbad\r",
        )],
        Format::Github,
    );
    assert!(
        out.starts_with("::error title=data_unreadable::100%25%0Abad%0D\n"),
        "{out}"
    );
}
