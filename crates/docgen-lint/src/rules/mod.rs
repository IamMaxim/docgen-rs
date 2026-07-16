//! The rule registry. A rule inspects the site-wide [`LintContext`] and emits
//! [`Diagnostic`]s at its *default* severity; the engine re-levels them to the
//! configured severity afterward, so rules never read `[lint.rules]` themselves.

mod assets;
mod diagrams;
mod links;
mod meta;
mod structure;
mod util;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};

/// One lint rule.
pub trait Rule {
    /// Stable kebab-case id (config key in `[lint.rules]`).
    fn id(&self) -> &'static str;
    /// Severity used when `[lint.rules]` has no override for this rule.
    fn default_severity(&self) -> Severity;
    /// One-line description, for `--list-rules`.
    fn description(&self) -> &'static str;
    /// Emit findings into `out` with `severity = self.default_severity()`;
    /// the engine re-levels them to the resolved severity.
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>);
}

/// Every built-in rule, in the order they run (stable output grouping:
/// links, diagrams, assets, meta, structure).
pub fn all_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(links::BrokenWikilink),
        Box::new(links::BrokenRelativeLink),
        Box::new(links::BrokenInclude),
        Box::new(links::BrokenAnchor),
        Box::new(diagrams::PlantumlSrcMissing),
        Box::new(diagrams::PlantumlEmpty),
        Box::new(diagrams::MermaidEmpty),
        Box::new(diagrams::MermaidUnknownType),
        Box::new(assets::MissingAsset),
        Box::new(assets::UnknownComponent),
        Box::new(meta::InvalidFrontmatter),
        Box::new(meta::MissingTitle),
        Box::new(meta::DuplicateSlug),
        Box::new(meta::InvalidBase),
        Box::new(meta::BaseUnknownKey),
        Box::new(structure::OrphanPage),
        Box::new(structure::DuplicateHeading),
        Box::new(structure::HeadingLevelJump),
        Box::new(structure::EmptyPage),
        Box::new(structure::UnusedPartial),
    ]
}

/// Shared test seam for the per-rule tests: build a throwaway site and run the
/// real engine with a single rule selected.
#[cfg(test)]
pub(crate) mod test_fixture {
    use crate::engine::{run_with_rules, LintOptions};
    use crate::model::Diagnostic;

    /// Write `files` (docs-relative; `docgen.toml` goes to the project root)
    /// into a temp project and lint it with only the `only` rule enabled.
    pub(crate) fn lint_fixture(files: &[(&str, &str)], only: &str) -> Vec<Diagnostic> {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
        for (path, contents) in files {
            let full = if *path == "docgen.toml" {
                tmp.path().join(path)
            } else {
                tmp.path().join("docs").join(path)
            };
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        let options = LintOptions {
            only_rules: Some(vec![only.to_string()]),
            deny_warnings: false,
        };
        run_with_rules(tmp.path(), &options, &super::all_rules())
            .unwrap()
            .diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn registry_ids_are_unique_kebab_case_and_grouped() {
        let rules = all_rules();
        assert_eq!(rules.len(), 20);
        let ids: Vec<&str> = rules.iter().map(|r| r.id()).collect();
        let unique: BTreeSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate rule id");
        for rule in &rules {
            assert!(
                rule.id()
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{} is not kebab-case",
                rule.id()
            );
            assert!(!rule.description().is_empty());
        }
        // Stable group order: links, diagrams, assets, meta, structure.
        assert_eq!(ids[0], "broken-wikilink");
        assert_eq!(ids[4], "plantuml-src-missing");
        assert_eq!(ids[8], "missing-asset");
        assert_eq!(ids[10], "invalid-frontmatter");
        assert_eq!(ids[15], "orphan-page");
        assert_eq!(ids[19], "unused-partial");
    }

    #[test]
    fn a_clean_site_produces_no_diagnostics() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("docs")).unwrap();
        std::fs::write(
            tmp.path().join("docs/index.md"),
            "# Home\n\nSee [[a]].\n\n## Section\n",
        )
        .unwrap();
        std::fs::write(tmp.path().join("docs/a.md"), "# A\n\nBack to [[index]].\n").unwrap();
        let out = crate::engine::run_with_rules(
            tmp.path(),
            &crate::engine::LintOptions::default(),
            &all_rules(),
        )
        .unwrap();
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    }
}
