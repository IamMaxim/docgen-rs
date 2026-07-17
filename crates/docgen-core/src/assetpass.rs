//! AST pass that rewrites *relative* asset references in a doc body to
//! base-absolute URLs.
//!
//! Markdown authored inside a docs tree naturally uses paths relative to the
//! source file, e.g. `![](./attachments/img.png)` in `docs/system/index.md`.
//! But pages are emitted as clean URLs (`system/index.md` → served at
//! `/system/index/`), so a browser resolving `./attachments/img.png` against the
//! page URL asks for `/system/index/attachments/img.png` — the wrong place. The
//! asset itself is copied mirroring the docs tree (see docgen-build), i.e. to
//! `/system/attachments/img.png`.
//!
//! This pass bridges the two: it resolves each relative reference against the
//! *source* directory of the page (`system` here) and emits it as an absolute,
//! base-prefixed URL (`{base}/system/attachments/img.png`). Because the result is
//! root-absolute it is immune to the extra clean-URL directory level, and it
//! points exactly where the copy step wrote the file.
//!
//! Scope (matches the chosen design):
//!   * Image nodes (`![](...)`) — always rewritten (an image is always an asset).
//!   * Link nodes (`[](...)`) to a non-`.md` file (e.g. `[report](./x.pdf)`) —
//!     rewritten as an asset, exactly like an image.
//!   * Link nodes to another *page* — a relative `.md` target (`[x](./other.md)`)
//!     or an extensionless clean-URL target (`[x](../guide/intro)`) — rewritten to
//!     the page's clean URL (`{base}/{slug}`), but ONLY when the resolved path is a
//!     known page slug. Unknown targets are left untouched (they may be
//!     intentional, or an author typo we shouldn't silently mask). `[[wikilinks]]`
//!     are handled separately and never reach here as Link nodes.
//!
//! References that are already absolute (`/…`), protocol-relative (`//…`),
//! external (`https:`, `mailto:`, `data:`, …), or pure fragments (`#…`) are left
//! untouched.

use comrak::nodes::{AstNode, NodeValue};

use crate::wikilink::SlugSet;

/// Rewrite relative asset references (image srcs, non-`.md` link hrefs) and
/// relative page links (`.md`/clean-URL link hrefs that resolve to a known
/// `slugs` entry) in `root` to `{base}/{resolved}`, resolving each against
/// `source_dir` (the page's docs-relative directory, `/`-separated, no trailing
/// slash; `""` for a root-level page). `base` is the deploy sub-path prefix (`""`
/// for root deployment). `slugs` is the whole site's slug set, used to validate
/// page-link targets.
pub fn transform_asset_urls<'a>(
    root: &'a AstNode<'a>,
    base: &str,
    source_dir: &str,
    slugs: &SlugSet,
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
) {
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        match &mut data.value {
            NodeValue::Image(link) => {
                if let Some(rewritten) = rewrite_url(&link.url, base, source_dir, true, asset_urls)
                {
                    link.url = rewritten;
                }
            }
            NodeValue::Link(link) => {
                // Try an asset rewrite first (non-`.md` file); if the target is a
                // page, fall back to resolving it to a clean URL.
                if let Some(rewritten) = rewrite_url(&link.url, base, source_dir, false, asset_urls)
                {
                    link.url = rewritten;
                } else if let Some(rewritten) =
                    rewrite_page_link(&link.url, base, source_dir, slugs)
                {
                    link.url = rewritten;
                }
            }
            _ => {}
        }
    }
}

/// Prefilter `url` to a rewritable *relative* reference, returning
/// `(path_part, suffix)` where `suffix` is a preserved trailing `#fragment` /
/// `?query`. Returns `None` for anything left untouched: empty, root-absolute
/// (`/…`) or protocol-relative (`//…`), a pure fragment/query (`#…` / `?…`), or an
/// external/non-path scheme (`https:`, `mailto:`, `data:`, `tel:`…).
pub fn split_relative(url: &str) -> Option<(&str, &str)> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    // Root-absolute or protocol-relative (`/…`, `//…`), or a pure fragment/query.
    if url.starts_with('/') || url.starts_with('#') || url.starts_with('?') {
        return None;
    }
    // A scheme is a leading run of scheme-chars followed by `:`, before any `/?#`.
    if has_scheme(url) {
        return None;
    }
    // Peel off a trailing `#fragment` / `?query` to preserve it verbatim.
    let (path_part, suffix) = split_suffix(url);
    if path_part.is_empty() {
        return None;
    }
    Some((path_part, suffix))
}

