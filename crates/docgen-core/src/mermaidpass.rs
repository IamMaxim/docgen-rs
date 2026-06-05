//! AST pass that replaces ```mermaid fenced code blocks with an Alpine island
//! container.
//!
//! Runs in `render_docs` after the math pass and before `format_ast`. Each
//! mermaid fence becomes a `NodeValue::HtmlBlock` holding an inert container:
//! the diagram source is preserved verbatim (HTML-escaped) inside a hidden
//! `<pre>` so the island can read and render it without a network round-trip.
//! Raw HTML is allowed through because `comrak_options().render.unsafe = true`.

use comrak::nodes::{AstNode, NodeHtmlBlock, NodeValue};

use crate::util::escape_html;

/// Replace every ```mermaid fenced block with an island container.
///
/// Returns the count of diagrams found, letting callers record whether a page
/// uses mermaid (drives the lazy island + library load).
pub fn transform_mermaid<'a>(root: &'a AstNode<'a>) -> usize {
    let mut count = 0;
    transform(root, &mut count);
    count
}

fn transform<'a>(node: &'a AstNode<'a>, count: &mut usize) {
    // Read the code block info/literal first, then drop the borrow before mutating.
    let replacement = {
        let data = node.data.borrow();
        if let NodeValue::CodeBlock(cb) = &data.value {
            let lang = cb.info.split_whitespace().next().unwrap_or("");
            if lang.eq_ignore_ascii_case("mermaid") {
                Some(container_html(&cb.literal, *count))
            } else {
                None
            }
        } else {
            None
        }
    };
    if let Some(html) = replacement {
        node.data.borrow_mut().value = NodeValue::HtmlBlock(NodeHtmlBlock {
            block_type: 0,
            literal: html,
        });
        *count += 1;
        // A code block has no markdown children to recurse into.
        return;
    }
    for child in node.children() {
        transform(child, count);
    }
}

/// Build the inert island container. `x-data="docgenMermaid"` + `x-init="render()"`
/// hook the Alpine island; the raw source sits in a hidden `<pre>` the island reads.
fn container_html(src: &str, idx: usize) -> String {
    format!(
        "<div class=\"docgen-mermaid\" x-data=\"docgenMermaid\" x-init=\"render()\" \
         data-mermaid-id=\"docgen-mermaid-{idx}\">\
         <pre class=\"docgen-mermaid__src\" hidden>{}</pre>\
         <div class=\"docgen-mermaid__out\"></div></div>",
        escape_html(src)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{comrak_options, format_ast};
    use comrak::{parse_document, Arena};

    fn render(md: &str) -> (String, usize) {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, md, &opts);
        let n = transform_mermaid(root);
        (format_ast(root, &opts), n)
    }

    #[test]
    fn mermaid_block_becomes_island_container() {
        let (html, n) = render("```mermaid\ngraph TD; A-->B;\n```\n");
        assert_eq!(n, 1);
        assert!(html.contains("docgen-mermaid"));
        assert!(html.contains("x-data=\"docgenMermaid\""));
        assert!(html.contains("x-init=\"render()\""));
        assert!(html.contains("graph TD")); // source preserved (escaped)
        assert!(!html.contains("<code")); // not rendered as a normal code block
    }

    #[test]
    fn escapes_diagram_source() {
        let (html, _) = render("```mermaid\ngraph TD; A[\"<x>\"]-->B;\n```\n");
        assert!(html.contains("&lt;x&gt;"));
        assert!(!html.contains("<x>"));
    }

    #[test]
    fn non_mermaid_code_block_untouched() {
        let (html, n) = render("```rust\nfn x(){}\n```\n");
        assert_eq!(n, 0);
        assert!(html.contains("<pre"));
    }

    #[test]
    fn info_string_with_trailing_metadata_is_recognized() {
        // `cb.info.split_whitespace().next()` must still match a fence whose
        // info string carries trailing metadata after `mermaid`.
        let (html, n) = render("```mermaid title=\"x\"\ngraph TD;A-->B;\n```\n");
        assert_eq!(n, 1);
        assert!(html.contains("docgen-mermaid"));
        assert!(html.contains("graph TD"));
    }

    #[test]
    fn multiple_diagrams_get_distinct_ids() {
        let (html, n) = render(
            "```mermaid\ngraph TD;A-->B;\n```\n\n```mermaid\ngraph TD;C-->D;\n```\n",
        );
        assert_eq!(n, 2);
        assert!(html.contains("data-mermaid-id=\"docgen-mermaid-0\""));
        assert!(html.contains("data-mermaid-id=\"docgen-mermaid-1\""));
    }
}
