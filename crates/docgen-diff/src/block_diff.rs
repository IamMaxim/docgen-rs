//! Markdown block segmentation + block-level diff — a faithful port of
//! `block-diff.ts`.
//!
//! Blocks are segmented by a line classifier (fenced code, tables, headings,
//! blockquotes, lists, block-level HTML, paragraph runs), then diffed with the
//! same load-bearing LCS tie-break as `line_diff` (`lcs[i+1][j] >= lcs[i][j+1]`
//! ⇒ prefer *removed*). Comparison is over *normalized* blocks (whitespace
//! collapsed) while the emitted `raw` is the un-normalized block from the side
//! that owns it. `html` is left empty here and filled later by the orchestrator
//! via `docgen-core::markdown::render_markdown`.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{DocDiffBlock, DocDiffBlockKind};

struct BlockOp {
    kind: DocDiffBlockKind,
    raw: String,
    old_index: Option<usize>,
    new_index: Option<usize>,
}

/// Split a markdown document into its top-level blocks, preserving fenced-code
/// and table blocks verbatim. Frontmatter and `<script>` blocks are stripped
/// first.
pub fn split_markdown_blocks(markdown: &str) -> Vec<String> {
    let body = strip_invisible_document_parts(markdown);
    let lines: Vec<&str> = body.split('\n').collect();
    let mut blocks: Vec<String> = Vec::new();
    let mut index = 0usize;

    while index < lines.len() {
        while index < lines.len() && lines[index].trim().is_empty() {
            index += 1;
        }
        if index >= lines.len() {
            break;
        }

        let line = lines[index];
        let trimmed = line.trim();

        // Fenced code block.
        if trimmed.starts_with("```") {
            let start = index;
            index += 1;
            while index < lines.len() && !lines[index].trim().starts_with("```") {
                index += 1;
            }
            if index < lines.len() {
                index += 1;
            }
            blocks.push(trim_block(&lines[start..index]));
            continue;
        }

        // Table block.
        if trimmed.starts_with('|') {
            let start = index;
            while index < lines.len() && lines[index].trim().starts_with('|') {
                index += 1;
            }
            blocks.push(trim_block(&lines[start..index]));
            continue;
        }

        // ATX heading.
        if heading_re().is_match(line) {
            blocks.push(line.trim_end().to_string());
            index += 1;
            continue;
        }

        // Blockquote.
        if blockquote_re().is_match(line) {
            let start = index;
            while index < lines.len() && blockquote_re().is_match(lines[index]) {
                index += 1;
            }
            blocks.push(trim_block(&lines[start..index]));
            continue;
        }

        // List (continuation: blank line, list item, or indented continuation).
        if list_start_re().is_match(line) {
            let start = index;
            index += 1;
            while index < lines.len()
                && (lines[index].trim().is_empty() || list_continue_re().is_match(lines[index]))
            {
                index += 1;
            }
            blocks.push(trim_block(&lines[start..index]));
            continue;
        }

        // Block-level HTML.
        if html_block_re().is_match(line) {
            blocks.push(line.trim_end().to_string());
            index += 1;
            continue;
        }

        // Paragraph run (until blank line).
        let start = index;
        index += 1;
        while index < lines.len() && !lines[index].trim().is_empty() {
            index += 1;
        }
        blocks.push(trim_block(&lines[start..index]));
    }

    blocks
        .into_iter()
        .filter(|block| !block.trim().is_empty())
        .collect()
}

/// Strip a leading frontmatter block and all `<script>…</script>` blocks,
/// then trim. Mirrors the original's three non-greedy regex replacements.
pub fn strip_invisible_document_parts(markdown: &str) -> String {
    let without_frontmatter = frontmatter_re().replace(markdown, "");
    let without_scripts = script_re().replace_all(without_frontmatter.as_ref(), "");
    without_scripts.trim().to_string()
}

/// Build the full block-level diff stream between two markdown documents. Each
/// `DocDiffBlock` has `html = ""` (filled later by the orchestrator).
pub fn build_block_diff(old_markdown: &str, new_markdown: &str) -> Vec<DocDiffBlock> {
    let old_blocks = split_markdown_blocks(old_markdown);
    let new_blocks = split_markdown_blocks(new_markdown);
    let ops = build_block_ops(&old_blocks, &new_blocks);

    ops.into_iter()
        .enumerate()
        .map(|(index, op)| DocDiffBlock {
            id: format!("block-{index}"),
            kind: op.kind,
            raw: op.raw,
            html: String::new(),
            old_index: op.old_index,
            new_index: op.new_index,
        })
        .collect()
}

fn trim_block(lines: &[&str]) -> String {
    let joined = lines.join("\n");
    joined.trim_end().to_string()
}

fn normalize_block(block: &str) -> String {
    whitespace_re().replace_all(block.trim(), " ").into_owned()
}

