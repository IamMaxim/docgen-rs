//! Site/page structure rules: orphaned pages, heading hygiene, empty pages and
//! partials nothing includes.

use std::collections::{BTreeMap, BTreeSet};

use docgen_core::pipeline::resolve_include_key;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::util::{doc_dir, line32};
use crate::rules::Rule;

/// A page no other page links to.
pub struct OrphanPage;

impl Rule for OrphanPage {
    fn id(&self) -> &'static str {
        "orphan-page"
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn description(&self) -> &'static str {
        "page has no inbound links"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        // Partials are not pages — they are transcluded, never linked to.
        for doc in ctx.docs.iter().filter(|d| !d.is_partial) {
            let slug = &doc.prepared.slug;
            // The root index is the site's entry point — never an orphan.
            if slug == "index" {
                continue;
            }
            if ctx.inbound.get(slug).copied().unwrap_or(0) == 0 {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: doc.prepared.rel_path.clone(),
                    line: None,
                    col: None,
                    message: "page has no inbound links".to_string(),
                    note: None,
                });
            }
        }
    }
}

/// The same heading text appearing more than once within one page.
pub struct DuplicateHeading;

impl Rule for DuplicateHeading {
    fn id(&self) -> &'static str {
        "duplicate-heading"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "same heading text appears more than once on a page"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            // Normalized text -> line of the first occurrence.
            let mut first_seen: BTreeMap<String, usize> = BTreeMap::new();
            for h in &doc.refs.headings {
                let key = h.text.trim().to_lowercase();
                match first_seen.get(&key) {
                    Some(first_line) => out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(h.line),
                        col: None,
                        message: format!("duplicate heading `{}`", h.text),
                        note: Some(format!("first occurrence at line {first_line}")),
                    }),
                    None => {
                        first_seen.insert(key, h.line);
                    }
                }
            }
        }
    }
}

/// A heading more than one level deeper than the heading before it.
pub struct HeadingLevelJump;

impl Rule for HeadingLevelJump {
    fn id(&self) -> &'static str {
        "heading-level-jump"
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn description(&self) -> &'static str {
        "heading level increases by more than one"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for pair in doc.refs.headings.windows(2) {
                let (prev, next) = (&pair[0], &pair[1]);
                if next.depth > prev.depth + 1 {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(next.line),
                        col: None,
                        message: format!(
                            "heading level jumps from h{} to h{}",
                            prev.depth, next.depth
                        ),
                        note: None,
                    });
                }
            }
        }
    }
}

/// A page whose body is empty (or frontmatter-only).
pub struct EmptyPage;

impl Rule for EmptyPage {
    fn id(&self) -> &'static str {
        "empty-page"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "page body is empty"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        // A deliberately-thin partial is not an "empty page" — skip partials.
        for doc in ctx.docs.iter().filter(|d| !d.is_partial) {
            if doc.prepared.body_md.trim().is_empty() {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: doc.prepared.rel_path.clone(),
                    line: None,
                    col: None,
                    message: "page has no content".to_string(),
                    note: None,
                });
            }
        }
    }
}

/// A partial (`_*.md`) that no page or partial ever includes.
pub struct UnusedPartial;

