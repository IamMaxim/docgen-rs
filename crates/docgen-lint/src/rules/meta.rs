//! Page/base metadata rules: frontmatter validity, titles, slug uniqueness and
//! `.base` file health.

use std::collections::BTreeMap;

use docgen_bases::parse_base;
use docgen_core::pipeline::first_h1;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::Rule;

/// A frontmatter block that exists but is not valid YAML.
pub struct InvalidFrontmatter;

impl Rule for InvalidFrontmatter {
    fn id(&self) -> &'static str {
        "invalid-frontmatter"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "frontmatter block is not valid YAML"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            if let Some(err) = &doc.frontmatter_error {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: doc.prepared.rel_path.clone(),
                    line: Some(1),
                    col: None,
                    message: "invalid YAML frontmatter".to_string(),
                    note: Some(err.clone()),
                });
            }
        }
    }
}

/// A page with neither a frontmatter `title` nor an h1 heading.
pub struct MissingTitle;

impl Rule for MissingTitle {
    fn id(&self) -> &'static str {
        "missing-title"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "page has no frontmatter title and no h1 heading"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in ctx.docs.iter().filter(|d| !d.is_partial) {
            // Mirror the build's `prepare` exactly: only a non-blank STRING
            // frontmatter title counts (`title: 42` is ignored by `as_str()`
            // and falls through to the h1/slug fallback).
            let fm_title = doc.prepared.frontmatter.get("title");
            let has_fm_title = fm_title
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.trim().is_empty());
            // Mirror the build's h1 fallback too: `first_h1` is a raw line
            // scan, NOT an AST walk — a `# x` line inside a code fence titles
            // the page, so it must satisfy this rule as well.
            let has_h1 = first_h1(&doc.prepared.body_md).is_some();
            if !has_fm_title && !has_h1 {
                let note = match fm_title {
                    Some(v) if !v.is_null() => {
                        "frontmatter `title` is not a string, so the build ignores it; \
                         the title falls back to the slug segment"
                    }
                    _ => "the title falls back to the slug segment",
                };
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: doc.prepared.rel_path.clone(),
                    line: None,
                    col: None,
                    message: "page has no frontmatter title and no h1 heading".to_string(),
                    note: Some(note.to_string()),
                });
            }
        }
    }
}

/// Two pages (markdown or `.base`) claiming the same slug.
pub struct DuplicateSlug;

impl Rule for DuplicateSlug {
    fn id(&self) -> &'static str {
        "duplicate-slug"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "two pages resolve to the same slug"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        // Docs first (discovery order), then `.base` pages — mirroring the
        // build, which skips a base whose slug collides with a page.
        let mut first_by_slug: BTreeMap<&str, &str> = BTreeMap::new();
        let entries = ctx
            .docs
            .iter()
            // Partials are not pages: they claim no slug (and are excluded
            // from the slug set), so they can never collide.
            .filter(|d| !d.is_partial)
            .map(|d| (d.prepared.slug.as_str(), d.prepared.rel_path.as_str()))
            .chain(
                ctx.bases
                    .iter()
                    .map(|b| (b.slug.as_str(), b.rel_path.as_str())),
            );
        for (slug, rel_path) in entries {
            match first_by_slug.get(slug) {
                Some(first) => out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: rel_path.to_string(),
                    line: None,
                    col: None,
                    message: format!("duplicate slug `{slug}` — already used by `{first}`"),
                    note: None,
                }),
                None => {
                    first_by_slug.insert(slug, rel_path);
                }
            }
        }
    }
}

/// A `.base` file whose YAML fails to parse.
pub struct InvalidBase;

impl Rule for InvalidBase {
    fn id(&self) -> &'static str {
        "invalid-base"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        ".base file is not valid YAML"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for base in &ctx.bases {
            if let Err(err) = parse_base(&base.source) {
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: base.rel_path.clone(),
                    line: err
                        .location()
                        .and_then(|loc| u32::try_from(loc.line()).ok()),
                    col: None,
                    message: "invalid .base file".to_string(),
                    note: Some(err.to_string()),
                });
            }
        }
    }
}

/// A `docgenInteractive` block containing keys docgen does not recognize.
pub struct BaseUnknownKey;

