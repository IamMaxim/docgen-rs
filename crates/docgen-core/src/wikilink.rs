use std::collections::BTreeSet;

use comrak::nodes::{AstNode, NodeValue};
use comrak::Arena;

/// The set of all known slugs, used to resolve wikilink targets.
pub type SlugSet = BTreeSet<String>;

/// Split a `[[...]]` inner string into `(target, Some(label))` or `(target, None)`.
/// Splits on the FIRST `|` only; the remainder is the label.
pub fn parse_wikilink(inner: &str) -> (String, Option<String>) {
    match inner.split_once('|') {
        Some((t, label)) => (t.trim().to_string(), Some(label.trim().to_string())),
        None => (inner.trim().to_string(), None),
    }
}

/// Resolve a wikilink target to a slug.
/// Order: trimmed-exact slug match, then case-insensitive basename match
/// (basename = last `/`-segment of a slug). First basename match wins by
/// `SlugSet` (BTreeSet) order, making resolution deterministic.
pub fn resolve_target(target: &str, slugs: &SlugSet) -> Option<String> {
    let t = target.trim();
    if t.is_empty() {
        return None;
    }
    if slugs.contains(t) {
        return Some(t.to_string());
    }
    let needle = t.to_ascii_lowercase();
    slugs
        .iter()
        .find(|slug| {
            slug.rsplit('/')
                .next()
                .unwrap_or(slug)
                .eq_ignore_ascii_case(&needle)
        })
        .cloned()
}

/// Outcome of transforming one document's AST.
pub struct WikilinkPass {
    /// Target slugs this doc links to, deduped, in first-seen document order.
    pub resolved: Vec<String>,
}

/// Minimal HTML-attribute / text escaper for the small strings we inject.
/// Single-pass into one allocation (avoids the 4-string `replace` chain).
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The display text for a wikilink: the label if present and non-blank, else the
/// target. An empty/whitespace-only label is treated as absent so we never emit an
/// anchor with invisible text. Mirrors `search::push_unwrapping_wikilinks`.
fn display_text(target: &str, label: Option<String>) -> String {
    label
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| target.to_string())
}

/// Build the inline HTML for one wikilink occurrence and, if resolved, push its
/// target slug into `resolved` (deduped, first-seen order).
fn render_link(
    inner: &str,
    slugs: &SlugSet,
    resolved: &mut Vec<String>,
    seen: &mut BTreeSet<String>,
) -> String {
    let (target, label) = parse_wikilink(inner);
    match resolve_target(&target, slugs) {
        Some(slug) => {
            if seen.insert(slug.clone()) {
                resolved.push(slug.clone());
            }
            let text = display_text(&target, label);
            format!(
                r#"<a class="docgen-wikilink" href="/{}">{}</a>"#,
                esc(&slug),
                esc(&text)
            )
        }
        None => {
            let text = display_text(&target, label);
            format!(
                r#"<span class="docgen-wikilink docgen-wikilink--broken" data-target="{}">{}</span>"#,
                esc(&target),
                esc(&text)
            )
        }
    }
}

/// Walk the AST; for each Text node containing `[[...]]`, split it into
/// surrounding Text nodes + raw-HTML inline nodes for each wikilink.
/// The flat source text a child node contributes when reconstructing an inline
/// run, or `None` if the node breaks the run (it is not foldable into text).
fn flat_source(node: &AstNode<'_>) -> Option<String> {
    match &node.data.borrow().value {
        NodeValue::Text(t) => Some(t.to_string()),
        // Raw inline HTML inside `[[ ... ]]` is folded back into the target string
        // (e.g. `[[a<b>]]`), so the resolver/escaper sees the literal `a<b>`.
        NodeValue::HtmlInline(h) => Some(h.clone()),
        _ => None,
    }
}

