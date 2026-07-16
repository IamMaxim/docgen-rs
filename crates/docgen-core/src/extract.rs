//! Read-only reference extraction for linting. Walks a document body and
//! reports every wikilink, markdown link/image, heading, fenced code block and
//! directive with (approximate, see below) 1-based source positions — without
//! touching the render/build path.
//!
//! Positions: comrak populates `sourcepos` on AST nodes during parsing, so
//! lines come straight from the AST. Two approximations apply:
//!
//! - Wikilink `col` is the start column of the folded Text/HtmlInline run that
//!   contains the `[[…]]`, advanced by the *character* offset of the link
//!   within that run. Markdown escapes or entity references consumed by the
//!   parser can make this drift a little within the line; the line is exact.
//! - Lines inside a block directive's body are the directive's opening line
//!   plus the 1-based line within `inner_md` — exact for the common case, but
//!   nested rewrites deeper down inherit the same additive scheme, so treat
//!   inner positions as best-effort.
//!
//! Directive extraction runs first (via [`directivepass::extract`]), so
//! everything the render pipeline would treat as directive content is reported
//! under its directive, and sentinel-substituted lines are re-padded so the
//! content *after* a block directive keeps its original line numbers.

use std::collections::BTreeMap;

use comrak::html::collect_text;
use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena};

use crate::directivepass;
use crate::markdown::comrak_options;
use crate::wikilink::{flat_source, parse_wikilink};

/// One `[[target#anchor|label]]` wikilink occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikilinkRef {
    /// Target as written, trimmed, without any `#anchor` part.
    pub target: String,
    /// Label after the first `|`, if present.
    pub label: Option<String>,
    /// Anchor after the first `#` in the target part, if present.
    pub anchor: Option<String>,
    /// 1-based source line (exact).
    pub line: usize,
    /// 1-based source column (approximate — see module docs).
    pub col: usize,
}

/// One markdown link or image, url as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdLinkRef {
    pub url: String,
    pub is_image: bool,
    pub line: usize,
}

/// One heading, any level h1–h6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingRef {
    pub depth: u8,
    pub text: String,
    pub line: usize,
}

/// One fenced code block (indented code blocks are not reported).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FenceRef {
    /// First word of the fence info string; empty if none.
    pub lang: String,
    pub body: String,
    pub line: usize,
}

/// One directive occurrence (block or leaf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveRef {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    /// 1-based line of the opening delimiter.
    pub line: usize,
    /// True for the block (`:::name … :::`) form.
    pub has_body: bool,
    /// The `src` attribute, when present (e.g. `:include{src=…}`).
    pub src: Option<String>,
}

/// Every reference found in one document body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocRefs {
    pub wikilinks: Vec<WikilinkRef>,
    pub links: Vec<MdLinkRef>,
    pub headings: Vec<HeadingRef>,
    pub fences: Vec<FenceRef>,
    pub directives: Vec<DirectiveRef>,
}

/// Extract all references from a markdown body (frontmatter already stripped).
/// Read-only: shares parsing helpers with the render path but never mutates
/// render behavior. Directive bodies are recursed into; their references are
/// reported with lines offset by the directive's opening line.
pub fn extract_refs(body_md: &str) -> DocRefs {
    let mut refs = DocRefs::default();
    collect(body_md, 0, &mut refs);
    refs
}

/// Recursive worker: `line_offset` is added to every 1-based line found in
/// `body_md` (0 at the top level; a directive's opening line when recursing
/// into its `inner_md`, since inner line 1 is the doc line right after it).
fn collect(body_md: &str, line_offset: usize, refs: &mut DocRefs) {
    let (rewritten, instances) = directivepass::extract(body_md);

    for inst in &instances {
        refs.directives.push(DirectiveRef {
            name: inst.name.clone(),
            attrs: inst.attrs.clone(),
            line: line_offset + inst.line,
            has_body: inst.is_block,
            src: inst.attrs.get("src").cloned(),
        });
    }

    // A block directive spanning N source lines was replaced by a single
    // sentinel line; re-pad with N-1 blank lines so everything after it keeps
    // its original line numbers.
    let padded = repad_sentinels(&rewritten, &instances);

    let arena = Arena::new();
    let options = comrak_options();
    let root = parse_document(&arena, &padded, &options);
    scan_ast(root, line_offset, refs);

    // Recurse into block-directive bodies (their own directives, links, …).
    for inst in &instances {
        if inst.is_block && !inst.inner_md.is_empty() {
            collect(&inst.inner_md, line_offset + inst.line, refs);
        }
    }
}