impl Rule for BaseUnknownKey {
    fn id(&self) -> &'static str {
        "base-unknown-key"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        ".base docgenInteractive block has unrecognized keys"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for base in &ctx.bases {
            let Ok(parsed) = parse_base(&base.source) else {
                continue; // `invalid-base` already reports the parse failure
            };
            for (idx, view) in parsed.views.iter().enumerate() {
                let Some(msg) = view
                    .interactive
                    .as_ref()
                    .and_then(|iv| iv.unknown_key_warning())
                else {
                    continue;
                };
                let view_name = view.name.clone().unwrap_or_else(|| format!("#{}", idx + 1));
                out.push(Diagnostic {
                    rule: self.id(),
                    severity: self.default_severity(),
                    file: base.rel_path.clone(),
                    line: None,
                    col: None,
                    message: format!("view {view_name}: {msg}"),
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
    fn invalid_frontmatter_reports_the_yaml_error_at_line_1() {
        let diags = lint_fixture(
            &[
                ("bad.md", "---\n: not: valid: yaml\n---\n# B\n"),
                ("good.md", "---\ntitle: Fine\n---\n# G\n"),
            ],
            "invalid-frontmatter",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "bad.md");
        assert_eq!(diags[0].line, Some(1));
        assert!(diags[0].note.is_some());
    }

    #[test]
    fn missing_title_accepts_frontmatter_title_or_h1() {
        let diags = lint_fixture(
            &[
                ("untitled.md", "just some text\n"),
                ("fm.md", "---\ntitle: Named\n---\ntext\n"),
                ("h1.md", "# Heading Title\ntext\n"),
                ("blank-fm.md", "---\ntitle: \"\"\n---\ntext\n"),
            ],
            "missing-title",
        );
        let files: Vec<&str> = diags.iter().map(|d| d.file.as_str()).collect();
        assert_eq!(files, vec!["blank-fm.md", "untitled.md"], "{diags:?}");
    }

    #[test]
    fn missing_title_mirrors_build_title_derivation() {
        // m2 regression, both directions:
        // (a) a non-string frontmatter title (`title: 42`) is IGNORED by the
        //     build (`as_str()` fails), so the rule must warn;
        // (b) the build's h1 fallback is a raw line scan that finds `# x`
        //     even inside a code fence, so the rule must NOT warn there.
        let diags = lint_fixture(
            &[
                ("numeric.md", "---\ntitle: 42\n---\nno heading here\n"),
                (
                    "fenced.md",
                    "```sh\n# not a real heading, but prepare() takes it\n```\n",
                ),
            ],
            "missing-title",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "numeric.md");
        assert!(
            diags[0]
                .note
                .as_deref()
                .unwrap_or("")
                .contains("not a string"),
            "{diags:?}"
        );
    }

    #[test]
    fn missing_title_skips_partials() {
        // M2: partials never render as pages, so they need no title.
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_frag.md}\n"),
                ("_frag.md", "just a fragment\n"),
            ],
            "missing-title",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn invalid_frontmatter_reported_for_partials_too() {
        // M2: a partial's malformed frontmatter block is a real finding.
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_frag.md}\n"),
                ("_frag.md", "---\n: not: valid: yaml\n---\nbody\n"),
            ],
            "invalid-frontmatter",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "_frag.md");
    }

    #[test]
    fn duplicate_slug_flags_base_colliding_with_page() {
        let diags = lint_fixture(
            &[
                ("books.md", "# Books\n"),
                ("books.base", "views:\n  - type: table\n"),
                ("other.base", "views:\n  - type: table\n"),
            ],
            "duplicate-slug",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "books.base");
        assert!(diags[0].message.contains("`books`"));
        assert!(diags[0].message.contains("books.md"));
    }

    #[test]
    fn invalid_base_reports_yaml_errors_with_note() {
        let diags = lint_fixture(
            &[
                ("bad.base", "views:\n  - type: [unclosed\n"),
                ("good.base", "views:\n  - type: table\n"),
                ("empty.base", "\n"),
            ],
            "invalid-base",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "bad.base");
        assert!(diags[0].note.is_some());
    }

    #[test]
    fn base_unknown_key_names_the_view_and_suggests_case_fixes() {
        let diags = lint_fixture(
            &[
                (
                    "books.base",
                    "views:\n  - type: table\n    name: Shelf\n    docgenInteractive:\n      pagesize: 10\n",
                ),
                (
                    "clean.base",
                    "views:\n  - type: table\n    docgenInteractive:\n      pageSize: 10\n",
                ),
            ],
            "base-unknown-key",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "books.base");
        assert!(diags[0].message.contains("Shelf"));
        assert!(diags[0].message.contains("did you mean `pageSize`"));
    }
}