/// Decide whether `url` is a rewritable relative asset reference and, if so,
/// return the rewritten base-absolute URL. `is_image` skips the extension check
/// (images are always assets); for links, only non-`.md` targets with a real
/// file extension are rewritten.
fn rewrite_url(
    url: &str,
    base: &str,
    source_dir: &str,
    is_image: bool,
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
) -> Option<String> {
    let (path_part, suffix) = split_relative(url)?;

    // Links to pages (a `.md` target or an extensionless clean-URL target) are not
    // static assets — leave them for `rewrite_page_link`.
    if !is_image && !is_asset_path(path_part) {
        return None;
    }

    let resolved = normalize_join(source_dir, path_part);
    if let Some(resolver) = asset_urls {
        if let Some(public_url) = resolver.resolve(&resolved) {
            return Some(format!("{public_url}{suffix}"));
        }
    }
    Some(format!("{base}/{resolved}{suffix}"))
}

/// Rewrite a relative link to another *page* to its clean URL (`{base}/{slug}`).
/// A page target is a `.md` path (`[x](./other.md)`) or an extensionless clean-URL
/// path (`[x](../guide/intro)`); the `.md` suffix is stripped exactly as slugs are
/// formed (see `prepare`), the path is resolved against `source_dir`, and the
/// result is emitted ONLY when it is a known page slug. Non-page targets (asset
/// extensions — already handled by `rewrite_url`) and paths that don't resolve to
/// a known page return `None`, leaving the author's link untouched.
fn rewrite_page_link(url: &str, base: &str, source_dir: &str, slugs: &SlugSet) -> Option<String> {
    let (path_part, suffix) = split_relative(url)?;

    // The page's docs-relative stem: strip a trailing `.md` (matching how slugs are
    // formed), or take the whole path when it's extensionless. A non-`.md` file
    // extension means it's an asset, not a page — not our job.
    let stem = match path_part.strip_suffix(".md") {
        Some(s) => s,
        None if !is_asset_path(path_part) => path_part,
        None => return None,
    };
    if stem.is_empty() {
        return None;
    }

    let resolved = normalize_join(source_dir, stem);
    if !slugs.contains(&resolved) {
        return None;
    }
    Some(format!("{base}/{resolved}{suffix}"))
}

/// True if `url` begins with a URL scheme (`scheme:`), per a permissive read of
/// RFC 3986: an ALPHA followed by ALPHA/DIGIT/`+`/`-`/`.`, terminated by `:`,
/// with the `:` appearing before any path separator.
fn has_scheme(url: &str) -> bool {
    let mut chars = url.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for (_, c) in chars {
        if c == ':' {
            return true;
        }
        if c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.' {
            continue;
        }
        // A `/`, `?`, `#`, or anything else before a `:` means no scheme.
        return false;
    }
    false
}

/// Split off a trailing `#fragment` or `?query` (whichever comes first), so the
/// path can be normalized while the suffix is preserved verbatim.
fn split_suffix(url: &str) -> (&str, &str) {
    match url.find(['#', '?']) {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    }
}

/// A link target counts as a static asset when its final path segment has a file
/// extension other than `.md`. Extensionless targets (clean-URL page links) and
/// `.md` targets are treated as pages, not assets.
pub fn is_asset_path(path: &str) -> bool {
    let last = path.rsplit('/').next().unwrap_or(path);
    match last.rsplit_once('.') {
        // A leading dot with nothing before it (e.g. a dotfile) is not an extension.
        Some((stem, ext)) => !stem.is_empty() && !ext.is_empty() && !ext.eq_ignore_ascii_case("md"),
        None => false,
    }
}