/// Restore the line count of the sentinel-substituted body: each *block*
/// directive consumed `inner lines + 2` source lines (opener + inner + closer)
/// but occupies one sentinel line, so append `inner lines + 1` blank lines.
/// Leaf sentinels are inline and don't change the line count.
fn repad_sentinels(rewritten: &str, instances: &[directivepass::DirectiveInstance]) -> String {
    let mut out = rewritten.to_string();
    for (idx, inst) in instances.iter().enumerate() {
        if !inst.is_block {
            continue;
        }
        let inner_lines = if inst.inner_md.is_empty() {
            0
        } else {
            inst.inner_md.split('\n').count()
        };
        let s = directivepass::sentinel(idx);
        out = out.replacen(&s, &format!("{s}{}", "\n".repeat(inner_lines + 1)), 1);
    }
    out
}

/// One pass over the parsed AST: headings, links/images, fences, wikilinks.
fn scan_ast<'a>(root: &'a AstNode<'a>, line_offset: usize, refs: &mut DocRefs) {
    for node in root.descendants() {
        let data = node.data.borrow();
        let line = line_offset + data.sourcepos.start.line;
        match &data.value {
            NodeValue::Heading(h) => {
                refs.headings.push(HeadingRef {
                    depth: h.level,
                    text: collect_text(node).trim().to_string(),
                    line,
                });
            }
            NodeValue::CodeBlock(cb) if cb.fenced => {
                refs.fences.push(FenceRef {
                    lang: cb
                        .info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    body: cb.literal.clone(),
                    line,
                });
            }
            NodeValue::Link(l) => {
                refs.links.push(MdLinkRef {
                    url: l.url.clone(),
                    is_image: false,
                    line,
                });
            }
            NodeValue::Image(l) => {
                refs.links.push(MdLinkRef {
                    url: l.url.clone(),
                    is_image: true,
                    line,
                });
            }
            _ => {}
        }
    }
    scan_wikilinks(root, line_offset, refs);
}

