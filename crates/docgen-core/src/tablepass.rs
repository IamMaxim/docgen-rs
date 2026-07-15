//! AST pass that wraps every GFM table in a horizontal-scroll container.
//!
//! Runs after the math/mermaid passes and before `format_ast`. Each
//! `NodeValue::Table` gets a `<div class="docgen-table-scroll">` opening block
//! inserted immediately before it and a matching `</div>` immediately after, so
//! a table wider than the reading column scrolls inside its own region instead
//! of squishing its columns (or forcing whole-page horizontal overflow). The CSS
//! side switches tables to `table-layout: auto` with per-cell min/max widths, so
//! columns size to their content — the wrapper is what makes the overflow
//! scrollable rather than clipped.
//!
//! Raw HTML is allowed through because `comrak_options().render.unsafe = true`
//! (same mechanism `mermaidpass` relies on). Wrapping uses sibling insertion
//! rather than node replacement because a table must keep rendering as a real
//! `<table>` — only its surroundings change.

use comrak::nodes::{AstNode, NodeHtmlBlock, NodeValue};
use comrak::Arena;

/// Wrap every table node in a `.docgen-table-scroll` div. Returns the number of
/// tables wrapped (0 when the document has none).
pub fn transform_tables<'a>(root: &'a AstNode<'a>, arena: &'a Arena<'a>) -> usize {
    // Collect first, then mutate: inserting siblings while walking the live tree
    // would perturb the sibling iterator (mirrors the collect-then-mutate shape
    // in wikilink.rs).
    let mut tables: Vec<&'a AstNode<'a>> = Vec::new();
    collect_tables(root, &mut tables);

    for table in &tables {
        let open = arena.alloc(AstNode::from(NodeValue::HtmlBlock(NodeHtmlBlock {
            block_type: 0,
            literal: "<div class=\"docgen-table-scroll\">\n".to_string(),
        })));
        let close = arena.alloc(AstNode::from(NodeValue::HtmlBlock(NodeHtmlBlock {
            block_type: 0,
            literal: "</div>\n".to_string(),
        })));
        table.insert_before(open);
        table.insert_after(close);
    }

    tables.len()
}

fn collect_tables<'a>(node: &'a AstNode<'a>, out: &mut Vec<&'a AstNode<'a>>) {
    if matches!(node.data.borrow().value, NodeValue::Table(_)) {
        out.push(node);
        // A table's children are rows/cells, never nested tables — no need to
        // recurse into it.
        return;
    }
    for child in node.children() {
        collect_tables(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{comrak_options, format_ast};
    use comrak::parse_document;

    fn render(md: &str) -> (String, usize) {
        let opts = comrak_options();
        let arena = Arena::new();
        let root = parse_document(&arena, md, &opts);
        let count = transform_tables(root, &arena);
        (format_ast(root, &opts), count)
    }

    const TABLE: &str = "| a | b |\n|---|---|\n| 1 | 2 |\n";

    #[test]
    fn wraps_a_single_table_exactly_once() {
        let (html, count) = render(TABLE);
        assert_eq!(count, 1);
        assert_eq!(html.matches("docgen-table-scroll").count(), 1);
        assert!(html.contains("<div class=\"docgen-table-scroll\">"));
        assert!(html.contains("<table>"));
        // The wrapper opens before the table and closes after it.
        let open = html.find("docgen-table-scroll").unwrap();
        let table = html.find("<table>").unwrap();
        let close = html.rfind("</div>").unwrap();
        let table_end = html.find("</table>").unwrap();
        assert!(open < table, "wrapper must open before the table");
        assert!(close > table_end, "wrapper must close after the table");
    }

    #[test]
    fn wraps_each_of_several_tables() {
        let md = format!("{TABLE}\ntext between\n\n{TABLE}");
        let (html, count) = render(&md);
        assert_eq!(count, 2);
        assert_eq!(html.matches("docgen-table-scroll").count(), 2);
    }

    #[test]
    fn document_without_a_table_is_unchanged() {
        let (html, count) = render("# Heading\n\nJust a paragraph.\n");
        assert_eq!(count, 0);
        assert!(!html.contains("docgen-table-scroll"));
    }
}
