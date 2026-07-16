//! Output formatters for a [`LintOutcome`]: human-readable terminal output,
//! machine JSON, GitHub Actions workflow commands, and GitLab Code Quality.

use sha2::{Digest, Sha256};

use crate::engine::LintOutcome;
use crate::model::{Diagnostic, Severity};

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";

fn severity_color(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => RED,
        Severity::Warn => YELLOW,
        Severity::Info => BLUE,
        Severity::Allow => "",
    }
}

/// `3:5`, `3` (no column) or `-` (no position).
fn position(d: &Diagnostic) -> String {
    match (d.line, d.col) {
        (Some(l), Some(c)) => format!("{l}:{c}"),
        (Some(l), None) => l.to_string(),
        (None, _) => "-".to_string(),
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("{n} {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Human-readable output: findings grouped by file, then a summary line.
/// ANSI colors (error red, warn yellow, info blue, dim notes) when `use_color`.
pub fn pretty(outcome: &LintOutcome, use_color: bool) -> String {
    let paint = |code: &str, s: &str| {
        if use_color && !code.is_empty() {
            format!("{code}{s}{RESET}")
        } else {
            s.to_string()
        }
    };

    let mut out = String::new();
    let mut current_file: Option<&str> = None;
    for d in &outcome.diagnostics {
        if current_file != Some(d.file.as_str()) {
            if current_file.is_some() {
                out.push('\n');
            }
            out.push_str(&paint(BOLD, &d.file));
            out.push('\n');
            current_file = Some(&d.file);
        }
        let level = paint(severity_color(d.severity), &d.severity.to_string());
        out.push_str(&format!(
            "  {level}[{}] {}  {}\n",
            d.rule,
            position(d),
            d.message
        ));
        if let Some(note) = &d.note {
            out.push_str(&paint(DIM, &format!("      note: {note}")));
            out.push('\n');
        }
    }

    if !outcome.diagnostics.is_empty() {
        out.push('\n');
    }
    let mut parts: Vec<String> = Vec::new();
    if outcome.errors > 0 {
        parts.push(paint(RED, &plural(outcome.errors, "error")));
    }
    if outcome.warnings > 0 {
        parts.push(paint(YELLOW, &plural(outcome.warnings, "warning")));
    }
    if outcome.infos > 0 {
        parts.push(paint(BLUE, &plural(outcome.infos, "info")));
    }
    let summary = if parts.is_empty() {
        "no problems found".to_string()
    } else {
        parts.join(", ")
    };
    out.push_str(&format!(
        "{summary} · {} checked\n",
        plural(outcome.files_checked, "file")
    ));
    out
}

/// Machine JSON: `{"diagnostics": [...], "summary": {...}}`.
pub fn json(outcome: &LintOutcome) -> String {
    let value = serde_json::json!({
        "diagnostics": outcome.diagnostics,
        "summary": {
            "errors": outcome.errors,
            "warnings": outcome.warnings,
            "infos": outcome.infos,
            "files_checked": outcome.files_checked,
        },
    });
    serde_json::to_string_pretty(&value).expect("diagnostics serialize to JSON")
}

/// Escape workflow-command *data* (the message part): `%`, CR, LF.
fn gh_escape_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escape workflow-command *property* values (file=...): data escapes plus `:` and `,`.
fn gh_escape_property(s: &str) -> String {
    gh_escape_data(s).replace(':', "%3A").replace(',', "%2C")
}

/// GitHub Actions annotations, one workflow command per diagnostic:
/// `::error file=docs/{file},line={line}::{message} [{rule}]`.
pub fn github(outcome: &LintOutcome) -> String {
    let mut out = String::new();
    for d in &outcome.diagnostics {
        let level = match d.severity {
            Severity::Error => "error",
            Severity::Warn => "warning",
            _ => "notice",
        };
        let file = gh_escape_property(&format!("docs/{}", d.file));
        let line = d.line.map(|l| format!(",line={l}")).unwrap_or_default();
        let message = gh_escape_data(&format!("{} [{}]", d.message, d.rule));
        out.push_str(&format!("::{level} file={file}{line}::{message}\n"));
    }
    out
}

/// The stable Code Quality fingerprint: sha256 over rule + file + line + message.
fn gitlab_fingerprint(d: &Diagnostic) -> String {
    let mut hasher = Sha256::new();
    hasher.update(d.rule.as_bytes());
    hasher.update(b"\0");
    hasher.update(d.file.as_bytes());
    hasher.update(b"\0");
    hasher.update(d.line.map(|l| l.to_string()).unwrap_or_default().as_bytes());
    hasher.update(b"\0");
    hasher.update(d.message.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// GitLab Code Quality report: a JSON array of issue objects.
pub fn gitlab(outcome: &LintOutcome) -> String {
    let issues: Vec<serde_json::Value> = outcome
        .diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "description": format!("[{}] {}", d.rule, d.message),
                "check_name": d.rule,
                "fingerprint": gitlab_fingerprint(d),
                "severity": match d.severity {
                    Severity::Error => "major",
                    Severity::Warn => "minor",
                    _ => "info",
                },
                "location": {
                    "path": format!("docs/{}", d.file),
                    "lines": { "begin": d.line.unwrap_or(1) },
                },
            })
        })
        .collect();
    serde_json::to_string_pretty(&issues).expect("issues serialize to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small fixed outcome: two positioned findings (one with a note, one
    /// with an escape-worthy message) and one without a line.
    fn outcome() -> LintOutcome {
        LintOutcome {
            diagnostics: vec![
                Diagnostic {
                    rule: "broken-wikilink",
                    severity: Severity::Error,
                    file: "guide/intro.md".into(),
                    line: Some(3),
                    col: Some(5),
                    message: "unresolved wikilink `[[ghost]]`".into(),
                    note: Some("did you mean `guide/ghost`?".into()),
                },
                Diagnostic {
                    rule: "heading-order",
                    severity: Severity::Info,
                    file: "guide/intro.md".into(),
                    line: Some(9),
                    col: None,
                    message: "50% skipped\nlevel".into(),
                    note: None,
                },
                Diagnostic {
                    rule: "orphan-page",
                    severity: Severity::Warn,
                    file: "lost.md".into(),
                    line: None,
                    col: None,
                    message: "page has no inbound links".into(),
                    note: None,
                },
            ],
            files_checked: 4,
            errors: 1,
            warnings: 1,
            infos: 1,
        }
    }

    #[test]
    fn pretty_groups_by_file_with_summary() {
        let s = pretty(&outcome(), false);
        // One header per file, findings indented beneath.
        assert_eq!(s.matches("guide/intro.md\n").count(), 1);
        assert!(s.contains("  error[broken-wikilink] 3:5  unresolved wikilink `[[ghost]]`"));
        assert!(s.contains("      note: did you mean `guide/ghost`?"));
        assert!(s.contains("  info[heading-order] 9  50%")); // line-only position
        assert!(s.contains("lost.md\n  warn[orphan-page] -  page has no inbound links"));
        assert!(s.contains("1 error, 1 warning, 1 info · 4 files checked"));
        // No ANSI escapes without color.
        assert!(!s.contains('\x1b'));
    }

    #[test]
    fn pretty_uses_ansi_colors_when_asked() {
        let s = pretty(&outcome(), true);
        assert!(s.contains("\x1b[31merror\x1b[0m")); // error red
        assert!(s.contains("\x1b[33mwarn\x1b[0m")); // warn yellow
        assert!(s.contains("\x1b[34minfo\x1b[0m")); // info blue
        assert!(s.contains("\x1b[2m")); // dim note
    }

    #[test]
    fn pretty_empty_outcome_says_no_problems() {
        let empty = LintOutcome {
            diagnostics: vec![],
            files_checked: 7,
            errors: 0,
            warnings: 0,
            infos: 0,
        };
        assert_eq!(
            pretty(&empty, false),
            "no problems found · 7 files checked\n"
        );
    }

    #[test]
    fn json_round_trips_structure() {
        let parsed: serde_json::Value = serde_json::from_str(&json(&outcome())).unwrap();
        let diags = parsed["diagnostics"].as_array().unwrap();
        assert_eq!(diags.len(), 3);
        assert_eq!(diags[0]["rule"], "broken-wikilink");
        assert_eq!(diags[0]["severity"], "error");
        assert_eq!(diags[0]["line"], 3);
        assert_eq!(diags[0]["col"], 5);
        assert_eq!(diags[0]["note"], "did you mean `guide/ghost`?");
        assert!(diags[2]["line"].is_null()); // no-line diagnostic
        assert!(diags[1]["note"].is_null());
        assert_eq!(parsed["summary"]["errors"], 1);
        assert_eq!(parsed["summary"]["warnings"], 1);
        assert_eq!(parsed["summary"]["infos"], 1);
        assert_eq!(parsed["summary"]["files_checked"], 4);
    }

    #[test]
    fn github_emits_escaped_workflow_commands() {
        let s = github(&outcome());
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0],
            "::error file=docs/guide/intro.md,line=3::unresolved wikilink `[[ghost]]` [broken-wikilink]"
        );
        // % and \n escaped in the message data.
        assert_eq!(
            lines[1],
            "::notice file=docs/guide/intro.md,line=9::50%25 skipped%0Alevel [heading-order]"
        );
        // No `,line=` when the diagnostic has no line; Warn maps to warning.
        assert_eq!(
            lines[2],
            "::warning file=docs/lost.md::page has no inbound links [orphan-page]"
        );
    }

    #[test]
    fn gitlab_report_maps_severities_and_locations() {
        let parsed: serde_json::Value = serde_json::from_str(&gitlab(&outcome())).unwrap();
        let issues = parsed.as_array().unwrap();
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0]["severity"], "major");
        assert_eq!(issues[1]["severity"], "info");
        assert_eq!(issues[2]["severity"], "minor");
        assert_eq!(issues[0]["check_name"], "broken-wikilink");
        assert_eq!(
            issues[0]["description"],
            "[broken-wikilink] unresolved wikilink `[[ghost]]`"
        );
        assert_eq!(issues[0]["location"]["path"], "docs/guide/intro.md");
        assert_eq!(issues[0]["location"]["lines"]["begin"], 3);
        // A diagnostic without a line anchors at line 1.
        assert_eq!(issues[2]["location"]["lines"]["begin"], 1);
    }

    #[test]
    fn gitlab_fingerprints_are_stable_hex_and_distinct() {
        let a = gitlab(&outcome());
        let b = gitlab(&outcome());
        assert_eq!(a, b, "fingerprints must be deterministic");
        let parsed: serde_json::Value = serde_json::from_str(&a).unwrap();
        let fps: Vec<String> = parsed
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["fingerprint"].as_str().unwrap().to_string())
            .collect();
        for fp in &fps {
            assert_eq!(fp.len(), 64);
            assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert_ne!(fps[0], fps[1]);
        assert_ne!(fps[1], fps[2]);
    }
}