/// Read-only mirror of `wikilink::transform_wikilinks`' run folding: walk
/// maximal runs of foldable (Text/HtmlInline) siblings and report every
/// complete `[[…]]` found in the combined text. Text nodes never contain code
/// spans/fences (comrak parses those as Code/CodeBlock), so wikilinks inside
/// code are naturally not reported.
fn scan_wikilinks<'a>(root: &'a AstNode<'a>, line_offset: usize, refs: &mut DocRefs) {
    for parent in root.descendants().filter(|n| n.first_child().is_some()) {
        let children: Vec<&'a AstNode<'a>> = parent.children().collect();
        let mut i = 0;
        while i < children.len() {
            if flat_source(children[i]).is_none() {
                i += 1;
                continue;
            }
            let start = i;
            let mut combined = String::new();
            while i < children.len() {
                match flat_source(children[i]) {
                    Some(s) => {
                        combined.push_str(&s);
                        i += 1;
                    }
                    None => break,
                }
            }
            if !combined.contains("[[") {
                continue;
            }

            let sp = children[start].data.borrow().sourcepos;
            let line = line_offset + sp.start.line;
            let base_col = sp.start.column;

            let mut rest = combined.as_str();
            let mut consumed_chars = 0usize;
            while let Some(open) = rest.find("[[") {
                let Some(close_rel) = rest[open + 2..].find("]]") else {
                    break; // unterminated `[[` — same as the render pass
                };
                let close = open + 2 + close_rel;
                let inner = &rest[open + 2..close];

                let (target_full, label) = parse_wikilink(inner);
                // The render pass keeps `#` as part of the target; only the
                // extractor splits the anchor out.
                let (target, anchor) = match target_full.split_once('#') {
                    Some((t, a)) => (t.trim().to_string(), Some(a.trim().to_string())),
                    None => (target_full, None),
                };
                refs.wikilinks.push(WikilinkRef {
                    target,
                    label,
                    anchor,
                    line,
                    col: base_col + consumed_chars + rest[..open].chars().count(),
                });

                consumed_chars += rest[..close + 2].chars().count();
                rest = &rest[close + 2..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilinks_with_anchor_label_and_broken_target_are_reported() {
        let md = "See [[guide/intro#setup|Setup]] and [[nope]] here.\n";
        let refs = extract_refs(md);
        assert_eq!(refs.wikilinks.len(), 2);

        let w = &refs.wikilinks[0];
        assert_eq!(w.target, "guide/intro");
        assert_eq!(w.anchor.as_deref(), Some("setup"));
        assert_eq!(w.label.as_deref(), Some("Setup"));
        assert_eq!(w.line, 1);
        assert_eq!(w.col, 5); // "See " is 4 chars; `[[` opens at column 5

        let w = &refs.wikilinks[1];
        assert_eq!(w.target, "nope"); // extraction doesn't resolve; linter does
        assert!(w.anchor.is_none());
        assert!(w.label.is_none());
        assert_eq!(w.line, 1);
    }

    #[test]
    fn wikilink_inside_code_span_or_fence_is_not_reported() {
        let md = "inline `[[not-a-link]]` here\n\n```\n[[also-not]]\n```\n\n[[real]]\n";
        let refs = extract_refs(md);
        assert_eq!(refs.wikilinks.len(), 1);
        assert_eq!(refs.wikilinks[0].target, "real");
        assert_eq!(refs.wikilinks[0].line, 7);
    }

    #[test]
    fn wikilink_line_is_exact_across_multiline_paragraph() {
        let md = "first line\nsecond [[here]]\n";
        let refs = extract_refs(md);
        assert_eq!(refs.wikilinks.len(), 1);
        assert_eq!(refs.wikilinks[0].line, 2);
    }

    #[test]
    fn md_links_and_images_are_reported_with_url_as_written() {
        let md = "[text](https://example.com/a) and\n![alt](../img/pic.png)\n";
        let refs = extract_refs(md);
        assert_eq!(refs.links.len(), 2);
        assert_eq!(refs.links[0].url, "https://example.com/a");
        assert!(!refs.links[0].is_image);
        assert_eq!(refs.links[0].line, 1);
        assert_eq!(refs.links[1].url, "../img/pic.png");
        assert!(refs.links[1].is_image);
        assert_eq!(refs.links[1].line, 2);
    }

    #[test]
    fn headings_all_levels_with_lines() {
        let md = "# A\n## B\n### C\n#### D\n##### E\n###### F\n";
        let refs = extract_refs(md);
        assert_eq!(refs.headings.len(), 6);
        for (i, h) in refs.headings.iter().enumerate() {
            assert_eq!(h.depth as usize, i + 1);
            assert_eq!(h.line, i + 1);
        }
        assert_eq!(refs.headings[2].text, "C");
    }

    #[test]
    fn fences_carry_lang_body_and_line() {
        let md = "intro\n\n```rust ignore\nfn main() {}\n```\n\n```\nplain\n```\n";
        let refs = extract_refs(md);
        assert_eq!(refs.fences.len(), 2);
        assert_eq!(refs.fences[0].lang, "rust"); // first word of the info string
        assert_eq!(refs.fences[0].body, "fn main() {}\n");
        assert_eq!(refs.fences[0].line, 3);
        assert_eq!(refs.fences[1].lang, "");
        assert_eq!(refs.fences[1].line, 7);
    }

    #[test]
    fn directive_lines_reported_including_after_fence_with_colons() {
        let md = "```text\n:::\n```\n\n:::callout{type=note}\nbody\n:::\n\n:note[x]{}\n";
        let refs = extract_refs(md);
        assert_eq!(refs.directives.len(), 2);
        assert_eq!(refs.directives[0].name, "callout");
        assert_eq!(refs.directives[0].line, 5);
        assert!(refs.directives[0].has_body);
        assert_eq!(refs.directives[1].name, "note");
        assert_eq!(refs.directives[1].line, 9);
        assert!(!refs.directives[1].has_body);
        // The fence itself is still a fence ref, and its `:::` is not a directive.
        assert_eq!(refs.fences.len(), 1);
        assert_eq!(refs.fences[0].line, 1);
    }

    #[test]
    fn content_after_block_directive_keeps_original_lines() {
        let md = "# T\n\n:::callout{}\nbody\n:::\n\n## After\n";
        let refs = extract_refs(md);
        let after = refs.headings.iter().find(|h| h.text == "After").unwrap();
        assert_eq!(after.line, 7);
    }

    #[test]
    fn recursion_into_directive_body_offsets_lines_by_directive_line() {
        let md = "intro\n\n:::callout{type=note}\n[[inner-link]]\n\n## Inner Heading\n:::\n";
        let refs = extract_refs(md);
        assert_eq!(refs.directives.len(), 1);
        assert_eq!(refs.directives[0].line, 3);
        // inner_md line 1 → doc line 4; inner line 3 → doc line 6.
        assert_eq!(refs.wikilinks.len(), 1);
        assert_eq!(refs.wikilinks[0].target, "inner-link");
        assert_eq!(refs.wikilinks[0].line, 4);
        let inner = refs
            .headings
            .iter()
            .find(|h| h.text == "Inner Heading")
            .unwrap();
        assert_eq!(inner.line, 6);
    }

    #[test]
    fn nested_directives_are_reported_from_recursion() {
        let md = ":::callout{type=note}\n:::callout{type=warning}\ninner\n:::\n:::\n";
        let refs = extract_refs(md);
        assert_eq!(refs.directives.len(), 2);
        assert_eq!(refs.directives[0].line, 1);
        assert_eq!(refs.directives[1].line, 2);
        assert_eq!(refs.directives[1].attrs.get("type").unwrap(), "warning");
    }

    #[test]
    fn include_src_is_surfaced() {
        let md = ":include{src=partials/setup.md}\n";
        let refs = extract_refs(md);
        assert_eq!(refs.directives.len(), 1);
        assert_eq!(refs.directives[0].name, "include");
        assert_eq!(refs.directives[0].src.as_deref(), Some("partials/setup.md"));
        assert!(!refs.directives[0].has_body);
    }

    #[test]
    fn empty_body_yields_empty_refs() {
        assert_eq!(extract_refs(""), DocRefs::default());
    }
}
