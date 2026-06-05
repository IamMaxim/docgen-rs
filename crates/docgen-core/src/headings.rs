//! Extract the `h2`/`h3` heading outline of a document and stamp matching
//! `id` anchors onto the rendered heading tags.
//!
//! The right-rail "On this page" table of contents and the scroll-spy island
//! both key off `id` attributes on `<h2>`/`<h3>` in the rendered article. Comrak
//! *can* emit heading ids, but it places them on a nested
//! `<a class="anchor" id="…">` element rather than the heading itself, which the
//! `h2[id]` / `h3[id]` selectors the scroll-spy uses would never match. So we
//! anchorize the heading text ourselves (with comrak's own [`Anchorizer`], so
//! the slugs are byte-for-byte what comrak would have produced) and inject the
//! `id` directly onto the heading's opening tag.

use comrak::html::collect_text;
use comrak::nodes::{AstNode, NodeValue};
use comrak::Anchorizer;
use serde::Serialize;

/// One entry in a page's heading outline. Only `h2`/`h3` are collected — `h1`
/// is the (hidden) page title and `h4`+ are too deep for the rail TOC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heading {
    /// Stable anchor id (matches the `id` stamped on the rendered heading tag).
    pub id: String,
    /// Human-readable heading text.
    pub text: String,
    /// Heading level: `2` or `3`.
    pub depth: u8,
}

/// Walk the AST in document order and collect the `h2`/`h3` outline, anchorizing
/// each heading's text into a unique id. One [`Anchorizer`] per call guarantees
/// the `-1`, `-2`, … de-duplication suffixes match comrak's own scheme.
pub fn collect_headings<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    let mut anchorizer = Anchorizer::new();
    let mut out = Vec::new();
    for node in root.descendants() {
        if let NodeValue::Heading(h) = &node.data.borrow().value {
            if h.level == 2 || h.level == 3 {
                let text = collect_text(node);
                let id = anchorizer.anchorize(&text);
                out.push(Heading { id, text: text.trim().to_string(), depth: h.level });
            }
        }
    }
    out
}

/// Inject `id="…"` onto the `<h2>`/`<h3>` opening tags of `html`, in document
/// order, using the ids from [`collect_headings`].
///
/// `headings` MUST be in the same order the tags appear in `html` (it is, since
/// both derive from one AST walk). Each heading consumes the next matching
/// `<h2>`/`<h3>` occurrence. Comrak emits bare `<h2>` / `<h3>` (no sourcepos,
/// no existing id), so a plain ordered text rewrite is exact and unambiguous.
pub fn stamp_heading_ids(html: &str, headings: &[Heading]) -> String {
    let mut out = String::with_capacity(html.len() + headings.len() * 24);
    let mut rest = html;
    let mut iter = headings.iter();

    loop {
        // Find the next `<h2>` or `<h3>` opening tag.
        let h2 = rest.find("<h2>");
        let h3 = rest.find("<h3>");
        let next = match (h2, h3) {
            (None, None) => None,
            (Some(a), None) => Some((a, 2u8)),
            (None, Some(b)) => Some((b, 3u8)),
            (Some(a), Some(b)) => {
                if a < b {
                    Some((a, 2))
                } else {
                    Some((b, 3))
                }
            }
        };

        let Some((pos, level)) = next else {
            out.push_str(rest);
            break;
        };

        let tag_len = 4; // "<hN>"
        out.push_str(&rest[..pos]);
        match iter.next() {
            Some(h) if h.depth == level => {
                out.push_str(&format!("<h{} id=\"{}\">", level, escape_attr(&h.id)));
            }
            // Misalignment (shouldn't happen): leave the tag untouched.
            _ => out.push_str(&rest[pos..pos + tag_len]),
        }
        rest = &rest[pos + tag_len..];
    }

    out
}

/// Minimal attribute escaping for an anchorized id (anchorize already strips
/// most markup-significant characters; this guards the remainder).
fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena};

    #[test]
    fn collects_h2_and_h3_skips_h1_and_h4() {
        let arena = Arena::new();
        let root = parse_document(
            &arena,
            "# Title\n\n## Alpha\n\n### Beta\n\n#### Deep\n",
            &crate::markdown::comrak_options(),
        );
        let hs = collect_headings(root);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0], Heading { id: "alpha".into(), text: "Alpha".into(), depth: 2 });
        assert_eq!(hs[1], Heading { id: "beta".into(), text: "Beta".into(), depth: 3 });
    }

    #[test]
    fn duplicate_headings_get_unique_suffixes() {
        let arena = Arena::new();
        let root = parse_document(
            &arena,
            "## Notes\n\n## Notes\n",
            &crate::markdown::comrak_options(),
        );
        let hs = collect_headings(root);
        assert_eq!(hs[0].id, "notes");
        assert_eq!(hs[1].id, "notes-1");
    }

    #[test]
    fn stamps_ids_onto_heading_tags_in_order() {
        let html = "<h2>Alpha</h2>\n<p>x</p>\n<h3>Beta</h3>\n";
        let headings = vec![
            Heading { id: "alpha".into(), text: "Alpha".into(), depth: 2 },
            Heading { id: "beta".into(), text: "Beta".into(), depth: 3 },
        ];
        let out = stamp_heading_ids(html, &headings);
        assert!(out.contains(r#"<h2 id="alpha">Alpha</h2>"#));
        assert!(out.contains(r#"<h3 id="beta">Beta</h3>"#));
    }

    #[test]
    fn stamp_is_noop_without_headings() {
        let html = "<p>no headings here</p>";
        assert_eq!(stamp_heading_ids(html, &[]), html);
    }
}