impl Rule for UnusedPartial {
    fn id(&self) -> &'static str {
        "unused-partial"
    }
    fn default_severity(&self) -> Severity {
        Severity::Info
    }
    fn description(&self) -> &'static str {
        "partial is never included by any page or partial"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        // Every include target, resolved from pages AND from partials —
        // `ctx.docs` carries both (partials included with their own refs), so
        // one pass over it sees every `:include`.
        let mut referenced: BTreeSet<String> = BTreeSet::new();
        for doc in &ctx.docs {
            let base_dir = doc_dir(&doc.prepared.rel_path);
            for d in doc.refs.directives.iter().filter(|d| d.name == "include") {
                if let Some(key) = d
                    .src
                    .as_deref()
                    .and_then(|src| resolve_include_key(base_dir, src))
                {
                    referenced.insert(key);
                }
            }
        }
        for rel_path in &ctx.partial_paths {
            if !referenced.contains(rel_path) {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: rel_path.clone(),
                    line: None,
                    col: None,
                    message: "partial is never included".to_string(),
                    note: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::test_fixture::lint_fixture;

    #[test]
    fn orphan_page_flags_unlinked_pages_and_exempts_index() {
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n[[a]]\n"),
                ("a.md", "# A\n"),
                ("lonely.md", "# Lonely\n"),
            ],
            "orphan-page",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "lonely.md");
        assert_eq!(diags[0].message, "page has no inbound links");
    }

    #[test]
    fn orphan_page_counts_relative_md_links_as_inbound() {
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n[a](./a.md)\n"),
                ("a.md", "# A\n[[index]]\n"),
            ],
            "orphan-page",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn orphan_page_not_flagged_for_clamped_root_escaping_link_target() {
        // M3 regression: `[a](../a.md)` from the root clamps to `a.md` in the
        // build, so a.md has an inbound link and is NOT an orphan.
        let diags = lint_fixture(
            &[("index.md", "# Home\n[a](../a.md)\n"), ("a.md", "# A\n")],
            "orphan-page",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn orphan_page_and_empty_page_skip_partials() {
        // M2: partials are transcluded, never linked — no orphan/empty findings.
        let orphans = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_thin.md}\n"),
                ("_thin.md", "\n"),
            ],
            "orphan-page",
        );
        assert!(orphans.is_empty(), "{orphans:?}");
        let empties = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_thin.md}\n"),
                ("_thin.md", "\n"),
            ],
            "empty-page",
        );
        assert!(empties.is_empty(), "{empties:?}");
    }

    #[test]
    fn duplicate_heading_line_accounts_for_frontmatter() {
        // C1 regression: heading lines (finding AND first-occurrence note)
        // shift by the 3-line frontmatter block.
        let diags = lint_fixture(
            &[(
                "index.md",
                "---\ntitle: T\n---\n# T\n\n## Notes\n\n## Notes\n",
            )],
            "duplicate-heading",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, Some(8));
        assert_eq!(diags[0].note.as_deref(), Some("first occurrence at line 6"));
    }

    #[test]
    fn duplicate_heading_flags_repeats_case_insensitively() {
        let diags = lint_fixture(
            &[(
                "index.md",
                "# T\n\n## Notes\n\n## Other\n\n## notes\n\n## Notes\n",
            )],
            "duplicate-heading",
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!(diags[0].line, Some(7));
        assert_eq!(diags[1].line, Some(9));
        assert_eq!(diags[0].note.as_deref(), Some("first occurrence at line 3"));
    }

    #[test]
    fn heading_level_jump_flags_skipped_levels_only() {
        let diags = lint_fixture(
            &[("index.md", "# T\n\n### Deep\n\n#### Fine\n\n## Back\n")],
            "heading-level-jump",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, Some(3));
        assert!(diags[0].message.contains("h1 to h3"));
    }

    #[test]
    fn empty_page_flags_blank_and_frontmatter_only_pages() {
        let diags = lint_fixture(
            &[
                ("blank.md", "   \n"),
                ("fm-only.md", "---\ntitle: T\n---\n\n"),
                ("full.md", "# Content\n"),
            ],
            "empty-page",
        );
        let files: Vec<&str> = diags.iter().map(|d| d.file.as_str()).collect();
        assert_eq!(files, vec!["blank.md", "fm-only.md"], "{diags:?}");
    }

    #[test]
    fn unused_partial_sees_includes_from_pages_and_partials() {
        let diags = lint_fixture(
            &[
                ("index.md", ":include{src=_a.md}\n"),
                ("_a.md", "outer\n\n:include{src=nested/_b.md}\n"),
                ("nested/_b.md", "inner\n"),
                ("_unused.md", "never\n"),
            ],
            "unused-partial",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "_unused.md");
        assert_eq!(diags[0].line, None);
    }
}