pub fn transform_wikilinks<'a>(
    root: &'a AstNode<'a>,
    arena: &'a Arena<'a>,
    slugs: &SlugSet,
) -> WikilinkPass {
    let mut resolved: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // Collect every node that has children, so we can scan their direct child
    // runs. We snapshot the list first to avoid iterating while mutating.
    let parents: Vec<&'a AstNode<'a>> =
        root.descendants().filter(|n| n.first_child().is_some()).collect();

    for parent in parents {
        // Snapshot direct children.
        let children: Vec<&'a AstNode<'a>> = parent.children().collect();

        // Walk maximal runs of foldable (Text/HtmlInline) children, rebuild any
        // run that contains a complete `[[...]]`.
        let mut i = 0;
        while i < children.len() {
            if flat_source(children[i]).is_none() {
                i += 1;
                continue;
            }
            // Extend the foldable run.
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

            // Build replacement nodes from the combined run, inserted before the
            // first node of the run; then detach the whole run.
            let anchor = children[start];
            let mut rest = combined.as_str();
            let mut produced_any = false;
            while let Some(open) = rest.find("[[") {
                if let Some(close_rel) = rest[open + 2..].find("]]") {
                    let close = open + 2 + close_rel;
                    let before = &rest[..open];
                    let inner = &rest[open + 2..close];

                    if !before.is_empty() {
                        let n =
                            arena.alloc(AstNode::from(NodeValue::Text(before.to_string().into())));
                        anchor.insert_before(n);
                    }
                    let html = render_link(inner, slugs, &mut resolved, &mut seen);
                    let n = arena.alloc(AstNode::from(NodeValue::HtmlInline(html)));
                    anchor.insert_before(n);

                    rest = &rest[close + 2..];
                    produced_any = true;
                } else {
                    break; // unterminated `[[` — leave the remainder literal
                }
            }

            if produced_any {
                if !rest.is_empty() {
                    let n = arena.alloc(AstNode::from(NodeValue::Text(rest.to_string().into())));
                    anchor.insert_before(n);
                }
                for node in &children[start..i] {
                    node.detach();
                }
            }
        }
    }

    WikilinkPass { resolved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::comrak_options;
    use comrak::{parse_document, Arena};

    fn slugs() -> SlugSet {
        ["index", "guide/intro", "guide/Advanced", "reference/api"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn resolves_exact_slug() {
        assert_eq!(resolve_target("guide/intro", &slugs()), Some("guide/intro".to_string()));
    }

    #[test]
    fn resolves_basename_case_insensitive() {
        // "advanced" matches the basename of "guide/Advanced".
        assert_eq!(resolve_target("advanced", &slugs()), Some("guide/Advanced".to_string()));
        assert_eq!(resolve_target("INTRO", &slugs()), Some("guide/intro".to_string()));
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(resolve_target("  index  ", &slugs()), Some("index".to_string()));
    }

    #[test]
    fn unresolved_returns_none() {
        assert_eq!(resolve_target("does/not/exist", &slugs()), None);
        assert_eq!(resolve_target("", &slugs()), None);
    }

    #[test]
    fn parse_splits_label() {
        assert_eq!(parse_wikilink("target|Label"), ("target".to_string(), Some("Label".to_string())));
        assert_eq!(parse_wikilink("target"), ("target".to_string(), None));
        // Only the first pipe splits; extra pipes belong to the label.
        assert_eq!(parse_wikilink("a|b|c"), ("a".to_string(), Some("b|c".to_string())));
    }

    fn render(md: &str, slugs: &SlugSet) -> (String, Vec<String>) {
        let arena = Arena::new();
        let options = comrak_options();
        let root = parse_document(&arena, md, &options);
        let pass = transform_wikilinks(root, &arena, slugs);
        let html = crate::markdown::format_ast(root, &options);
        (html, pass.resolved)
    }

    #[test]
    fn resolved_wikilink_becomes_anchor() {
        let (html, resolved) = render("see [[guide/intro]] now\n", &slugs());
        assert!(html.contains(r#"<a class="docgen-wikilink" href="/guide/intro">guide/intro</a>"#));
        assert_eq!(resolved, vec!["guide/intro".to_string()]);
    }

    #[test]
    fn labeled_wikilink_uses_label_text() {
        let (html, _) = render("[[guide/intro|The Intro]]\n", &slugs());
        assert!(html.contains(r#"href="/guide/intro">The Intro</a>"#));
    }

    #[test]
    fn broken_wikilink_becomes_marked_span() {
        let (html, resolved) = render("[[nope]] here\n", &slugs());
        assert!(html.contains(
            r#"<span class="docgen-wikilink docgen-wikilink--broken" data-target="nope">nope</span>"#
        ));
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolved_targets_are_deduped_in_order() {
        let (_html, resolved) = render("[[guide/intro]] and [[index]] and [[intro]]\n", &slugs());
        // "intro" resolves to guide/intro (already present) -> deduped.
        assert_eq!(resolved, vec!["guide/intro".to_string(), "index".to_string()]);
    }

    #[test]
    fn empty_or_whitespace_label_falls_back_to_target() {
        // `[[index|]]` and `[[index|   ]]` must not render an empty clickable text;
        // they fall back to the target, matching the search-index unwrap path.
        let (html, _) = render("[[index|]]\n", &slugs());
        assert!(html.contains(r#"href="/index">index</a>"#));
        assert!(!html.contains(r#"href="/index"></a>"#));

        let (html, _) = render("[[index|   ]]\n", &slugs());
        assert!(html.contains(r#"href="/index">index</a>"#));

        // Broken target with empty label also falls back to the target text.
        let (html, _) = render("[[nope|]] x\n", &slugs());
        assert!(html.contains(r#"data-target="nope">nope</span>"#));
    }

    #[test]
    fn ambiguous_basename_resolves_deterministically() {
        // Two slugs share the basename `dup`; resolution is first by BTreeSet order.
        let amb: SlugSet =
            ["a/dup", "b/dup"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resolve_target("dup", &amb), Some("a/dup".to_string()));
    }

    #[test]
    fn html_special_chars_in_broken_target_are_escaped() {
        let (html, _) = render("[[a<b>]] x\n", &slugs());
        assert!(html.contains("data-target=\"a&lt;b&gt;\""));
        assert!(!html.contains("<b>"));
    }
}
