//! AST pass that replaces comrak math nodes with build-time KaTeX HTML.
//!
//! Runs in `render_docs` after the wikilink pass and before `format_ast`.
//! Each `NodeValue::Math` becomes a `NodeValue::HtmlInline` holding the rendered
//! KaTeX markup (raw HTML is allowed through because `comrak_options().render
//! .unsafe = true`). Display math keeps KaTeX's own block-level
//! `<span class="katex-display">` wrapper, so layout stays correct even though
//! the carrier node is inline.

use comrak::nodes::{AstNode, NodeValue};

use crate::math::render_math;

/// Replace every math node in the tree with its build-time KaTeX HTML.
///
/// Returns the count of math nodes rendered, letting callers record whether a
/// page used math (drives the conditional KaTeX `<head>` link).
pub fn transform_math<'a>(root: &'a AstNode<'a>) -> usize {
    let mut count = 0;
    transform(root, &mut count);
    count
}

fn transform<'a>(node: &'a AstNode<'a>, count: &mut usize) {
    // Read the math literal/flags first, then drop the borrow before mutating.
    let replacement = {
        let data = node.data.borrow();
        if let NodeValue::Math(m) = &data.value {
            Some(render_math(&m.literal, m.display_math))
        } else {
            None
        }
    };
    if let Some(html) = replacement {
        node.data.borrow_mut().value = NodeValue::HtmlInline(html);
        *count += 1;
    }
    for child in node.children() {
        transform(child, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{comrak_options, format_ast};
    use comrak::{parse_document, Arena};

    fn render(md: &str) -> String {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, md, &opts);
        let n = transform_math(root);
        assert!(n >= 1, "expected at least one math node");
        format_ast(root, &opts)
    }

    #[test]
    fn inline_dollar_math_becomes_katex_html() {
        let html = render("Euler: $e^{i\\pi}+1=0$ done\n");
        assert!(html.contains("katex"));
        assert!(!html.contains("$e^")); // raw delimiters gone
    }

    #[test]
    fn display_math_becomes_katex_display() {
        let html = render("$$\\sum_{i=1}^n i$$\n");
        assert!(html.contains("katex-display"));
    }

    #[test]
    fn no_math_leaves_document_untouched() {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, "plain text only\n", &opts);
        assert_eq!(transform_math(root), 0);
        assert!(format_ast(root, &opts).contains("plain text only"));
    }
}
