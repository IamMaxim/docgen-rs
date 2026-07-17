//! Asset and component rules: referenced files that don't exist and directives
//! nothing knows how to render.

use docgen_core::assetpass::is_asset_path;

use crate::context::LintContext;
use crate::model::{Diagnostic, Severity};
use crate::rules::util::{
    classify_url, doc_dir, known_files, line32, resolve_link_path, LinkTarget,
};
use crate::rules::Rule;

/// An image (or a link to a non-`.md` file) whose target file does not exist.
pub struct MissingAsset;

impl Rule for MissingAsset {
    fn id(&self) -> &'static str {
        "missing-asset"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "referenced image or asset file does not exist"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        let files = known_files(ctx);
        for doc in &ctx.docs {
            let base_dir = doc_dir(&doc.prepared.rel_path);
            for l in &doc.refs.links {
                // Images are always assets; plain links only when they name a
                // file with a non-`.md` extension (page links are
                // `broken-relative-link`'s job).
                if !l.is_image {
                    let is_asset_link = match classify_url(&l.url) {
                        LinkTarget::External => false,
                        LinkTarget::Absolute(p) | LinkTarget::Relative(p) => is_asset_path(p),
                    };
                    if !is_asset_link {
                        continue;
                    }
                }
                // External / data: / pure-fragment URLs resolve to None: skip.
                let Some(resolved) = resolve_link_path(&l.url, base_dir) else {
                    continue;
                };
                if !files.contains(resolved.as_str()) {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(l.line),
                        col: None,
                        message: format!("asset `{}` not found", l.url),
                        note: Some(format!("resolved to `{resolved}` under the docs root")),
                    });
                }
            }
        }
    }
}

/// A directive whose name no component (built-in or project) provides.
pub struct UnknownComponent;

impl Rule for UnknownComponent {
    fn id(&self) -> &'static str {
        "unknown-component"
    }
    fn default_severity(&self) -> Severity {
        Severity::Error
    }
    fn description(&self) -> &'static str {
        "directive does not name a known component"
    }
    fn check(&self, ctx: &LintContext, out: &mut Vec<Diagnostic>) {
        for doc in &ctx.docs {
            for d in &doc.refs.directives {
                // `include` and `plantuml` are pipeline built-ins, not components.
                if d.name == "include" || d.name == "plantuml" {
                    continue;
                }
                if !ctx.components.contains(&d.name) {
                    out.push(Diagnostic {
                        rule: self.id(),
                        severity: self.default_severity(),
                        file: doc.prepared.rel_path.clone(),
                        line: line32(d.line),
                        col: None,
                        message: format!("unknown directive/component `{}`", d.name),
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
    fn missing_asset_flags_absent_images_and_files() {
        let diags = lint_fixture(
            &[
                (
                    "guide/index.md",
                    "![ok](./img/logo.png)\n![gone](./img/nope.png)\n[report](../files/gone.pdf)\n",
                ),
                ("guide/img/logo.png", "png"),
            ],
            "missing-asset",
        );
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags[0].message.contains("./img/nope.png"));
        assert_eq!(
            diags[0].note.as_deref(),
            Some("resolved to `guide/img/nope.png` under the docs root")
        );
        assert!(diags[1].message.contains("../files/gone.pdf"));
    }

    #[test]
    fn missing_asset_checks_partials_and_offsets_frontmatter_lines() {
        // M2 + C1 regression: a partial's missing image is reported against
        // the partial, at the RAW file line (after its 3-line frontmatter).
        let diags = lint_fixture(
            &[
                ("index.md", "# Home\n\n:include{src=_frag.md}\n"),
                (
                    "_frag.md",
                    "---\ntitle: frag\n---\n![gone](./img/nope.png)\n",
                ),
            ],
            "missing-asset",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].file, "_frag.md");
        assert_eq!(diags[0].line, Some(4));
        assert!(diags[0].message.contains("./img/nope.png"), "{diags:?}");
    }

    #[test]
    fn missing_asset_skips_external_data_and_page_links() {
        let diags = lint_fixture(
            &[(
                "index.md",
                "![a](https://e.com/x.png)\n![b](data:image/png;base64,AAAA)\n![c](//cdn.e.com/x.png)\n[page](./other.md)\n",
            )],
            "missing-asset",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn missing_asset_strips_query_and_fragment_and_checks_absolute() {
        let diags = lint_fixture(
            &[
                (
                    "index.md",
                    "![v](./logo.png?v=2)\n[dl](/files/x.pdf#page=3)\n",
                ),
                ("logo.png", "png"),
            ],
            "missing-asset",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("/files/x.pdf"));
    }

    #[test]
    fn missing_asset_accepts_diagram_and_base_files() {
        let diags = lint_fixture(
            &[
                ("index.md", "[d](./d.puml)\n[b](./books.base)\n"),
                ("d.puml", "@startuml\n@enduml\n"),
                ("books.base", "views:\n  - type: table\n"),
            ],
            "missing-asset",
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_component_flags_unregistered_directives_only() {
        let diags = lint_fixture(
            &[(
                "index.md",
                ":::callout{type=note}\nx\n:::\n\n:::wat{}\nx\n:::\n\n:include{src=_s.md}\n\n:::plantuml{src=d.puml}\n:::\n",
            ), ("_s.md", "s\n"), ("d.puml", "@startuml\n@enduml\n")],
            "unknown-component",
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert!(diags[0].message.contains("`wat`"));
        assert_eq!(diags[0].line, Some(5));
    }
}