fn build_block_ops(old_blocks: &[String], new_blocks: &[String]) -> Vec<BlockOp> {
    let old_norm: Vec<String> = old_blocks.iter().map(|b| normalize_block(b)).collect();
    let new_norm: Vec<String> = new_blocks.iter().map(|b| normalize_block(b)).collect();
    let lcs = build_lcs_table(&old_norm, &new_norm);

    let mut ops = Vec::new();
    let mut old_index = 0usize;
    let mut new_index = 0usize;

    while old_index < old_blocks.len() || new_index < new_blocks.len() {
        if old_index < old_blocks.len()
            && new_index < new_blocks.len()
            && old_norm[old_index] == new_norm[new_index]
        {
            ops.push(BlockOp {
                kind: DocDiffBlockKind::Context,
                raw: new_blocks[new_index].clone(),
                old_index: Some(old_index),
                new_index: Some(new_index),
            });
            old_index += 1;
            new_index += 1;
        } else if old_index < old_blocks.len()
            && (new_index == new_blocks.len()
                || lcs[old_index + 1][new_index] >= lcs[old_index][new_index + 1])
        {
            ops.push(BlockOp {
                kind: DocDiffBlockKind::Removed,
                raw: old_blocks[old_index].clone(),
                old_index: Some(old_index),
                new_index: None,
            });
            old_index += 1;
        } else {
            ops.push(BlockOp {
                kind: DocDiffBlockKind::Added,
                raw: new_blocks[new_index].clone(),
                old_index: None,
                new_index: Some(new_index),
            });
            new_index += 1;
        }
    }

    ops
}

fn build_lcs_table(old_blocks: &[String], new_blocks: &[String]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0usize; new_blocks.len() + 1]; old_blocks.len() + 1];

    for old_index in (0..old_blocks.len()).rev() {
        for new_index in (0..new_blocks.len()).rev() {
            table[old_index][new_index] = if old_blocks[old_index] == new_blocks[new_index] {
                table[old_index + 1][new_index + 1] + 1
            } else {
                table[old_index + 1][new_index].max(table[old_index][new_index + 1])
            };
        }
    }

    table
}

fn frontmatter_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // JS: /^---[\s\S]*?---\s*/  (non-greedy, anchored at start, single match).
    RE.get_or_init(|| Regex::new(r"(?s)^---.*?---\s*").unwrap())
}

fn script_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // JS: /<script[\s\S]*?<\/script>\s*/gi
    RE.get_or_init(|| Regex::new(r"(?is)<script.*?</script>\s*").unwrap())
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn heading_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(#{1,6})\s+").unwrap())
}

fn blockquote_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^>\s?").unwrap())
}

fn list_start_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s{0,3}[-*+]\s+|\s{0,3}\d+\.\s+)").unwrap())
}

fn list_continue_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\s{0,3}[-*+]\s+|\s{0,3}\d+\.\s+|\s{2,}\S)").unwrap())
}

fn html_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // JS: /^\s*<[A-Z][\s\S]*>/
    RE.get_or_init(|| Regex::new(r"(?s)^\s*<[A-Z].*>").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use DocDiffBlockKind::*;

    #[test]
    fn split_preserves_fenced_code_and_tables() {
        assert_eq!(
            split_markdown_blocks(
                "# Title\n\nPara one.\n\n```rust\nfn main() {}\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n"
            ),
            vec![
                "# Title",
                "Para one.",
                "```rust\nfn main() {}\n```",
                "| A | B |\n| - | - |\n| 1 | 2 |"
            ]
        );
    }

    #[test]
    fn block_diff_full_stream_added_removed_context() {
        let blocks = build_block_diff(
            "# Title\n\nSame paragraph.\n\nOld paragraph.\n\nTail paragraph.\n",
            "# Title\n\nSame paragraph.\n\nNew paragraph.\n\nTail paragraph.\n",
        );
        let proj: Vec<(&DocDiffBlockKind, &str)> =
            blocks.iter().map(|b| (&b.kind, b.raw.as_str())).collect();
        assert_eq!(
            proj,
            vec![
                (&Context, "# Title"),
                (&Context, "Same paragraph."),
                (&Removed, "Old paragraph."),
                (&Added, "New paragraph."),
                (&Context, "Tail paragraph."),
            ]
        );
    }

    #[test]
    fn block_diff_strips_frontmatter_and_script() {
        let blocks = build_block_diff(
            "---\ntitle: Old\n---\n\n<script>const hidden = true;</script>\n\nVisible old.",
            "---\ntitle: New\n---\n\n<script>const hidden = false;</script>\n\nVisible new.",
        );
        let proj: Vec<(&DocDiffBlockKind, &str)> =
            blocks.iter().map(|b| (&b.kind, b.raw.as_str())).collect();
        assert_eq!(
            proj,
            vec![(&Removed, "Visible old."), (&Added, "Visible new.")]
        );
    }
}