/// Join `source_dir` and a relative `path`, then resolve `.`/`..` segments,
/// producing a clean docs-root-relative path (no leading slash). `..` that would
/// escape the docs root is clamped (dropped) rather than allowed to walk out.
pub fn normalize_join(source_dir: &str, path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    let combined = if source_dir.is_empty() {
        path.to_string()
    } else {
        format!("{source_dir}/{path}")
    };
    for seg in combined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{comrak_options, format_ast};
    use comrak::{parse_document, Arena};

    fn render(md: &str, base: &str, source_dir: &str) -> String {
        render_with_slugs(md, base, source_dir, &SlugSet::new())
    }

    fn render_with_slugs(md: &str, base: &str, source_dir: &str, slugs: &SlugSet) -> String {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, md, &opts);
        transform_asset_urls(root, base, source_dir, slugs, None);
        format_ast(root, &opts)
    }

    fn slugset(items: &[&str]) -> SlugSet {
        items.iter().map(|s| s.to_string()).collect()
    }

    struct MapResolver(std::collections::HashMap<String, String>);
    impl crate::asseturl::AssetUrlResolver for MapResolver {
        fn resolve(&self, rel_path: &str) -> Option<String> {
            self.0.get(rel_path).cloned()
        }
    }

    fn render_with_resolver(
        md: &str,
        base: &str,
        source_dir: &str,
        resolver: &dyn crate::asseturl::AssetUrlResolver,
    ) -> String {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, md, &opts);
        transform_asset_urls(root, base, source_dir, &SlugSet::new(), Some(resolver));
        format_ast(root, &opts)
    }

    #[test]
    fn resolver_rewrites_image_to_public_url() {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "system/attachments/image.png".to_string(),
            "https://cdn.example.com/docs-assets/system/attachments/image.abcdef0123456789.png"
                .to_string(),
        );
        let html = render_with_resolver(
            "![](./attachments/image.png)\n",
            "",
            "system",
            &MapResolver(m),
        );
        assert!(
            html.contains(
                r#"src="https://cdn.example.com/docs-assets/system/attachments/image.abcdef0123456789.png""#
            ),
            "{html}"
        );
    }

    #[test]
    fn resolver_miss_falls_back_to_base_absolute() {
        // Resolver present but path not in the map -> today's local URL.
        let html = render_with_resolver(
            "![](./attachments/image.png)\n",
            "",
            "system",
            &MapResolver(std::collections::HashMap::new()),
        );
        assert!(
            html.contains(r#"src="/system/attachments/image.png""#),
            "{html}"
        );
    }

    #[test]
    fn resolver_not_consulted_for_page_links() {
        // A `.md` page link is never an asset; resolver must not affect it.
        let mut m = std::collections::HashMap::new();
        m.insert(
            "system/other".to_string(),
            "https://cdn.example.com/WRONG".to_string(),
        );
        let slugs = slugset(&["system/other"]);
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, "[x](./other.md)\n", &opts);
        transform_asset_urls(root, "", "system", &slugs, Some(&MapResolver(m)));
        let html = format_ast(root, &opts);
        assert!(html.contains(r#"href="/system/other""#), "{html}");
    }

    #[test]
    fn relative_image_resolves_against_source_dir() {
        // The core bug: page docs/system/index.md, image ./attachments/image.png.
        let html = render("![Image](./attachments/image.png)\n", "", "system");
        assert!(
            html.contains(r#"src="/system/attachments/image.png""#),
            "{html}"
        );
    }

    #[test]
    fn relative_image_without_dot_prefix() {
        let html = render("![Image](attachments/image.png)\n", "", "system");
        assert!(
            html.contains(r#"src="/system/attachments/image.png""#),
            "{html}"
        );
    }

    #[test]
    fn base_prefix_is_applied() {
        let html = render("![Image](./attachments/image.png)\n", "/docs", "system");
        assert!(
            html.contains(r#"src="/docs/system/attachments/image.png""#),
            "{html}"
        );
    }

    #[test]
    fn parent_dir_traversal_is_resolved() {
        let html = render("![](../shared/logo.png)\n", "", "system/sub");
        assert!(html.contains(r#"src="/system/shared/logo.png""#), "{html}");
    }

    #[test]
    fn root_level_page_has_no_dir_prefix() {
        let html = render("![](./img/x.png)\n", "", "");
        assert!(html.contains(r#"src="/img/x.png""#), "{html}");
    }

    #[test]
    fn absolute_and_external_images_untouched() {
        let abs = render("![](/already/abs.png)\n", "", "system");
        assert!(abs.contains(r#"src="/already/abs.png""#), "{abs}");
        let ext = render("![](https://example.com/x.png)\n", "", "system");
        assert!(ext.contains(r#"src="https://example.com/x.png""#), "{ext}");
        let data = render("![](data:image/png;base64,AAAA)\n", "", "system");
        assert!(
            data.contains("src=\"data:image/png;base64,AAAA\""),
            "{data}"
        );
    }

    #[test]
    fn protocol_relative_image_untouched() {
        let html = render("![](//cdn.example.com/x.png)\n", "", "system");
        assert!(html.contains(r#"src="//cdn.example.com/x.png""#), "{html}");
    }

    #[test]
    fn relative_link_to_asset_is_rewritten() {
        let html = render("[report](./files/report.pdf)\n", "", "system");
        assert!(
            html.contains(r#"href="/system/files/report.pdf""#),
            "{html}"
        );
    }

    #[test]
    fn relative_md_link_resolves_to_clean_url() {
        // A `.md` link to a sibling page: docs/system/index.md -> ./other.md, which
        // is the page docs/system/other.md (slug `system/other`), served at
        // /system/other. The clean-URL nesting is bypassed by the absolute URL.
        let slugs = slugset(&["system/index", "system/other"]);
        let html = render_with_slugs("[other](./other.md)\n", "", "system", &slugs);
        assert!(html.contains(r#"href="/system/other""#), "{html}");
    }

    #[test]
    fn relative_md_link_parent_dir_resolves() {
        // From docs/system/page.md, `../guide/intro.md` is docs/guide/intro.md.
        let slugs = slugset(&["guide/intro"]);
        let html = render_with_slugs("[x](../guide/intro.md)\n", "", "system", &slugs);
        assert!(html.contains(r#"href="/guide/intro""#), "{html}");
    }

    #[test]
    fn extensionless_page_link_resolves_when_known() {
        // A clean-URL-style relative target with no extension is a page link too.
        let slugs = slugset(&["guide/intro"]);
        let html = render_with_slugs("[x](../guide/intro)\n", "", "system", &slugs);
        assert!(html.contains(r#"href="/guide/intro""#), "{html}");
    }

    #[test]
    fn page_link_base_prefix_and_fragment_preserved() {
        let slugs = slugset(&["system/other"]);
        let html = render_with_slugs("[x](./other.md#sec)\n", "/docs", "system", &slugs);
        assert!(html.contains(r#"href="/docs/system/other#sec""#), "{html}");
    }

    #[test]
    fn unknown_page_link_is_left_untouched() {
        // The resolved path is not a known page slug — don't mask the author's
        // (possibly intentional, possibly mistaken) link.
        let slugs = slugset(&["system/index"]);
        let md = render_with_slugs("[nope](./nope.md)\n", "", "system", &slugs);
        assert!(md.contains(r#"href="./nope.md""#), "{md}");
        // An extensionless target that doesn't resolve is likewise left alone.
        let ext = render_with_slugs("[x](../guide/intro)\n", "", "system", &slugs);
        assert!(ext.contains(r#"href="../guide/intro""#), "{ext}");
    }

    #[test]
    fn asset_link_still_wins_over_page_link() {
        // A non-`.md` file is an asset regardless of the slug set: always rewritten.
        let slugs = slugset(&["system/report"]);
        let html = render_with_slugs("[report](./report.pdf)\n", "", "system", &slugs);
        assert!(html.contains(r#"href="/system/report.pdf""#), "{html}");
    }

    #[test]
    fn external_and_absolute_page_links_untouched() {
        let slugs = slugset(&["system/other"]);
        let ext = render_with_slugs("[x](https://example.com/other.md)\n", "", "system", &slugs);
        assert!(
            ext.contains(r#"href="https://example.com/other.md""#),
            "{ext}"
        );
        let abs = render_with_slugs("[x](/system/other.md)\n", "", "system", &slugs);
        assert!(abs.contains(r#"href="/system/other.md""#), "{abs}");
    }

    #[test]
    fn query_and_fragment_are_preserved() {
        let q = render("![](./x.png?v=2)\n", "", "system");
        assert!(q.contains(r#"src="/system/x.png?v=2""#), "{q}");
        let f = render("[dl](./x.pdf#page=3)\n", "", "system");
        assert!(f.contains(r#"href="/system/x.pdf#page=3""#), "{f}");
    }
}
