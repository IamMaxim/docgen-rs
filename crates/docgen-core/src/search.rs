use comrak::nodes::{AstNode, NodeValue};

use crate::model::SearchEntry;

/// Serialize the search index to a compact JSON array for `dist/search-index.json`.
pub fn index_json(entries: &[SearchEntry]) -> String {
    serde_json::to_string(entries).expect("SearchEntry serializes")
}

/// Extract searchable plaintext from an already-parsed markdown AST (frontmatter
/// already stripped). Walks the AST collecting text + inline-code, and unwraps
/// `[[wikilinks]]` to their label/target text. Collapses whitespace runs to single
/// spaces.
///
/// Fenced/indented code blocks (`NodeValue::CodeBlock`) and HTML blocks are
/// intentionally excluded: only prose and inline code are indexed.
///
/// IMPORTANT: this must run on the AST *before* `transform_wikilinks`, which
/// rewrites the `[[...]]` Text nodes into `HtmlInline` anchors that this walk
/// would otherwise skip.
pub fn plaintext<'a>(root: &'a AstNode<'a>) -> String {
    let mut buf = String::new();
    for node in root.descendants() {
        match &node.data.borrow().value {
            NodeValue::Text(t) => {
                push_unwrapping_wikilinks(&mut buf, t);
                // Separate contributions from distinct inline/block nodes so text
                // across paragraph or element boundaries does not run together.
                buf.push(' ');
            }
            NodeValue::Code(c) => {
                buf.push(' ');
                buf.push_str(&c.literal);
                buf.push(' ');
            }
            _ => {}
        }
    }

    buf.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Append text, replacing any `[[target|label]]`/`[[target]]` with its display text.
fn push_unwrapping_wikilinks(buf: &mut String, text: &str) {
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        if let Some(close_rel) = rest[open + 2..].find("]]") {
            let close = open + 2 + close_rel;
            buf.push_str(&rest[..open]);
            let inner = &rest[open + 2..close];
            let display = match inner.split_once('|') {
                Some((_t, label)) if !label.trim().is_empty() => label.trim(),
                _ => inner.trim(),
            };
            buf.push(' ');
            buf.push_str(display);
            buf.push(' ');
            rest = &rest[close + 2..];
        } else {
            break;
        }
    }
    buf.push_str(rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::comrak_options;
    use comrak::{parse_document, Arena};

    /// Parse a body and extract plaintext, mirroring the pipeline's pre-transform call.
    fn plaintext_of(body: &str) -> String {
        let arena = Arena::new();
        let options = comrak_options();
        let root = parse_document(&arena, body, &options);
        plaintext(root)
    }

    #[test]
    fn strips_markup_to_plaintext() {
        let text = plaintext_of("# Title\n\nSome **bold** and `code` and a [link](/x).\n");
        assert!(text.contains("Title"));
        assert!(text.contains("Some bold and code and a link"));
        assert!(!text.contains('#'));
        assert!(!text.contains('*'));
        assert!(!text.contains("/x"));
    }

    #[test]
    fn includes_wikilink_inner_text() {
        // Wikilinks are still raw `[[...]]` text in the body the index sees.
        let text = plaintext_of("see [[guide/intro|The Intro]] here\n");
        // We keep the human-facing label/target text, not the brackets.
        assert!(text.contains("The Intro") || text.contains("guide/intro"));
        assert!(!text.contains("[["));
    }

    #[test]
    fn unwraps_broken_wikilink_target_into_index() {
        // A broken/unresolvable `[[target]]` still contributes its display word,
        // with no brackets — search must find broken-link prose.
        let text = plaintext_of("see [[missing-page]] x\n");
        assert!(text.contains("missing-page"));
        assert!(!text.contains("[["));
    }

    #[test]
    fn unterminated_wikilink_bracket_is_left_literal() {
        // An unterminated `[[` has no closing `]]`; push_unwrapping_wikilinks
        // breaks and emits the remainder verbatim, brackets included. This
        // documents the (rare, malformed-input) behavior so a regression is caught.
        let text = plaintext_of("see [[half open\n");
        assert!(text.contains("half open"));
        assert!(text.contains("[["));
    }

    #[test]
    fn collapses_whitespace() {
        let text = plaintext_of("a\n\n\nb    c\n");
        assert_eq!(text, "a b c");
    }

    #[test]
    fn serializes_index_to_json_array() {
        use crate::model::SearchEntry;
        let entries = vec![
            SearchEntry {
                slug: "a".into(),
                title: "A".into(),
                text: "alpha".into(),
            },
            SearchEntry {
                slug: "b".into(),
                title: "B".into(),
                text: "beta".into(),
            },
        ];
        let json = index_json(&entries);
        assert!(json.starts_with('['));
        assert!(json.contains(r#""slug":"a""#));
        assert!(json.contains(r#""title":"A""#));
        assert!(json.contains(r#""text":"alpha""#));
    }
}
