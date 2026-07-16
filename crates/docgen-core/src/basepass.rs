//! AST pass that replaces ` ```base ` fenced code blocks with a rendered Obsidian
//! Base (static table/cards/list HTML), computed against the whole-site corpus.
//!
//! Runs in `render_doc` (top level) when the `bases` feature is on and a corpus is
//! available. Mirrors [`crate::mermaidpass`] structurally, but the block body is
//! the `.base` YAML source rendered to HTML at build time (no island, no runtime).
//! Malformed YAML degrades to an inline error block (never a panic), matching the
//! PlantUML/`:include` graceful-degradation ethos.

use comrak::nodes::{AstNode, NodeHtmlBlock, NodeValue};
use docgen_bases::{render_base_source, Corpus, RenderOptions};

/// Replace every ` ```base ` fenced block with its rendered view HTML. Returns the
/// count of bases rendered (lets callers record whether a page used one).
pub fn transform_bases<'a>(root: &'a AstNode<'a>, corpus: &Corpus, base_path: &str) -> usize {
    let opts = RenderOptions {
        base: base_path.to_string(),
        default_view_name: String::new(),
        interactive: true,
        // Overwritten per block below; `count` doubles as the block index.
        block_index: 0,
    };
    let mut count = 0;
    transform(root, corpus, &opts, &mut count);
    count
}

fn transform<'a>(node: &'a AstNode<'a>, corpus: &Corpus, opts: &RenderOptions, count: &mut usize) {
    let replacement = {
        let data = node.data.borrow();
        if let NodeValue::CodeBlock(cb) = &data.value {
            let lang = cb.info.split_whitespace().next().unwrap_or("");
            if lang.eq_ignore_ascii_case("base") {
                // Each block gets its own index so the views it emits are
                // namespaced to it. Two blocks on a page would otherwise both
                // number their views from 0, and the island keys URL-hash
                // segments and facet DOM ids off that number — each block would
                // strip the other's state from the URL on every keystroke.
                let opts = RenderOptions {
                    block_index: *count,
                    ..opts.clone()
                };
                Some(render_base_source(&cb.literal, corpus, &opts))
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
        return;
    }
    for child in node.children() {
        transform(child, corpus, opts, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{comrak_options, format_ast};
    use comrak::{parse_document, Arena};
    use docgen_bases::Note;

    fn corpus() -> Corpus {
        let n = Note {
            slug: "books/dune".into(),
            basename: "Dune".into(),
            name: "Dune.md".into(),
            path: "books/Dune.md".into(),
            tags: vec!["book".into()],
            ..Default::default()
        };
        Corpus::new(vec![n])
    }

    fn render(md: &str, corpus: &Corpus) -> (String, usize) {
        let arena = Arena::new();
        let opts = comrak_options();
        let root = parse_document(&arena, md, &opts);
        let n = transform_bases(root, corpus, "");
        (format_ast(root, &opts), n)
    }

    #[test]
    fn base_block_becomes_table() {
        let md = "```base\nfilters:\n  and:\n    - file.hasTag(\"book\")\nviews:\n  - type: table\n    order: [file.name]\n```\n";
        let (html, n) = render(md, &corpus());
        assert_eq!(n, 1);
        assert!(html.contains("docgen-base-table"));
        assert!(html.contains(">Dune<"));
        assert!(!html.contains("<code")); // not a normal code block
    }

    /// The reported failure was page-level, not base-level: two ` ```base ` blocks
    /// each numbering their views from 0. The island keys URL-hash segments
    /// (`b{idx}.`) and facet-panel DOM ids off `data-base-view`, so colliding ids
    /// meant each block stripped the other's state out of the URL on every
    /// keystroke, and both restored the same state on reload.
    #[test]
    fn two_base_blocks_on_a_page_get_distinct_view_ids() {
        let block = "```base\nviews:\n  - type: table\n    order: [file.name]\n```\n";
        let (html, n) = render(&format!("{block}\n{block}"), &corpus());
        assert_eq!(n, 2);
        assert!(html.contains("data-base-view=\"0-0\""));
        assert!(html.contains("data-base-view=\"1-0\""));
    }

    #[test]
    fn non_base_code_block_untouched() {
        let (html, n) = render("```rust\nfn x() {}\n```\n", &corpus());
        assert_eq!(n, 0);
        assert!(html.contains("<pre"));
    }

    #[test]
    fn malformed_base_yields_error_block_not_panic() {
        let md = "```base\nfilters: [unclosed\n```\n";
        let (html, n) = render(md, &corpus());
        assert_eq!(n, 1);
        assert!(html.contains("docgen-base-error"));
    }

    #[test]
    fn empty_corpus_renders_no_results() {
        let md = "```base\nviews:\n  - type: table\n```\n";
        let (html, _) = render(md, &Corpus::default());
        assert!(html.contains("No results"));
    }
}
