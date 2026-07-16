use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Recursively copy the checked-in `fixtures/site-lint` into a fresh temp dir
/// (existing tests copy fixtures file-by-file; this fixture is deep enough to
/// warrant a helper).
fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &to);
        } else {
            fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// Copy `fixtures/site-lint` into a temp dir named after the calling test.
fn lint_fixture_copy(tag: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen
    let workspace = manifest.parent().unwrap().parent().unwrap(); // repo root
    let fixture = workspace.join("fixtures/site-lint");
    let tmp = std::env::temp_dir().join(format!("docgen_lint_cli_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_dir(&fixture, &tmp);
    tmp
}

/// Run `docgen lint <args...> <root>` and capture everything.
fn lint(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("lint")
        .args(args)
        .arg(root)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn default_pretty_run_reports_findings_and_exits_1() {
    let tmp = lint_fixture_copy("default");
    let out = lint(&tmp, &[]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);

    // Expected rules across all severities.
    for rule in [
        "error[broken-wikilink]",
        "error[missing-asset]",
        "error[unknown-component]",
        "error[invalid-frontmatter]",
        "error[duplicate-slug]",
        "error[plantuml-src-missing]",
        "warn[broken-anchor]",
        "warn[mermaid-empty]",
        "warn[mermaid-unknown-type]",
        "warn[empty-page]",
        "warn[missing-title]",
        "info[orphan-page]",
        "info[unused-partial]",
    ] {
        assert!(text.contains(rule), "missing `{rule}` in:\n{text}");
    }

    // The re-leveled rule appears at its configured severity (info -> warn).
    assert!(text.contains("warn[heading-level-jump]"), "{text}");
    assert!(!text.contains("info[heading-level-jump]"), "{text}");

    // The `drafts/**` ignore glob keeps the broken draft out entirely.
    assert!(!text.contains("drafts/"), "{text}");
    assert!(!text.contains("nowhere-at-all"), "{text}");

    // Summary line with true counts; piped output carries no ANSI colors.
    assert!(
        text.contains("6 errors, 7 warnings, 2 infos · 10 files checked"),
        "{text}"
    );
    assert!(!text.contains('\x1b'), "{text}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn json_format_matches_summary_and_sorts_files() {
    let tmp = lint_fixture_copy("json");
    let out = lint(&tmp, &["--format", "json"]);
    assert_eq!(out.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();

    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());

    // Summary counts match the diagnostics array.
    let count = |sev: &str| diags.iter().filter(|d| d["severity"] == sev).count() as u64;
    assert_eq!(parsed["summary"]["errors"].as_u64(), Some(count("error")));
    assert_eq!(parsed["summary"]["warnings"].as_u64(), Some(count("warn")));
    assert_eq!(parsed["summary"]["infos"].as_u64(), Some(count("info")));
    assert_eq!(parsed["summary"]["files_checked"].as_u64(), Some(10));

    // Expected rules present.
    let rules: Vec<&str> = diags.iter().map(|d| d["rule"].as_str().unwrap()).collect();
    for rule in ["broken-wikilink", "duplicate-slug", "orphan-page"] {
        assert!(rules.contains(&rule), "missing {rule} in {rules:?}");
    }

    // Diagnostics sorted by file.
    let files: Vec<&str> = diags.iter().map(|d| d["file"].as_str().unwrap()).collect();
    let mut sorted = files.clone();
    sorted.sort();
    assert_eq!(files, sorted);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn github_format_emits_workflow_commands() {
    let tmp = lint_fixture_copy("github");
    let out = lint(&tmp, &["--format", "github"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(!lines.is_empty());
    for line in &lines {
        assert!(
            line.starts_with("::error ")
                || line.starts_with("::warning ")
                || line.starts_with("::notice "),
            "unexpected line: {line}"
        );
    }
    assert!(text.contains("[broken-wikilink]"), "{text}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn gitlab_format_emits_code_quality_issues() {
    let tmp = lint_fixture_copy("gitlab");
    let out = lint(&tmp, &["--format", "gitlab"]);
    assert_eq!(out.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let issues = parsed.as_array().unwrap();
    assert!(!issues.is_empty());
    for issue in issues {
        assert!(issue["check_name"].is_string(), "{issue}");
        let fp = issue["fingerprint"].as_str().unwrap();
        assert_eq!(fp.len(), 64);
        assert!(issue["location"]["path"]
            .as_str()
            .unwrap()
            .starts_with("docs/"));
        assert!(issue["location"]["lines"]["begin"].is_u64());
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn rules_flag_selects_a_single_rule() {
    let tmp = lint_fixture_copy("rules_single");
    let out = lint(&tmp, &["--rules", "broken-wikilink", "--format", "json"]);
    assert_eq!(out.status.code(), Some(1)); // it's an error-level rule
    let parsed: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let diags = parsed["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty());
    assert!(
        diags.iter().all(|d| d["rule"] == "broken-wikilink"),
        "{diags:?}"
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn unknown_rule_id_exits_2_and_names_it_on_stderr() {
    let tmp = lint_fixture_copy("rules_unknown");
    let out = lint(&tmp, &["--rules", "no-such-rule"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.contains("no-such-rule"), "{err}");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn deny_warnings_flips_a_warnings_only_run_to_exit_1() {
    let tmp = lint_fixture_copy("deny_warnings");
    // missing-title alone yields warnings only -> exit 0.
    let out = lint(&tmp, &["--rules", "missing-title"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).contains("warn[missing-title]"));

    // --deny-warnings promotes them -> exit 1, shown as errors.
    let out = lint(&tmp, &["--rules", "missing-title", "--deny-warnings"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("error[missing-title]"),
        "{}",
        stdout(&out)
    );

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn list_rules_prints_all_23_and_exits_0() {
    let out = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("lint")
        .arg("--list-rules")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert_eq!(text.lines().count(), 23);
    for id in [
        "broken-wikilink",
        "duplicate-slug",
        "orphan-page",
        "unused-partial",
        "external-url",
    ] {
        assert!(
            text.lines().any(|l| l.starts_with(id)),
            "missing {id} in:\n{text}"
        );
    }
}

#[test]
fn clean_site_exits_0_with_no_problems() {
    // site-basic carries deliberate error-level findings (broken wikilinks, a
    // bogus directive) for the build tests, so a truly-clean minimal site is
    // built here instead.
    let tmp = std::env::temp_dir().join(format!("docgen_lint_cli_clean_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::write(tmp.join("docs/index.md"), "# Home\n\nSee [[a]].\n").unwrap();
    fs::write(tmp.join("docs/a.md"), "# A\n\nBack to [[index]].\n").unwrap();

    let out = lint(&tmp, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).contains("no problems found · 2 files checked"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn quiet_hides_warn_and_info_but_keeps_true_summary_counts() {
    let tmp = lint_fixture_copy("quiet");
    let out = lint(&tmp, &["--quiet"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);

    // Error findings still shown; warn/info findings hidden.
    assert!(text.contains("error[broken-wikilink]"), "{text}");
    assert!(!text.contains("warn["), "{text}");
    assert!(!text.contains("info["), "{text}");

    // The summary line keeps the TRUE counts.
    assert!(
        text.contains("6 errors, 7 warnings, 2 infos · 10 files checked"),
        "{text}"
    );

    let _ = fs::remove_dir_all(&tmp);
}
