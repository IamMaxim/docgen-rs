//! The lint engine: load config, validate `[lint]` settings, build the
//! [`LintContext`], run every (selected) rule at its resolved severity, apply
//! per-file frontmatter suppression, and produce a sorted [`LintOutcome`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use globset::{Glob, GlobMatcher};

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::{all_rules, Rule};

/// Caller-facing knobs for one lint run.
#[derive(Debug, Clone, Default)]
pub struct LintOptions {
    /// Run only these rule ids (an unknown name is [`LintError::UnknownRule`]).
    pub only_rules: Option<Vec<String>>,
    /// Promote every remaining warning to an error (after suppression).
    pub deny_warnings: bool,
}

/// The result of a lint run: sorted diagnostics plus summary counts.
#[derive(Debug, Clone)]
pub struct LintOutcome {
    pub diagnostics: Vec<Diagnostic>,
    pub files_checked: usize,
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

/// Operational failures of the lint run itself (CLI exit 2 territory) — as
/// opposed to lint findings, which are diagnostics in the outcome.
#[derive(Debug, thiserror::Error)]
pub enum LintError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Config(#[from] docgen_config::ConfigError),
    #[error("unknown lint rule `{name}`")]
    UnknownRule { name: String },
    #[error("invalid severity `{value}` for rule `{rule}` (expected allow|info|warn|error)")]
    BadSeverity { rule: String, value: String },
    #[error("invalid [lint] ignore glob `{glob}`: {source}")]
    BadIgnoreGlob {
        glob: String,
        #[source]
        source: globset::Error,
    },
}

/// Lint the project at `project_root` with every built-in rule.
pub fn run(project_root: &Path, options: &LintOptions) -> Result<LintOutcome, LintError> {
    run_with_rules(project_root, options, &all_rules())
}

/// `(id, default severity, description)` for every built-in rule — for `--list-rules`.
pub fn list_rules() -> Vec<(&'static str, Severity, &'static str)> {
    all_rules()
        .iter()
        .map(|r| (r.id(), r.default_severity(), r.description()))
        .collect()
}

/// The engine proper, parameterized over the rule set so tests can inject a
/// fake rule. [`run`] passes [`all_rules`].
pub(crate) fn run_with_rules(
    project_root: &Path,
    options: &LintOptions,
    rules: &[Box<dyn Rule>],
) -> Result<LintOutcome, LintError> {
    let config = docgen_config::load(project_root)?;
    let known: BTreeSet<&str> = rules.iter().map(|r| r.id()).collect();

    // Validate `[lint.rules]` up front: unknown ids and bad severity strings
    // are operational errors, not silently-dropped config.
    let mut overrides: BTreeMap<String, Severity> = BTreeMap::new();
    for (name, value) in &config.lint.rules {
        if !known.contains(name.as_str()) {
            return Err(LintError::UnknownRule { name: name.clone() });
        }
        let sev: Severity = value.parse().map_err(|_| LintError::BadSeverity {
            rule: name.clone(),
            value: value.clone(),
        })?;
        overrides.insert(name.clone(), sev);
    }
    if let Some(only) = &options.only_rules {
        for name in only {
            if !known.contains(name.as_str()) {
                return Err(LintError::UnknownRule { name: name.clone() });
            }
        }
    }

    // Compile `[lint] ignore` globs (matched against docs-relative paths).
    let ignore: Vec<GlobMatcher> = config
        .lint
        .ignore
        .iter()
        .map(|g| {
            Glob::new(g)
                .map(|glob| glob.compile_matcher())
                .map_err(|source| LintError::BadIgnoreGlob {
                    glob: g.clone(),
                    source,
                })
        })
        .collect::<Result<_, _>>()?;

    let ctx = LintContext::build(project_root, config, &ignore)?;

    // Per-file suppression sets, keyed by docs-relative path. Frontmatter
    // suppression applies only to diagnostics attributed to that file.
    let suppression: BTreeMap<&str, &BTreeSet<String>> = ctx
        .docs
        .iter()
        .map(|d| (d.prepared.rel_path.as_str(), &d.suppressed))
        .collect();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for rule in rules {
        if let Some(only) = &options.only_rules {
            if !only.iter().any(|n| n == rule.id()) {
                continue;
            }
        }
        let severity = overrides
            .get(rule.id())
            .copied()
            .unwrap_or_else(|| rule.default_severity());
        if severity == Severity::Allow {
            // A rule the user EXPLICITLY asked for via `--rules` that resolves
            // to allow would otherwise no-op silently — surface why.
            if options
                .only_rules
                .as_ref()
                .is_some_and(|only| only.iter().any(|n| n == rule.id()))
            {
                diagnostics.push(Diagnostic {
                    rule: rule.id(),
                    severity: Severity::Info,
                    file: "docgen.toml".to_string(),
                    line: None,
                    col: None,
                    message: format!(
                        "rule `{}` is allow-level and did not run",
                        rule.id()
                    ),
                    note: Some(format!(
                        "enable it with `[lint.rules] \"{}\" = \"warn\"` (or \"info\"/\"error\") in docgen.toml",
                        rule.id()
                    )),
                });
            }
            continue;
        }
        let mut emitted = Vec::new();
        rule.check(&ctx, &mut emitted);
        for mut d in emitted {
            // Rules emit at their default level; the engine owns re-leveling.
            // A diagnostic a rule explicitly emitted at a DIFFERENT level (the
            // external rules' Info "check skipped" notices) keeps that level —
            // an unreachable-server notice must stay Info even when the rule
            // itself is configured to error.
            if d.severity == rule.default_severity() {
                d.severity = severity;
            }
            if suppression
                .get(d.file.as_str())
                .is_some_and(|s| s.contains(d.rule))
            {
                continue;
            }
            diagnostics.push(d);
        }
    }

    // Promotion runs AFTER suppression so a suppressed warning never becomes
    // an un-suppressible error.
    if options.deny_warnings {
        for d in &mut diagnostics {
            if d.severity == Severity::Warn {
                d.severity = Severity::Error;
            }
        }
    }

    diagnostics
        .sort_by(|a, b| (a.file.as_str(), a.line, a.rule).cmp(&(b.file.as_str(), b.line, b.rule)));

    let count = |sev: Severity| diagnostics.iter().filter(|d| d.severity == sev).count();
    Ok(LintOutcome {
        // Pages AND partials: both are linted (partials run the content rules),
        // so both count as checked files.
        files_checked: ctx.docs.len(),
        errors: count(Severity::Error),
        warnings: count(Severity::Warn),
        infos: count(Severity::Info),
        diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake rule: one Warn-level finding per doc, emitted in REVERSE doc
    /// order so the engine's sort is actually exercised.
    struct FakeRule;

    impl Rule for FakeRule {
        fn id(&self) -> &'static str {
            "fake-rule"
        }
        fn default_severity(&self) -> Severity {
            Severity::Warn
        }
        fn description(&self) -> &'static str {
            "emits one finding per doc (test-only)"
        }
        fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
            for d in ctx.docs.iter().rev() {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: d.prepared.rel_path.clone(),
                    line: Some(1),
                    col: Some(1),
                    message: format!("fake finding in {}", d.prepared.slug),
                    note: None,
                });
            }
        }
    }

    /// A second fake rule (Info default) to exercise `only_rules` filtering.
    struct OtherRule;

    impl Rule for OtherRule {
        fn id(&self) -> &'static str {
            "other-rule"
        }
        fn default_severity(&self) -> Severity {
            Severity::Info
        }
        fn description(&self) -> &'static str {
            "emits one info on index.md (test-only)"
        }
        fn check(&self, _ctx: &LintContext, out: &mut Vec<Diagnostic>) {
            out.push(Diagnostic {
                rule: self.id(),
                severity: self.default_severity(),
                file: "index.md".to_string(),
                line: None,
                col: None,
                message: "other finding".to_string(),
                note: None,
            });
        }
    }

    fn fake_rules() -> Vec<Box<dyn Rule>> {
        vec![Box::new(FakeRule), Box::new(OtherRule)]
    }

    /// A mini-site: docs/index.md, docs/a.md, docs/drafts/x.md (+ optional
    /// docgen.toml contents).
    fn mini_site(config: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(docs.join("drafts")).unwrap();
        if !config.is_empty() {
            std::fs::write(tmp.path().join("docgen.toml"), config).unwrap();
        }
        std::fs::write(docs.join("index.md"), "# Home\nSee [[a]].\n").unwrap();
        std::fs::write(docs.join("a.md"), "# A\n").unwrap();
        std::fs::write(docs.join("drafts/x.md"), "# Draft\n").unwrap();
        tmp
    }

    fn run_fake(root: &Path, options: &LintOptions) -> Result<LintOutcome, LintError> {
        run_with_rules(root, options, &fake_rules())
    }

    #[test]
    fn default_run_emits_warnings_sorted_by_file() {
        let tmp = mini_site("");
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert_eq!(out.files_checked, 3);
        // 3 fake warnings + 1 other info.
        assert_eq!(out.diagnostics.len(), 4);
        assert_eq!((out.errors, out.warnings, out.infos), (0, 3, 1));
        // Sorted by (file, line, rule) despite reverse emission; the line-less
        // other-rule diagnostic on index.md sorts before the fake one (None < Some).
        let keys: Vec<(&str, Option<u32>, &str)> = out
            .diagnostics
            .iter()
            .map(|d| (d.file.as_str(), d.line, d.rule))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("a.md", Some(1), "fake-rule"),
                ("drafts/x.md", Some(1), "fake-rule"),
                ("index.md", None, "other-rule"),
                ("index.md", Some(1), "fake-rule"),
            ]
        );
    }

    #[test]
    fn config_override_relevels_diagnostics() {
        let tmp = mini_site("[lint.rules]\n\"fake-rule\" = \"error\"\n");
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert_eq!(out.errors, 3);
        assert_eq!(out.warnings, 0);
        assert!(out
            .diagnostics
            .iter()
            .filter(|d| d.rule == "fake-rule")
            .all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn allow_silences_a_rule_entirely() {
        let tmp = mini_site("[lint.rules]\n\"fake-rule\" = \"allow\"\n");
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert!(out.diagnostics.iter().all(|d| d.rule != "fake-rule"));
        assert_eq!(out.files_checked, 3); // files are still checked
    }

    #[test]
    fn frontmatter_suppression_drops_that_files_diagnostics_only() {
        let tmp = mini_site("");
        std::fs::write(
            tmp.path().join("docs/a.md"),
            "---\nlint:\n  ignore: [fake-rule]\n---\n# A\n",
        )
        .unwrap();
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert!(!out
            .diagnostics
            .iter()
            .any(|d| d.rule == "fake-rule" && d.file == "a.md"));
        // Other files keep their findings.
        assert!(out
            .diagnostics
            .iter()
            .any(|d| d.rule == "fake-rule" && d.file == "index.md"));
    }

    #[test]
    fn ignore_globs_exclude_files_from_the_run() {
        let tmp = mini_site("[lint]\nignore = [\"drafts/**\"]\n");
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert_eq!(out.files_checked, 2);
        assert!(!out
            .diagnostics
            .iter()
            .any(|d| d.file.starts_with("drafts/")));
    }

    #[test]
    fn deny_warnings_promotes_after_suppression() {
        let tmp = mini_site("");
        // a.md suppresses fake-rule; deny_warnings must not resurrect it.
        std::fs::write(
            tmp.path().join("docs/a.md"),
            "---\nlint:\n  ignore: [fake-rule]\n---\n# A\n",
        )
        .unwrap();
        let out = run_fake(
            tmp.path(),
            &LintOptions {
                only_rules: None,
                deny_warnings: true,
            },
        )
        .unwrap();
        assert_eq!(out.warnings, 0);
        assert_eq!(out.errors, 2); // index.md + drafts/x.md, promoted
        assert!(!out.diagnostics.iter().any(|d| d.file == "a.md"));
        assert_eq!(out.infos, 1); // infos are never promoted
    }

    #[test]
    fn only_rules_filters_and_rejects_unknown_names() {
        let tmp = mini_site("");
        let out = run_fake(
            tmp.path(),
            &LintOptions {
                only_rules: Some(vec!["other-rule".to_string()]),
                deny_warnings: false,
            },
        )
        .unwrap();
        assert!(out.diagnostics.iter().all(|d| d.rule == "other-rule"));

        let err = run_fake(
            tmp.path(),
            &LintOptions {
                only_rules: Some(vec!["nope".to_string()]),
                deny_warnings: false,
            },
        )
        .unwrap_err();
        assert!(matches!(err, LintError::UnknownRule { name } if name == "nope"));
    }

    #[test]
    fn explicitly_selected_allow_rule_emits_an_info_notice() {
        // m3 regression: `--rules fake-rule` with the rule configured to allow
        // must say WHY nothing ran instead of silently no-opping.
        let tmp = mini_site("[lint.rules]\n\"fake-rule\" = \"allow\"\n");
        let out = run_fake(
            tmp.path(),
            &LintOptions {
                only_rules: Some(vec!["fake-rule".to_string()]),
                deny_warnings: false,
            },
        )
        .unwrap();
        assert_eq!(out.diagnostics.len(), 1, "{:?}", out.diagnostics);
        let d = &out.diagnostics[0];
        assert_eq!(d.rule, "fake-rule");
        assert_eq!(d.severity, Severity::Info);
        assert_eq!(d.file, "docgen.toml");
        assert_eq!(d.line, None);
        assert!(d.message.contains("allow-level"), "{d:?}");
        assert!(
            d.note.as_deref().unwrap_or("").contains("[lint.rules]"),
            "{d:?}"
        );

        // Without --rules the allow-level rule stays silent, as before.
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        assert!(out.diagnostics.iter().all(|d| d.rule != "fake-rule"));
    }

    #[test]
    fn partials_are_linted_counted_and_suppressible() {
        // M2 regression: partials appear in ctx.docs (flagged), run through
        // rules, count as checked files, and honor frontmatter suppression.
        let tmp = mini_site("");
        std::fs::write(
            tmp.path().join("docs/_frag.md"),
            "---\nlint:\n  ignore: [fake-rule]\n---\nshared\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("docs/_loud.md"), "shared too\n").unwrap();
        let out = run_fake(tmp.path(), &LintOptions::default()).unwrap();
        // 3 pages + 2 partials all count as checked.
        assert_eq!(out.files_checked, 5);
        // The fake rule ran on the unsuppressed partial...
        assert!(out
            .diagnostics
            .iter()
            .any(|d| d.rule == "fake-rule" && d.file == "_loud.md"));
        // ...and the suppressed partial's frontmatter opt-out held.
        assert!(!out.diagnostics.iter().any(|d| d.file == "_frag.md"));
    }

    #[test]
    fn releveling_preserves_explicitly_non_default_severities() {
        // A rule (default Warn) that emits one finding at its default level and
        // one explicit Info notice, like the external rules' "check skipped".
        struct Mixed;
        impl Rule for Mixed {
            fn id(&self) -> &'static str {
                "mixed-rule"
            }
            fn default_severity(&self) -> Severity {
                Severity::Warn
            }
            fn description(&self) -> &'static str {
                "emits a default-level finding plus an explicit info (test-only)"
            }
            fn check(&self, _ctx: &LintContext, out: &mut Vec<Diagnostic>) {
                for (sev, msg) in [
                    (Severity::Warn, "real finding"),
                    (Severity::Info, "check skipped"),
                ] {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: sev,
                        file: "index.md".into(),
                        line: None,
                        col: None,
                        message: msg.into(),
                        note: None,
                    });
                }
            }
        }
        let tmp = mini_site("[lint.rules]\n\"mixed-rule\" = \"error\"\n");
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(Mixed)];
        let out = run_with_rules(tmp.path(), &LintOptions::default(), &rules).unwrap();
        // The default-level finding is re-leveled to the configured error; the
        // explicit Info notice keeps its severity.
        assert_eq!((out.errors, out.warnings, out.infos), (1, 0, 1));
        let by_msg = |m: &str| {
            out.diagnostics
                .iter()
                .find(|d| d.message == m)
                .unwrap()
                .severity
        };
        assert_eq!(by_msg("real finding"), Severity::Error);
        assert_eq!(by_msg("check skipped"), Severity::Info);
    }

    #[test]
    fn unknown_rule_in_config_is_an_error() {
        let tmp = mini_site("[lint.rules]\n\"no-such-rule\" = \"warn\"\n");
        let err = run_fake(tmp.path(), &LintOptions::default()).unwrap_err();
        assert!(matches!(err, LintError::UnknownRule { name } if name == "no-such-rule"));
    }

    #[test]
    fn bad_severity_in_config_is_an_error() {
        let tmp = mini_site("[lint.rules]\n\"fake-rule\" = \"loud\"\n");
        let err = run_fake(tmp.path(), &LintOptions::default()).unwrap_err();
        assert!(
            matches!(err, LintError::BadSeverity { ref rule, ref value } if rule == "fake-rule" && value == "loud")
        );
    }

    #[test]
    fn bad_ignore_glob_is_an_error() {
        let tmp = mini_site("[lint]\nignore = [\"drafts/[\"]\n");
        let err = run_fake(tmp.path(), &LintOptions::default()).unwrap_err();
        assert!(matches!(err, LintError::BadIgnoreGlob { ref glob, .. } if glob == "drafts/["));
    }

    #[test]
    fn context_wires_slugs_partials_and_inbound() {
        // Piggy-back on the engine to sanity-check the context the rules see.
        struct Probe;
        impl Rule for Probe {
            fn id(&self) -> &'static str {
                "probe"
            }
            fn default_severity(&self) -> Severity {
                Severity::Info
            }
            fn description(&self) -> &'static str {
                "context probe (test-only)"
            }
            fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
                // index.md links [[a]] -> a has 1 inbound; index has 0.
                assert_eq!(ctx.inbound.get("a"), Some(&1));
                assert_eq!(ctx.inbound.get("index"), Some(&0));
                assert!(ctx.slugs.contains("index") && ctx.slugs.contains("a"));
                assert_eq!(ctx.partial_paths, vec!["_snippet.md".to_string()]);
                assert!(ctx.partials.contains_key("_snippet.md"));
                assert!(ctx.assets.contains("logo.svg"));
                assert_eq!(ctx.headings.get("a").map(Vec::len), Some(1));
                out.push(Diagnostic {
                    rule: "probe",
                    severity: Severity::Info,
                    file: "index.md".into(),
                    line: None,
                    col: None,
                    message: "probed".into(),
                    note: None,
                });
            }
        }
        let tmp = mini_site("");
        std::fs::write(tmp.path().join("docs/_snippet.md"), "shared\n").unwrap();
        std::fs::write(tmp.path().join("docs/logo.svg"), "<svg/>").unwrap();
        let rules: Vec<Box<dyn Rule>> = vec![Box::new(Probe)];
        let out = run_with_rules(tmp.path(), &LintOptions::default(), &rules).unwrap();
        assert_eq!(out.infos, 1); // the probe ran (its asserts passed)
    }
}
