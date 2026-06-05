use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena};

use crate::markdown::comrak_options;

/// Extract searchable plaintext from a markdown body (frontmatter already stripped).
/// Walks the AST collecting text + inline-code, and unwraps `[[wikilinks]]` to their
/// label/target text. Collapses all whitespace runs to single spaces.
pub fn plaintext(body_md: &str) -> String {
    let arena = Arena::new();
    let options = comrak_options();
    let root = parse_document(&arena, body_md, &options);

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

    #[test]
    fn strips_markup_to_plaintext() {
        let text = plaintext("# Title\n\nSome **bold** and `code` and a [link](/x).\n");
        assert!(text.contains("Title"));
        assert!(text.contains("Some bold and code and a link"));
        assert!(!text.contains('#'));
        assert!(!text.contains('*'));
        assert!(!text.contains("/x"));
    }

    #[test]
    fn includes_wikilink_inner_text() {
        // Wikilinks are still raw `[[...]]` text in the body the index sees.
        let text = plaintext("see [[guide/intro|The Intro]] here\n");
        // We keep the human-facing label/target text, not the brackets.
        assert!(text.contains("The Intro") || text.contains("guide/intro"));
        assert!(!text.contains("[["));
    }

    #[test]
    fn collapses_whitespace() {
        let text = plaintext("a\n\n\nb    c\n");
        assert_eq!(text, "a b c");
    }
}
