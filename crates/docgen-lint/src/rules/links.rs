//! Link integrity rules: wikilinks, relative markdown links, includes and
//! wikilink anchors.

use docgen_core::assetpass::{is_asset_path, normalize_join};
use docgen_core::headings::anchor_ids;
use docgen_core::pipeline::resolve_include_key;
use docgen_core::wikilink::resolve_target;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::util::{classify_url, doc_dir, line32, page_exists, LinkTarget};
use crate::rules::Rule;

/// `[[target]]` does not resolve to any page.
pub struct BrokenWikilink;

impl Rule for BrokenWikilink {
    fn id(&self) -> &'static str {
        "broken-wikilink"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "wikilink target does not resolve to any page"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for w in &doc.refs.wikilinks {
                // `[[#heading]]` is a same-page anchor link, not a broken target;
                // the anchor itself is `broken-anchor`'s job.
                if w.target.trim().is_empty() && w.anchor.is_some() {
                    continue;
                }
                if resolve_target(&w.target, &ctx.slugs).is_none() {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(w.line),
                        col: line32(w.col),
                        message: format!("[[{}]] does not resolve to any page", w.target),
                        note: None,
                    });
                }
            }
        }
    }
}

/// A relative (or docs-root-absolute) markdown link to a page that doesn't exist.
pub struct BrokenRelativeLink;

impl Rule for BrokenRelativeLink {
    fn id(&self) -> &'static str {
        "broken-relative-link"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "relative markdown link does not resolve to a known page"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            let base_dir = doc_dir(&doc.prepared.rel_path);
            for l in doc.refs.links.iter().filter(|l| !l.is_image) {
                // Join with `normalize_join` — the SAME clamping semantics the
                // build's `rewrite_page_link` uses, so a root-escaping `..`
                // resolves to the page the build would actually link to
                // instead of a false positive.
                let key = match classify_url(&l.url) {
                    LinkTarget::External => continue,
                    LinkTarget::Absolute(p) => normalize_join("", p),
                    LinkTarget::Relative(p) => normalize_join(base_dir, p),
                };
                // Only page links here (a `.md` target or an extensionless
                // clean-URL target); links to asset files are `missing-asset`'s job.
                if !key.ends_with(".md") && is_asset_path(&key) {
                    continue; // asset target — not this rule's concern
                }
                let slug = key
                    .strip_suffix(".md")
                    .unwrap_or(&key)
                    .trim_end_matches('/');
                if !page_exists(ctx, slug) {
                    out.push(self.diag(doc, l));
                }
            }
        }
    }
}

impl BrokenRelativeLink {
    fn diag(
        &self,
        doc: &crate::context::DocEntry,
        l: &docgen_core::extract::MdLinkRef,
    ) -> Diagnostic {
        Diagnostic {
            rule: self.id(),
            severity: self.default_severity(),
            file: doc.prepared.rel_path.clone(),
            line: line32(l.line),
            col: None,
            message: format!("link `{}` does not resolve to a known page", l.url),
            note: None,
        }
    }
}

/// An `:include{src=…}` whose target is missing (or the `src` itself is).
pub struct BrokenInclude;

impl Rule for BrokenInclude {
    fn id(&self) -> &'static str {
        "broken-include"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "include directive target does not exist"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            let base_dir = doc_dir(&doc.prepared.rel_path);
            for d in doc.refs.directives.iter().filter(|d| d.name == "include") {
                let mut diag = |message: String, note: Option<String>| {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(d.line),
                        col: None,
                        message,
                        note,
                    });
                };
                let src = d.src.as_deref().map(str::trim).unwrap_or("");
                if src.is_empty() {
                    diag("include directive has no src".to_string(), None);
                    continue;
                }
                match resolve_include_key(base_dir, src) {
                    Some(key)
                        if ctx.partials.contains_key(&key)
                            || ctx.docs.iter().any(|o| o.prepared.rel_path == key) => {}
                    Some(_) => diag(format!("include target `{src}` not found"), None),
                    None => diag(
                        format!("include target `{src}` not found"),
                        Some("the path escapes the docs root".to_string()),
                    ),
                }
            }
        }
    }
}

/// A `[[page#anchor]]` whose page exists but whose anchor doesn't.
pub struct BrokenAnchor;

