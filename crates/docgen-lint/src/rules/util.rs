//! Small helpers shared by the built-in rules: URL classification and path
//! resolution mirroring the build's asset pass, plus common lookups.

use std::collections::BTreeSet;

use docgen_core::assetpass::{normalize_join, split_relative};

use crate::context::LintContext;

/// The docs-relative directory of a docs-relative file path (`""` for a
/// root-level file) — the base every relative reference resolves against.
pub(crate) fn doc_dir(rel_path: &str) -> &str {
    rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// A `usize` source line as the `Option<u32>` a [`crate::model::Diagnostic`] carries.
pub(crate) fn line32(line: usize) -> Option<u32> {
    u32::try_from(line).ok()
}

/// Where a markdown link/image URL points, for the link and asset rules.
pub(crate) enum LinkTarget<'a> {
    /// Left alone: external scheme (`https:`, `mailto:`, `data:`…),
    /// protocol-relative `//…`, a pure `#fragment`/`?query`, or empty.
    External,
    /// Docs-root-absolute `/path` (leading `/` and `#`/`?` suffix stripped).
    Absolute(&'a str),
    /// Relative to the doc's directory (`#`/`?` suffix stripped).
    Relative(&'a str),
}

/// Classify a URL exactly as the build's asset pass does: relative references
/// are rewritable, root-absolute ones are docs-root-relative claims we can
/// still check, everything else is left alone.
pub(crate) fn classify_url(url: &str) -> LinkTarget<'_> {
    if let Some((path, _suffix)) = split_relative(url) {
        return LinkTarget::Relative(path);
    }
    let url = url.trim();
    if let Some(rest) = url.strip_prefix('/') {
        // `//…` is protocol-relative (external); `/#…` has no path to check.
        if rest.starts_with('/') {
            return LinkTarget::External;
        }
        let path = rest.split(['#', '?']).next().unwrap_or("");
        if !path.is_empty() {
            return LinkTarget::Absolute(path);
        }
    }
    LinkTarget::External
}

/// Resolve a classified link path to a normalized docs-relative path, mirroring
/// the asset pass (`..` clamped to the docs root). `None` for external URLs.
pub(crate) fn resolve_link_path(url: &str, base_dir: &str) -> Option<String> {
    match classify_url(url) {
        LinkTarget::External => None,
        LinkTarget::Absolute(path) => Some(normalize_join("", path)),
        LinkTarget::Relative(path) => Some(normalize_join(base_dir, path)),
    }
}

/// Every docs-relative file path the site knows about: assets, `.puml`
/// diagrams, `.base` files, page docs and partials. Built once per rule run.
pub(crate) fn known_files(ctx: &LintContext) -> BTreeSet<&str> {
    let mut files: BTreeSet<&str> = ctx.assets.iter().map(String::as_str).collect();
    files.extend(ctx.diagrams.keys().map(String::as_str));
    files.extend(ctx.bases.iter().map(|b| b.rel_path.as_str()));
    files.extend(ctx.docs.iter().map(|d| d.prepared.rel_path.as_str()));
    files.extend(ctx.partials.keys().map(String::as_str));
    files
}

/// True when `slug` names a page: a markdown doc slug or a `.base` page slug.
pub(crate) fn page_exists(ctx: &LintContext, slug: &str) -> bool {
    ctx.slugs.contains(slug) || ctx.bases.iter().any(|b| b.slug == slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_url_covers_the_three_families() {
        // `./` is kept verbatim here; `normalize_join` collapses it later.
        assert!(matches!(
            classify_url("./a.png"),
            LinkTarget::Relative("./a.png")
        ));
        assert!(matches!(
            classify_url("a/b.md"),
            LinkTarget::Relative("a/b.md")
        ));
        assert!(matches!(
            classify_url("/x/y.md#z"),
            LinkTarget::Absolute("x/y.md")
        ));
        for external in [
            "https://example.com/a",
            "mailto:x@y.z",
            "data:image/png;base64,AAAA",
            "//cdn.example.com/x.png",
            "#fragment",
            "?query",
            "",
            "/",
        ] {
            assert!(
                matches!(classify_url(external), LinkTarget::External),
                "{external} should be external"
            );
        }
    }

    #[test]
    fn resolve_link_path_mirrors_assetpass_semantics() {
        assert_eq!(
            resolve_link_path("./img/x.png?v=2", "guide").as_deref(),
            Some("guide/img/x.png")
        );
        assert_eq!(
            resolve_link_path("../shared/x.png", "a/b").as_deref(),
            Some("a/shared/x.png")
        );
        // `..` escaping the docs root is clamped, exactly like the build.
        assert_eq!(
            resolve_link_path("../../x.png", "a").as_deref(),
            Some("x.png")
        );
        assert_eq!(
            resolve_link_path("/top.png#f", "a").as_deref(),
            Some("top.png")
        );
        assert_eq!(resolve_link_path("https://e.com/x.png", "a"), None);
    }
}