impl Rule for BrokenAnchor {
    fn id(&self) -> &'static str {
        "broken-anchor"
    }
    fn default_severity(&self) -> Severity {
        Severity::Warn
    }
    fn description(&self) -> &'static str {
        "wikilink anchor does not match any heading on the target page"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for w in &doc.refs.wikilinks {
                let Some(anchor) = w.anchor.as_deref().map(str::trim) else {
                    continue;
                };
                if anchor.is_empty() {
                    continue;
                }
                // An empty target is a same-page anchor; an unresolvable one is
                // `broken-wikilink`'s finding, not ours.
                let slug = if w.target.trim().is_empty() {
                    doc.prepared.slug.clone()
                } else {
                    match resolve_target(&w.target, &ctx.slugs) {
                        Some(s) => s,
                        None => continue,
                    }
                };
                let Some(headings) = ctx.headings.get(&slug) else {
                    continue;
                };
                // Rendered ids exist only on h2/h3; derive them exactly as the
                // build does (shared Anchorizer, so dedup suffixes line up).
                let ids = anchor_ids(
                    headings
                        .iter()
                        .filter(|h| h.depth == 2 || h.depth == 3)
                        .map(|h| h.text.as_str()),
                );
                let found = ids.iter().any(|id| id == anchor)
                    || headings.iter().any(|h| h.text.eq_ignore_ascii_case(anchor));
                if !found {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(w.line),
                        col: line32(w.col),
                        message: format!("anchor `#{anchor}` not found in `{slug}`"),
                        note: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::test_fixture::lint_fixture;

    #[test]
    fn broken_wikilink_flags_unresolved_and_skips_resolved() {
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n[[a]] and [[nope]]\n"),
                ("a.md", "# A\n"),
            ],
            "broken-wikilink",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "index.md");
        assert_eq!(diags[0].line, Some(2));
        assert!(diags[0].message.contains("[[nope]]"));
    }

    #[test]
    fn broken_wikilink_ignores_anchor_only_and_anchor_part() {
        let diags = lint_fixture(
            &[
                ("index.md", "## Setup\n[[#setup]] and [[a#anything]]\n"),
                ("a.md", "# A\n"),
            ],
            "broken-wikilink",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn broken_wikilink_line_accounts_for_frontmatter() {
        // C1 regression: the body starts after a 4-line frontmatter block, so
        // the wikilink on body line 2 is RAW-file line 6.
        let diags = lint_fixture(
            &[(
                "index.md",
                "---\ntitle: Home\ndescription: D\n---\n# Home\nSee [[nope]].\n",
            )],
            "broken-wikilink",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].line, Some(6), "line must be raw-file-based");
    }

    #[test]
    fn broken_relative_link_flags_missing_pages_only() {
        let diags = lint_fixture(
            &[
                (
                    "guide/index.md",
                    "[ok](./intro.md) [gone](./missing.md) [ext](https://e.com/x.md) [frag](#x) [mail](mailto:a@b.c)\n",
                ),
                ("guide/intro.md", "# Intro\n"),
            ],
            "broken-relative-link",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("./missing.md"));
        assert_eq!(diags[0].file, "guide/index.md");
    }

    #[test]
    fn broken_relative_link_checks_absolute_and_extensionless() {
        let diags = lint_fixture(
            &[
                (
                    "index.md",
                    "[a](/guide/intro) [b](/nope) [c](guide/intro#sec)\n",
                ),
                ("guide/intro.md", "# Intro\n"),
            ],
            "broken-relative-link",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("/nope"));
    }

    #[test]
    fn broken_relative_link_clamps_root_escaping_dotdot_like_the_build() {
        // M3 regression: the build's `normalize_join` CLAMPS `..` at the docs
        // root, so `[a](../a.md)` from a root doc links to `a.md` — the linter
        // must agree instead of flagging a false positive.
        let diags = lint_fixture(
            &[
                ("index.md", "[a](../a.md) [gone](../../nope.md)\n"),
                ("a.md", "# A\n[[index]]\n"),
            ],
            "broken-relative-link",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("../../nope.md"), "{diags:?}");
    }

    #[test]
    fn partials_are_checked_for_broken_wikilinks() {
        // M2 regression: a partial's broken wikilink is reported, attributed
        // to the partial file itself.
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_frag.md}\n"),
                ("_frag.md", "See [[ghost-page]].\n"),
            ],
            "broken-wikilink",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "_frag.md");
        assert_eq!(diags[0].line, Some(1));
        assert!(diags[0].message.contains("ghost-page"), "{diags:?}");
    }

    #[test]
    fn broken_relative_link_leaves_asset_links_to_missing_asset() {
        let diags = lint_fixture(
            &[("index.md", "[report](./missing.pdf)\n")],
            "broken-relative-link",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn broken_include_flags_missing_src_missing_target_and_escape() {
        let diags = lint_fixture(
            &[(
                "index.md",
                ":include{}\n\n:include{src=_gone.md}\n\n:include{src=../../etc.md}\n\n:include{src=_ok.md}\n",
            ), ("_ok.md", "shared\n")],
            "broken-include",
        );
        assert_eq!(diags.len(), 3, "{diags:?}");
        assert!(diags[0].message.contains("no src"));
        assert!(diags[1].message.contains("_gone.md"));
        assert!(diags[2].note.as_deref().unwrap_or("").contains("escapes"));
    }

    #[test]
    fn broken_include_accepts_page_docs_as_targets() {
        let diags = lint_fixture(
            &[("index.md", ":include{src=a.md}\n"), ("a.md", "# A\n")],
            "broken-include",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn broken_anchor_matches_heading_ids_and_text() {
        let diags = lint_fixture(
            &[
                (
                    "index.md",
                    "[[a#getting-started]] [[a#Getting Started]] [[a#nope]]\n",
                ),
                ("a.md", "# A\n\n## Getting Started\n"),
            ],
            "broken-anchor",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("#nope"));
        assert!(diags[0].message.contains("`a`"));
    }

    #[test]
    fn broken_anchor_handles_same_page_and_duplicate_suffixes() {
        let diags = lint_fixture(
            &[(
                "index.md",
                "## Notes\n\n## Notes\n\n[[#notes-1]] and [[#notes-2]]\n",
            )],
            "broken-anchor",
        );
        // `notes-1` is the rendered id of the second heading; `notes-2` isn't real.
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("#notes-2"));
    }

    #[test]
    fn broken_anchor_skips_unresolved_targets() {
        let diags = lint_fixture(&[("index.md", "[[nope#sec]]\n")], "broken-anchor");
        assert!(diags.is_empty(), "{diags:?}");
    }
}
