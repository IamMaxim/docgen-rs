# docgen-rs P1: Search + Syntax Highlight + Wikilinks/Backlinks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Use strict TDD: write the failing test, run it RED, implement, run it GREEN. Keep `cargo test` and `cargo clippy --all-targets` GREEN before declaring any task done.

**Goal:** Reach the P1 parity slice on top of the P0 core SSG:

1. **Syntax highlighting** of fenced code blocks, server-side at build (zero runtime JS) via comrak's `syntect` plugin.
2. **Wikilinks + backlinks**: resolve `[[target]]` / `[[target|label]]` against the known slug set, collect the link graph, build a backlinks map, render a "Backlinks" section per page, and mark unresolved links as broken (never crash).
3. **Search**: emit a static `dist/search-index.json` of `{slug, title, text}` per doc, plus a vendored self-contained JS Cmd/Ctrl-K modal, wired into the page template.

**Architecture impact — two-pass rendering.** P0's pipeline was one-shot per doc: `assemble(RawDoc) -> Doc` rendered markdown immediately. Wikilink resolution needs the full slug set *before* rendering any page, so P1 splits assembly into two phases:

- **Pass 1 — `prepare(RawDoc) -> PreparedDoc`**: parse frontmatter, derive slug/title, and capture the *raw markdown body* (NOT yet rendered to HTML). Pure, no cross-doc knowledge.
- **Pass 2 — `render_docs(Vec<PreparedDoc>) -> SiteBuild`**: build a `SlugSet`, then for each doc parse markdown to a comrak AST, run the wikilink transform pass (resolving against the `SlugSet`, recording graph edges), format the AST to HTML with the syntect plugin, and produce the final `Doc` plus the `LinkGraph` / `Backlinks` / `SearchIndex`.

The P0 `assemble` is preserved as a thin wrapper for backward-compatible unit tests but the build pipeline switches to `prepare` + `render_docs`.

**Tech Stack additions:** `comrak` `syntect` feature (built-in `SyntectAdapter`, `markdown_to_html_with_plugins` / `format_html_with_plugins`), `comrak::Arena` + `parse_document` for the AST pass, `serde_json` for the index. No new heavy deps; **no npm**. The search client JS is a vendored `.js` string emitted by the binary.

**Reference:** spec at `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`; P0 plan at `docs/superpowers/plans/2026-06-05-docgen-rs-p0-core-ssg.md`.

**Crate versions in tree (do not downgrade):** comrak 0.52, minijinja 2.20, syntect 5.3, serde_yml (present). Adapt to the ACTUAL current API; the compiler and crate source under `~/.cargo/registry/src/.../comrak-0.52.0/src` are the source of truth.

---

## Public API contract (defined once, used across all clusters)

These types/signatures are introduced in P1 and MUST stay consistent. New module files live in `crates/docgen-core/src/`.

```rust
// model.rs — additions

/// One resolved wikilink edge: `from` doc links to `to` doc (both slugs).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LinkEdge {
    pub from: String, // source slug
    pub to: String,   // target slug
}

/// Per-target inbound reference, for rendering a "Backlinks" section.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Backlink {
    pub slug: String,  // the linking doc's slug
    pub title: String, // the linking doc's title
}

/// One entry in the static search index.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchEntry {
    pub slug: String,
    pub title: String,
    pub text: String, // plaintext, frontmatter + markup stripped
}
```

```rust
// wikilink.rs — new module

use std::collections::{BTreeMap, BTreeSet};

/// The set of all known slugs, used to resolve wikilink targets.
pub type SlugSet = BTreeSet<String>;

/// Resolve a wikilink target string to a slug, given the known slug set.
/// Matching order: exact slug, then case-insensitive basename match.
/// Returns None when nothing matches (caller renders a broken link).
pub fn resolve_target(target: &str, slugs: &SlugSet) -> Option<String>;

/// Outcome of transforming one document's AST: the resolved outbound targets
/// (deduped, in document order) for graph/backlink construction.
pub struct WikilinkPass {
    pub resolved: Vec<String>, // target slugs this doc links to (deduped)
}
```

```rust
// graph.rs — new module

use std::collections::BTreeMap;
use crate::model::{Backlink, LinkEdge};

/// The full directed link graph plus the inverted backlinks map.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct LinkGraph {
    pub edges: Vec<LinkEdge>,                       // sorted, deduped
    pub backlinks: BTreeMap<String, Vec<Backlink>>, // to-slug -> inbound refs
}

/// Build a LinkGraph from per-doc resolved outbound targets.
/// `docs`: (slug, title) for every doc; `outbound`: slug -> resolved target slugs.
pub fn build_link_graph(
    docs: &[(String, String)],
    outbound: &BTreeMap<String, Vec<String>>,
) -> LinkGraph;
```

```rust
// markdown.rs — additions

/// Comrak Options used everywhere in P1 (GFM + the P0 extensions). Single source of truth.
pub fn comrak_options() -> comrak::Options<'static>;

/// Render a markdown body to HTML with syntect syntax highlighting (no wikilink pass).
/// Used where cross-doc resolution is not needed (back-compat `assemble`).
pub fn render_markdown(body: &str) -> String;

/// Default syntect theme name. One source of truth.
pub const SYNTECT_THEME: &str = "InspiredGitHub";
```

```rust
// pipeline.rs — new module (the two-pass orchestrator, pure/core-side)

use crate::graph::LinkGraph;
use crate::model::{Doc, RawDoc, SearchEntry};

/// A document after pass 1: frontmatter parsed, slug/title derived, raw body kept.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDoc {
    pub rel_path: String,
    pub slug: String,
    pub title: String,
    pub body_md: String, // raw markdown, frontmatter stripped, NOT rendered
}

/// Pass 1: pure per-doc preparation, no cross-doc knowledge.
pub fn prepare(raw: RawDoc) -> PreparedDoc;

/// The fully assembled site after pass 2.
pub struct SiteBuild {
    pub docs: Vec<Doc>,          // body_html now includes resolved wikilinks + highlight
    pub graph: LinkGraph,
    pub search: Vec<SearchEntry>,
}

/// Pass 2: build the slug set, run the wikilink AST pass + syntect highlight per doc,
/// assemble the link graph + search index. Deterministic, input order preserved.
pub fn render_docs(prepared: Vec<PreparedDoc>) -> SiteBuild;
```

```rust
// docgen-render additions (lib.rs)

/// PageContext gains backlinks + a search-enabled flag. Existing fields unchanged.
pub struct PageContext<'a> {
    pub title: &'a str,
    pub body_html: &'a str,
    pub tree: &'a [TreeNode],
    pub backlinks: &'a [Backlink], // NEW — rendered as a "Backlinks" section
}

/// The vendored search client script, emitted to dist/search.js by the binary.
pub const SEARCH_JS: &str = include_str!("../assets/search.js");

/// The syntect highlight CSS (theme is inline-styled by syntect, but we ship a small
/// wrapper stylesheet for the search modal + broken-link + backlinks styling).
pub const DOCGEN_CSS: &str = include_str!("../assets/docgen.css");
```

**Asset filenames (locked):** `dist/search-index.json`, `dist/search.js`, `dist/docgen.css`.

**New render context keys (locked):** `backlinks` (list of `{slug,title}`), `search_enabled` (bool, always true in P1), plus `<link>`/`<script>` references to `/docgen.css` and `/search.js` in the template `<head>`/end-of-`<body>`.

**Broken-link rendering (locked):** unresolved `[[x]]` renders as
`<span class="docgen-wikilink docgen-wikilink--broken" data-target="x">x</span>`.
Resolved `[[x|Label]]` renders as
`<a class="docgen-wikilink" href="/SLUG">Label</a>`.

---

# Cluster A — Syntax highlighting

Server-side fenced-code highlighting via comrak's built-in `SyntectAdapter`, wired through `markdown_to_html_with_plugins`. Theme: `InspiredGitHub` (a clean, light default present in syntect's `ThemeSet::load_defaults()`). This cluster is self-contained and does NOT yet touch the two-pass change — it upgrades the existing `render_markdown`.

### Task A1: Enable the comrak `syntect` feature

**Files:**
- Modify: `crates/docgen-core/Cargo.toml`

- [ ] **Step 1: Enable the feature (fetches current-compatible build of syntect via comrak)**

```bash
cargo add --package docgen-core comrak --features syntect
```

Confirm `crates/docgen-core/Cargo.toml` now lists `comrak` with `features = ["syntect"]`. Do NOT add a direct `syntect` dependency — comrak re-exports the adapter under `comrak::plugins::syntect`.

- [ ] **Step 2: Verify it still builds**

Run: `cargo build -p docgen-core`
Expected: PASS (syntect compiles; no code change yet).

- [ ] **Step 3: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "build(core): enable comrak syntect feature"
```

---

### Task A2: Highlighted `render_markdown` via the syntect plugin

**Files:**
- Modify: `crates/docgen-core/src/markdown.rs`

- [ ] **Step 1: Write the failing tests**

Append these tests to the existing `#[cfg(test)] mod tests` in `crates/docgen-core/src/markdown.rs` (keep all P0 tests):

```rust
    #[test]
    fn highlights_fenced_rust_code() {
        let md = "```rust\nfn main() { let x = 1; }\n```\n";
        let html = render_markdown(md);
        // Syntect emits inline-styled spans inside a <pre> wrapper.
        assert!(html.contains("<pre"));
        assert!(html.contains("style=\"color:"));
        // The keyword `fn` is highlighted as its own span, not left as plain text.
        assert!(html.contains("<span"));
    }

    #[test]
    fn unknown_language_does_not_crash_and_still_wraps_pre() {
        let md = "```not-a-real-lang\nplain text\n```\n";
        let html = render_markdown(md);
        assert!(html.contains("<pre"));
        assert!(html.contains("plain text"));
    }

    #[test]
    fn comrak_options_is_shared_source_of_truth() {
        // The shared options keep the P0 GFM extensions on.
        let html = render_markdown("~~gone~~\n");
        assert!(html.contains("<del>"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p docgen-core markdown`
Expected: FAIL — current `render_markdown` (plain `markdown_to_html`) does not emit `style="color:` spans.

- [ ] **Step 3: Implement shared options + highlighted render**

Replace the top of `crates/docgen-core/src/markdown.rs` (everything above `#[cfg(test)]`) with:

```rust
use comrak::plugins::syntect::SyntectAdapter;
use comrak::{markdown_to_html_with_plugins, Options, Plugins};

/// Default syntect theme. Single source of truth.
pub const SYNTECT_THEME: &str = "InspiredGitHub";

/// The comrak options used across the whole pipeline (GFM + P0 extensions).
/// Single source of truth so the AST pass (Cluster B) and the one-shot render agree.
pub fn comrak_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options
}

/// Render a markdown body (frontmatter already stripped) to HTML with GFM
/// extensions and server-side syntect syntax highlighting of fenced code.
pub fn render_markdown(body: &str) -> String {
    let options = comrak_options();
    let adapter = SyntectAdapter::new(Some(SYNTECT_THEME));
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);
    markdown_to_html_with_plugins(body, &options, &plugins)
}
```

> API notes (verified against comrak 0.52 source): `comrak::plugins::syntect::SyntectAdapter::new(Some(theme))`; `comrak::Plugins` has field `render: RenderPlugins`, and `RenderPlugins::codefence_syntax_highlighter: Option<&dyn SyntaxHighlighterAdapter>`. `markdown_to_html_with_plugins(md, &options, &plugins)` is the one-shot entry. `Options` is generic over a lifetime — `Options<'static>` is correct here.

- [ ] **Step 4: Run the tests green**

Run: `cargo test -p docgen-core markdown`
Expected: PASS — all P0 markdown tests plus the 3 new ones.

- [ ] **Step 5: Clippy + full core tests**

Run: `cargo clippy --all-targets -p docgen-core` then `cargo test -p docgen-core`
Expected: PASS, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): server-side syntect highlight + shared comrak options"
```

---

# Cluster B — Wikilinks + backlinks (two-pass rendering)

This cluster introduces the two-pass pipeline. Steps build bottom-up: model types → resolver → AST transform → graph builder → the `pipeline.rs` orchestrator → wire `build.rs`.

### Task B1: Model types for links + search

**Files:**
- Modify: `crates/docgen-core/src/model.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Add the new model types**

Append to `crates/docgen-core/src/model.rs`:

```rust
/// One resolved wikilink edge: `from` doc links to `to` doc (both slugs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkEdge {
    pub from: String,
    pub to: String,
}

/// Per-target inbound reference, for rendering a "Backlinks" section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backlink {
    pub slug: String,
    pub title: String,
}

/// One entry in the static search index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchEntry {
    pub slug: String,
    pub title: String,
    pub text: String,
}
```

- [ ] **Step 2: Re-export from `lib.rs`**

Update the `pub use` line in `crates/docgen-core/src/lib.rs`:

```rust
pub use model::{Backlink, Doc, LinkEdge, RawDoc, SearchEntry, TreeNode};
```

- [ ] **Step 3: Verify build**

Run: `cargo build -p docgen-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): add LinkEdge, Backlink, SearchEntry model types"
```

---

### Task B2: Wikilink target resolver (`wikilink.rs`)

**Files:**
- Create: `crates/docgen-core/src/wikilink.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/wikilink.rs`:

```rust
use std::collections::BTreeSet;

/// The set of all known slugs, used to resolve wikilink targets.
pub type SlugSet = BTreeSet<String>;

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-core wikilink`
Expected: FAIL — `resolve_target` / `parse_wikilink` undefined.

- [ ] **Step 3: Implement**

Add above the test module in `crates/docgen-core/src/wikilink.rs`:

```rust
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
```

> Note: the test `resolves_basename_case_insensitive` for `"INTRO"` relies on `guide/intro` being the only `intro` basename; keep the fixture slug set so exactly one basename matches per test target.

- [ ] **Step 4: Wire module**

Add `pub mod wikilink;` to `crates/docgen-core/src/lib.rs` (keep modules alphabetical: after `tree`? actual order in P0 is alphabetical — insert `pub mod wikilink;` last).

- [ ] **Step 5: Run GREEN**

Run: `cargo test -p docgen-core wikilink`
Expected: PASS — 5 tests.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): wikilink target resolver + parser"
```

---

### Task B3: AST wikilink transform pass

This is the heart of the two-pass change. We parse markdown into a comrak `Arena` AST, walk text nodes, replace `[[...]]` occurrences with raw-HTML inline nodes (resolved anchor or broken span), record resolved targets, then format the AST to HTML with the syntect plugin.

**Files:**
- Modify: `crates/docgen-core/src/wikilink.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/docgen-core/src/wikilink.rs` `tests` module:

```rust
    use crate::markdown::comrak_options;
    use comrak::{parse_document, Arena};

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
    fn html_special_chars_in_broken_target_are_escaped() {
        let (html, _) = render("[[a<b>]] x\n", &slugs());
        assert!(html.contains("data-target=\"a&lt;b&gt;\""));
        assert!(!html.contains("<b>"));
    }
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-core wikilink`
Expected: FAIL — `transform_wikilinks` / `format_ast` undefined.

- [ ] **Step 3: Implement `format_ast` in `markdown.rs`**

Add to `crates/docgen-core/src/markdown.rs` (above tests), reusing the shared adapter:

```rust
use comrak::nodes::AstNode;
use comrak::format_html_with_plugins;

/// Format an already-parsed (and possibly transformed) AST to HTML with syntect.
pub fn format_ast<'a>(root: &'a AstNode<'a>, options: &Options) -> String {
    let adapter = SyntectAdapter::new(Some(SYNTECT_THEME));
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);
    let mut out = Vec::new();
    format_html_with_plugins(root, options, &mut out, &plugins).expect("format AST to HTML");
    String::from_utf8(out).expect("comrak emits valid UTF-8")
}
```

> API notes (comrak 0.52): `comrak::format_html_with_plugins(root, &options, &mut writer, &plugins)` where writer is `impl Write` (a `Vec<u8>`). `AstNode<'a> = comrak::nodes::Node<'a, RefCell<Ast>>` is re-exported as `comrak::nodes::AstNode`. Add `use comrak::nodes::AstNode;` once.

- [ ] **Step 4: Implement `transform_wikilinks` in `wikilink.rs`**

Add above the test module:

```rust
use comrak::nodes::{AstNode, NodeValue};
use comrak::Arena;

/// Outcome of transforming one document's AST.
pub struct WikilinkPass {
    /// Target slugs this doc links to, deduped, in first-seen document order.
    pub resolved: Vec<String>,
}

/// Minimal HTML-attribute / text escaper for the small strings we inject.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the inline HTML for one wikilink occurrence and, if resolved, push its
/// target slug into `resolved` (deduped).
fn render_link(inner: &str, slugs: &SlugSet, resolved: &mut Vec<String>) -> String {
    let (target, label) = parse_wikilink(inner);
    match resolve_target(&target, slugs) {
        Some(slug) => {
            if !resolved.contains(&slug) {
                resolved.push(slug.clone());
            }
            let text = label.unwrap_or_else(|| target.clone());
            format!(
                r#"<a class="docgen-wikilink" href="/{}">{}</a>"#,
                esc(&slug),
                esc(&text)
            )
        }
        None => {
            let text = label.unwrap_or_else(|| target.clone());
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
pub fn transform_wikilinks<'a>(
    root: &'a AstNode<'a>,
    arena: &'a Arena<AstNode<'a>>,
    slugs: &SlugSet,
) -> WikilinkPass {
    let mut resolved: Vec<String> = Vec::new();

    // Collect text nodes first (avoid mutating while iterating).
    let text_nodes: Vec<&'a AstNode<'a>> = root
        .descendants()
        .filter(|n| matches!(n.data.borrow().value, NodeValue::Text(_)))
        .collect();

    for node in text_nodes {
        let text = match &node.data.borrow().value {
            NodeValue::Text(t) => t.to_string(),
            _ => continue,
        };
        if !text.contains("[[") {
            continue;
        }

        // Parse `text` into alternating literal / wikilink segments and build
        // replacement nodes, inserted before `node`, then detach `node`.
        let mut rest = text.as_str();
        let mut produced_any = false;
        while let Some(open) = rest.find("[[") {
            if let Some(close_rel) = rest[open + 2..].find("]]") {
                let close = open + 2 + close_rel;
                let before = &rest[..open];
                let inner = &rest[open + 2..close];

                if !before.is_empty() {
                    let n = arena.alloc(AstNode::from(NodeValue::Text(before.to_string())));
                    node.insert_before(n);
                }
                let html = render_link(inner, slugs, &mut resolved);
                let n = arena.alloc(AstNode::from(NodeValue::HtmlInline(html)));
                node.insert_before(n);

                rest = &rest[close + 2..];
                produced_any = true;
            } else {
                break; // unterminated `[[` — leave the remainder literal
            }
        }

        if produced_any {
            if !rest.is_empty() {
                let n = arena.alloc(AstNode::from(NodeValue::Text(rest.to_string())));
                node.insert_before(n);
            }
            node.detach();
        }
    }

    WikilinkPass { resolved }
}
```

> API notes (comrak 0.52): `Arena<AstNode<'a>>` allocates nodes; `arena.alloc(AstNode::from(value))` — `AstNode` implements `From<NodeValue>` (constructs a node with default sourcepos). `node.insert_before(new)`, `node.detach()`, and `node.descendants()` come from `comrak::arena_tree::Node`. `node.data` is a `RefCell<Ast>`; `node.data.borrow().value` is the `NodeValue`. `NodeValue::Text(String)` and `NodeValue::HtmlInline(String)` are the variants used. If `AstNode::from` is unavailable on the exact build, use the node constructor the compiler points to (e.g. `Ast::new(value, sourcepos)` wrapped) — let the compiler guide the exact constructor; the transform logic is unchanged.

- [ ] **Step 5: Run GREEN**

Run: `cargo test -p docgen-core wikilink`
Expected: PASS — all wikilink tests (resolver + transform).

- [ ] **Step 6: Clippy**

Run: `cargo clippy --all-targets -p docgen-core`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): AST wikilink transform pass + format_ast"
```

---

### Task B4: Link graph + backlinks builder (`graph.rs`)

**Files:**
- Create: `crates/docgen-core/src/graph.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/graph.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn builds_edges_and_inverted_backlinks() {
        let docs = vec![
            ("index".to_string(), "Home".to_string()),
            ("a".to_string(), "Page A".to_string()),
            ("b".to_string(), "Page B".to_string()),
        ];
        let mut outbound: BTreeMap<String, Vec<String>> = BTreeMap::new();
        outbound.insert("a".into(), vec!["index".into(), "b".into()]);
        outbound.insert("b".into(), vec!["index".into()]);

        let g = build_link_graph(&docs, &outbound);

        assert_eq!(
            g.edges,
            vec![
                LinkEdge { from: "a".into(), to: "b".into() },
                LinkEdge { from: "a".into(), to: "index".into() },
                LinkEdge { from: "b".into(), to: "index".into() },
            ]
        );
        // index is linked from a and b (sorted by linking slug).
        assert_eq!(
            g.backlinks.get("index").unwrap(),
            &vec![
                Backlink { slug: "a".into(), title: "Page A".into() },
                Backlink { slug: "b".into(), title: "Page B".into() },
            ]
        );
        assert_eq!(
            g.backlinks.get("b").unwrap(),
            &vec![Backlink { slug: "a".into(), title: "Page A".into() }]
        );
        assert!(g.backlinks.get("a").is_none());
    }

    #[test]
    fn self_links_are_dropped() {
        let docs = vec![("a".to_string(), "A".to_string())];
        let mut outbound = BTreeMap::new();
        outbound.insert("a".to_string(), vec!["a".to_string()]);
        let g = build_link_graph(&docs, &outbound);
        assert!(g.edges.is_empty());
        assert!(g.backlinks.is_empty());
    }
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-core graph`
Expected: FAIL — `build_link_graph` / `LinkGraph` undefined.

- [ ] **Step 3: Implement**

Add above the test module:

```rust
use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Backlink, LinkEdge};

/// The full directed link graph plus the inverted backlinks map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct LinkGraph {
    pub edges: Vec<LinkEdge>,
    pub backlinks: BTreeMap<String, Vec<Backlink>>,
}

/// Build a LinkGraph from per-doc resolved outbound targets.
/// `docs`: (slug, title) for every doc. `outbound`: slug -> resolved target slugs.
/// Self-links are dropped. Edges are sorted (from, to); backlink lists sorted by
/// linking slug. Deterministic.
pub fn build_link_graph(
    docs: &[(String, String)],
    outbound: &BTreeMap<String, Vec<String>>,
) -> LinkGraph {
    let title_of: BTreeMap<&str, &str> =
        docs.iter().map(|(s, t)| (s.as_str(), t.as_str())).collect();

    let mut edges: Vec<LinkEdge> = Vec::new();
    let mut backlinks: BTreeMap<String, Vec<Backlink>> = BTreeMap::new();

    for (from, targets) in outbound {
        for to in targets {
            if to == from {
                continue;
            }
            edges.push(LinkEdge { from: from.clone(), to: to.clone() });
            let title = title_of.get(from.as_str()).copied().unwrap_or(from.as_str());
            backlinks
                .entry(to.clone())
                .or_default()
                .push(Backlink { slug: from.clone(), title: title.to_string() });
        }
    }

    edges.sort();
    edges.dedup();
    for list in backlinks.values_mut() {
        list.sort();
        list.dedup();
    }

    LinkGraph { edges, backlinks }
}
```

> `LinkEdge` and `Backlink` derive `Ord`? They derive `PartialEq, Eq` in B1. Add `PartialOrd, Ord` to BOTH derives in `model.rs` so `edges.sort()` / `list.sort()` work. Update B1's derive lines to `#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]` for `LinkEdge` and `Backlink` (leave `SearchEntry` as is). If B1 is already committed, make this a one-line edit here and note it.

- [ ] **Step 4: Wire module**

Add `pub mod graph;` to `lib.rs` and re-export: `pub use graph::LinkGraph;`.

- [ ] **Step 5: Run GREEN + clippy**

Run: `cargo test -p docgen-core graph` then `cargo clippy --all-targets -p docgen-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): link graph + inverted backlinks builder"
```

---

### Task B5: Plaintext extraction for search (`search.rs`)

We extract searchable plaintext from the markdown body (frontmatter already stripped at prepare-time). Strip markup by rendering to HTML once with NO plugins and then stripping tags — OR simpler and robust: walk the comrak AST collecting `Text`/`Code` node contents. We use the AST walk to avoid HTML entity noise. This module is shared by Cluster C but lives here because it consumes the same AST machinery.

**Files:**
- Create: `crates/docgen-core/src/search.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/search.rs`:

```rust
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
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-core search`
Expected: FAIL — `plaintext` undefined.

- [ ] **Step 3: Implement**

Add above the test module:

```rust
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
            NodeValue::Text(t) => push_unwrapping_wikilinks(&mut buf, t),
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
                Some((t, label)) if !label.trim().is_empty() => label.trim(),
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
```

> Note: comrak parses `[link](/x)` into a `Link` node whose visible child is a `Text("link")` node — so the link *text* is collected but the URL is not. Inline `Code` literal lives at `NodeValue::Code(NodeCode).literal`. Verify field name against comrak 0.52 (`NodeCode { literal: String, .. }`); adjust if the compiler names it differently.

- [ ] **Step 4: Wire + GREEN + clippy**

Add `pub mod search;` to `lib.rs`. Run: `cargo test -p docgen-core search` then `cargo clippy --all-targets -p docgen-core`.
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): plaintext extraction for search index"
```

---

### Task B6: Two-pass pipeline orchestrator (`pipeline.rs`)

Ties B2–B5 together: pass 1 `prepare`, pass 2 `render_docs` building `SiteBuild { docs, graph, search }`.

**Files:**
- Create: `crates/docgen-core/src/pipeline.rs`
- Modify: `crates/docgen-core/src/lib.rs`
- Modify: `crates/docgen-core/src/assemble.rs` (make `assemble` delegate to `prepare` + single-doc render, for back-compat)

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;

    fn raw(path: &str, body: &str) -> RawDoc {
        RawDoc { rel_path: path.into(), raw: body.into() }
    }

    #[test]
    fn prepare_keeps_raw_body_and_derives_meta() {
        let p = prepare(raw("guide/intro.md", "---\ntitle: Intro\n---\n# H\nbody [[index]]\n"));
        assert_eq!(p.slug, "guide/intro");
        assert_eq!(p.title, "Intro");
        assert!(p.body_md.contains("[[index]]"));
        assert!(!p.body_md.contains("title:")); // frontmatter stripped
    }

    #[test]
    fn render_docs_resolves_links_highlights_and_indexes() {
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw("guide/intro.md", "# Intro\n```rust\nfn x(){}\n```\nBack to [[index]] and [[ghost]].\n")),
        ];
        let site = render_docs(prepared);

        // Doc order preserved.
        assert_eq!(site.docs[0].slug, "index");
        assert_eq!(site.docs[1].slug, "guide/intro");

        // index links to guide/intro (resolved anchor).
        assert!(site.docs[0].body_html.contains(r#"href="/guide/intro""#));
        // intro has highlighted code + a resolved link + a broken span.
        assert!(site.docs[1].body_html.contains("style=\"color:"));
        assert!(site.docs[1].body_html.contains(r#"href="/index""#));
        assert!(site.docs[1].body_html.contains("docgen-wikilink--broken"));

        // Graph: index->guide/intro and guide/intro->index (ghost dropped).
        assert!(site.graph.edges.iter().any(|e| e.from == "index" && e.to == "guide/intro"));
        assert!(site.graph.edges.iter().any(|e| e.from == "guide/intro" && e.to == "index"));
        assert!(!site.graph.edges.iter().any(|e| e.to == "ghost"));

        // Backlinks: index is linked from guide/intro.
        assert_eq!(site.graph.backlinks.get("index").unwrap()[0].slug, "guide/intro");

        // Search index: one entry per doc, plaintext, no markup.
        assert_eq!(site.search.len(), 2);
        let home = site.search.iter().find(|e| e.slug == "index").unwrap();
        assert_eq!(home.title, "Home");
        assert!(home.text.contains("Go to"));
        assert!(!home.text.contains("[["));
    }
}
```

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-core pipeline`
Expected: FAIL — `prepare` / `render_docs` / `PreparedDoc` / `SiteBuild` undefined.

- [ ] **Step 3: Implement**

Add above the test module:

```rust
use std::collections::BTreeMap;

use comrak::{parse_document, Arena};

use crate::frontmatter::parse_frontmatter;
use crate::graph::{build_link_graph, LinkGraph};
use crate::markdown::{comrak_options, format_ast};
use crate::model::{Doc, RawDoc, SearchEntry};
use crate::search::plaintext;
use crate::wikilink::{transform_wikilinks, SlugSet};

/// A document after pass 1: frontmatter parsed, slug/title derived, raw body kept.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDoc {
    pub rel_path: String,
    pub slug: String,
    pub title: String,
    pub body_md: String,
}

/// The fully assembled site after pass 2.
pub struct SiteBuild {
    pub docs: Vec<Doc>,
    pub graph: LinkGraph,
    pub search: Vec<SearchEntry>,
}

fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Pass 1: pure per-doc preparation, no cross-doc knowledge.
pub fn prepare(raw: RawDoc) -> PreparedDoc {
    let parsed = parse_frontmatter(&raw.raw);
    let slug = raw
        .rel_path
        .strip_suffix(".md")
        .unwrap_or(&raw.rel_path)
        .to_string();

    let fm_title = parsed
        .frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let title = fm_title
        .or_else(|| first_h1(&parsed.body))
        .unwrap_or_else(|| slug.rsplit('/').next().unwrap_or("").to_string());

    PreparedDoc { rel_path: raw.rel_path, slug, title, body_md: parsed.body }
}

/// Pass 2: build the slug set, run the wikilink pass + syntect highlight per doc,
/// assemble the link graph + search index. Input order preserved.
pub fn render_docs(prepared: Vec<PreparedDoc>) -> SiteBuild {
    let slugs: SlugSet = prepared.iter().map(|p| p.slug.clone()).collect();
    let doc_meta: Vec<(String, String)> =
        prepared.iter().map(|p| (p.slug.clone(), p.title.clone())).collect();
    let options = comrak_options();

    let mut docs = Vec::with_capacity(prepared.len());
    let mut outbound: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut search = Vec::with_capacity(prepared.len());

    for p in &prepared {
        // Search plaintext from the raw body (independent arena).
        search.push(SearchEntry {
            slug: p.slug.clone(),
            title: p.title.clone(),
            text: plaintext(&p.body_md),
        });

        // Wikilink AST pass + highlighted HTML.
        let arena = Arena::new();
        let root = parse_document(&arena, &p.body_md, &options);
        let pass = transform_wikilinks(root, &arena, &slugs);
        outbound.insert(p.slug.clone(), pass.resolved);
        let body_html = format_ast(root, &options);

        docs.push(Doc {
            rel_path: p.rel_path.clone(),
            slug: p.slug.clone(),
            title: p.title.clone(),
            body_html,
        });
    }

    let graph = build_link_graph(&doc_meta, &outbound);
    SiteBuild { docs, graph, search }
}
```

- [ ] **Step 4: Make `assemble` delegate (back-compat)**

Replace the body of `assemble` in `crates/docgen-core/src/assemble.rs` so its existing P0 tests still pass, now routed through the two-pass code (single doc, empty cross-doc set):

```rust
use crate::model::{Doc, RawDoc};
use crate::pipeline::{prepare, render_docs};

/// Derive a URL slug from a docs-relative path: strip a trailing `.md`.
pub fn slug_for(rel_path: &str) -> String {
    rel_path.strip_suffix(".md").unwrap_or(rel_path).to_string()
}

/// Process a single RawDoc into a renderable Doc (back-compat single-doc path).
/// Wikilinks resolve only against this one doc's slug, so cross-doc links render
/// broken here — full resolution happens via `pipeline::render_docs`.
pub fn assemble(raw: RawDoc) -> Doc {
    let prepared = prepare(raw);
    render_docs(vec![prepared]).docs.pop().expect("one doc in, one doc out")
}
```

> Keep `assemble.rs`'s existing `#[cfg(test)] mod tests` unchanged. The `title_*`/`slug_*` tests still hold because `prepare` mirrors P0's title/slug logic. Remove the now-unused `first_h1` / `parse_frontmatter` / `render_markdown` imports from `assemble.rs` to avoid dead-code/clippy warnings.

- [ ] **Step 5: Wire module + GREEN**

Add `pub mod pipeline;` to `lib.rs` and re-export: `pub use pipeline::{prepare, render_docs, PreparedDoc, SiteBuild};`.

Run: `cargo test -p docgen-core` then `cargo clippy --all-targets -p docgen-core`
Expected: PASS — pipeline tests + unchanged assemble/markdown/wikilink/graph/search tests, no warnings.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): two-pass pipeline (prepare + render_docs) with links, graph, search"
```

---

### Task B7: Backlinks in the render context + template

**Files:**
- Modify: `crates/docgen-render/src/lib.rs`
- Modify: `crates/docgen-render/templates/page.html`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` in `crates/docgen-render/src/lib.rs`:

```rust
    #[test]
    fn renders_backlinks_section() {
        use docgen_core::model::Backlink;
        let backlinks = vec![Backlink { slug: "a".into(), title: "Page A".into() }];
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                body_html: "",
                tree: &[],
                backlinks: &backlinks,
            })
            .unwrap();
        assert!(html.contains("Backlinks"));
        assert!(html.contains(r#"href="/a""#));
        assert!(html.contains(">Page A</a>"));
    }

    #[test]
    fn omits_backlinks_section_when_empty() {
        let html = renderer()
            .render_page(&PageContext { title: "X", body_html: "", tree: &[], backlinks: &[] })
            .unwrap();
        assert!(!html.contains("Backlinks"));
    }
```

Also update the three existing render tests to pass `backlinks: &[]` in their `PageContext` literals (compilation will force this).

- [ ] **Step 2: Run RED**

Run: `cargo test -p docgen-render`
Expected: FAIL — `PageContext` has no `backlinks` field.

- [ ] **Step 3: Add the field + context key**

In `crates/docgen-render/src/lib.rs`, add `pub backlinks: &'a [Backlink]` to `PageContext` (import `use docgen_core::model::{Backlink, TreeNode};`), and pass it in `render_page`:

```rust
        tmpl.render(context! {
            title => ctx.title,
            body => ctx.body_html,
            tree => ctx.tree,
            backlinks => ctx.backlinks,
            search_enabled => true,
        })
```

- [ ] **Step 4: Render the Backlinks section + assets in `page.html`**

In `crates/docgen-render/templates/page.html`, add to `<head>` (after the `<title>`):

```html
  <link rel="stylesheet" href="/docgen.css" />
```

After the `{{ body | safe }}` block inside `<main>`, add:

```html
    {% if backlinks %}
    <section class="docgen-backlinks">
      <h2>Backlinks</h2>
      <ul>
        {% for bl in backlinks %}
          <li><a href="/{{ bl.slug | safe }}">{{ bl.title }}</a></li>
        {% endfor %}
      </ul>
    </section>
    {% endif %}
```

And just before `</body>` add the search trigger + script (Cluster C wires the behavior; the markup is inert until `search.js` loads):

```html
  {% if search_enabled %}
  <button class="docgen-search-trigger" data-docgen-search>Search <kbd>Ctrl K</kbd></button>
  <script src="/search.js" defer></script>
  {% endif %}
```

- [ ] **Step 5: GREEN + clippy**

Run: `cargo test -p docgen-render` then `cargo clippy --all-targets -p docgen-render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(render): backlinks section + asset/search wiring in page template"
```

---

### Task B8: Switch `build.rs` to the two-pass pipeline + per-page backlinks

**Files:**
- Modify: `crates/docgen/src/build.rs`

- [ ] **Step 1: Rewrite the build orchestration**

Replace `crates/docgen/src/build.rs`'s `build` to use `prepare` + `render_docs`, look up per-doc backlinks, and (Cluster C will add the JSON/JS emit; leave a TODO marker only if implementing B before C — but prefer to land C's emit in C8). Body:

```rust
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use docgen_core::discover::discover_docs;
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

pub fn build(project_root: &Path) -> Result<()> {
    let docs_dir = project_root.join("docs");
    let dist_dir = project_root.join("dist");

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Two-pass: prepare all, then render with full slug knowledge.
    let prepared: Vec<_> = raws.into_iter().map(prepare).collect();
    let site = render_docs(prepared);
    let tree = build_tree(&site.docs);

    let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;

    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    let empty: Vec<docgen_core::model::Backlink> = Vec::new();
    for doc in &site.docs {
        let backlinks = site.graph.backlinks.get(&doc.slug).unwrap_or(&empty);
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            body_html: &doc.body_html,
            tree: &tree,
            backlinks,
        })?;
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), html)?;
    }

    // Cluster C adds: emit search-index.json, search.js, docgen.css here.
    println!("Built {} page(s) -> {}", site.docs.len(), dist_dir.display());
    Ok(())
}
```

- [ ] **Step 2: Build + run existing CLI test**

Run: `cargo build -p docgen` then `cargo test -p docgen --test build_cli`
Expected: PASS — the P0 fixture still builds; sidebar links unchanged. (The P0 fixture's `[[wikilink]]` now renders as a broken span since there's no `wikilink` doc — update the fixture in B9.)

- [ ] **Step 3: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(cli): build via two-pass pipeline with per-page backlinks"
```

---

### Task B9: Wikilink/backlink end-to-end fixture + CLI assertions

**Files:**
- Modify: `fixtures/site-basic/docs/index.md`
- Modify: `fixtures/site-basic/docs/guide/intro.md`
- Modify: `crates/docgen/tests/build_cli.rs`

- [ ] **Step 1: Update the fixture so a wikilink resolves**

Set `fixtures/site-basic/docs/index.md`:

```markdown
---
title: Home
---

# Welcome

This is the **basic** fixture site. See the [[guide/intro|Intro guide]].
```

Set `fixtures/site-basic/docs/guide/intro.md`:

```markdown
# Introduction

Some intro prose linking back to [[index]] and a [[missing-page]] one.

```rust
fn main() { println!("hi"); }
```
```

- [ ] **Step 2: Extend the CLI test**

Add assertions to `builds_fixture_site` in `crates/docgen/tests/build_cli.rs` (after the existing reads). Note the test copies fixtures into a temp dir — copy the (now-updated) files; the existing copy lines already cover both files. Append:

```rust
    // Resolved wikilink on the home page.
    assert!(home.contains(r#"<a class="docgen-wikilink" href="/guide/intro">Intro guide</a>"#));

    // Intro page: resolved backlink target, broken wikilink, highlighted code.
    assert!(intro.contains(r#"href="/index""#));
    assert!(intro.contains("docgen-wikilink--broken"));
    assert!(intro.contains("style=\"color:")); // syntect highlight

    // Backlinks section: intro links to index, so index's page lists intro as a backlink.
    assert!(home.contains("Backlinks"));
    assert!(home.contains(r#"href="/guide/intro""#));
```

> The earlier `home`/`intro` bindings are reused. Ensure the test still reads `home` before asserting on it.

- [ ] **Step 3: GREEN**

Run: `cargo test -p docgen --test build_cli`
Expected: PASS.

- [ ] **Step 4: Full suite + clippy**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "test(cli): end-to-end wikilinks, backlinks, highlight on fixture"
```

---

# Cluster C — Search (static index + vendored Cmd/Ctrl-K client)

Emit `dist/search-index.json` from `SiteBuild.search`, ship a self-contained `dist/search.js` (vendored, no npm) implementing a Cmd/Ctrl-K modal with substring + light fuzzy ranking, and a minimal `dist/docgen.css`. The template trigger/script tag was added in B7.

### Task C1: Serialize the search index to JSON (core)

**Files:**
- Modify: `crates/docgen-core/src/search.rs`
- Modify: `crates/docgen-core/Cargo.toml`

- [ ] **Step 1: Add serde_json**

```bash
cargo add --package docgen-core serde_json
```

- [ ] **Step 2: Write the failing test**

Add to `crates/docgen-core/src/search.rs` tests:

```rust
    #[test]
    fn serializes_index_to_json_array() {
        use crate::model::SearchEntry;
        let entries = vec![
            SearchEntry { slug: "a".into(), title: "A".into(), text: "alpha".into() },
            SearchEntry { slug: "b".into(), title: "B".into(), text: "beta".into() },
        ];
        let json = index_json(&entries);
        assert!(json.starts_with('['));
        assert!(json.contains(r#""slug":"a""#));
        assert!(json.contains(r#""title":"A""#));
        assert!(json.contains(r#""text":"alpha""#));
    }
```

- [ ] **Step 3: Implement**

Add to `search.rs` (above tests):

```rust
use crate::model::SearchEntry;

/// Serialize the search index to a compact JSON array for `dist/search-index.json`.
pub fn index_json(entries: &[SearchEntry]) -> String {
    serde_json::to_string(entries).expect("SearchEntry serializes")
}
```

- [ ] **Step 4: GREEN + clippy**

Run: `cargo test -p docgen-core search` then `cargo clippy --all-targets -p docgen-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(core): serialize search index to JSON"
```

---

### Task C2: Vendored search client JS (`search.js`)

**Files:**
- Create: `crates/docgen-render/assets/search.js`
- Create: `crates/docgen-render/assets/docgen.css`
- Modify: `crates/docgen-render/src/lib.rs`

- [ ] **Step 1: Write the vendored client script**

Create `crates/docgen-render/assets/search.js` — plain, self-contained, no deps:

```javascript
/* docgen search: Cmd/Ctrl-K modal over /search-index.json. No deps, no npm. */
(function () {
  "use strict";
  var index = null;
  var loading = null;

  function loadIndex() {
    if (index) return Promise.resolve(index);
    if (loading) return loading;
    loading = fetch("/search-index.json")
      .then(function (r) { return r.json(); })
      .then(function (data) { index = data; return index; });
    return loading;
  }

  // Substring score; lower is better. -1 means no match.
  function score(entry, q) {
    var hay = (entry.title + " " + entry.text).toLowerCase();
    var i = hay.indexOf(q);
    if (i === -1) return -1;
    // Prefer title hits and earlier positions.
    var titleHit = entry.title.toLowerCase().indexOf(q) !== -1 ? 0 : 1000;
    return titleHit + i;
  }

  function search(q) {
    q = q.trim().toLowerCase();
    if (!q || !index) return [];
    return index
      .map(function (e) { return { e: e, s: score(e, q) }; })
      .filter(function (x) { return x.s >= 0; })
      .sort(function (a, b) { return a.s - b.s; })
      .slice(0, 20)
      .map(function (x) { return x.e; });
  }

  var modal, input, list, selected = 0, results = [];

  function buildModal() {
    modal = document.createElement("div");
    modal.className = "docgen-search-modal";
    modal.setAttribute("hidden", "");
    modal.innerHTML =
      '<div class="docgen-search-backdrop" data-close></div>' +
      '<div class="docgen-search-box" role="dialog" aria-modal="true" aria-label="Search">' +
      '<input class="docgen-search-input" type="text" placeholder="Search docs..." aria-label="Search docs" />' +
      '<ul class="docgen-search-results"></ul></div>';
    document.body.appendChild(modal);
    input = modal.querySelector(".docgen-search-input");
    list = modal.querySelector(".docgen-search-results");

    input.addEventListener("input", function () { render(search(input.value)); });
    input.addEventListener("keydown", onKey);
    modal.addEventListener("click", function (ev) {
      if (ev.target.hasAttribute("data-close")) close();
    });
  }

  function render(rs) {
    results = rs; selected = 0;
    list.innerHTML = "";
    rs.forEach(function (e, i) {
      var li = document.createElement("li");
      li.className = "docgen-search-result" + (i === 0 ? " is-selected" : "");
      li.innerHTML = '<a href="/' + e.slug + '"><span class="title"></span></a>';
      li.querySelector(".title").textContent = e.title;
      li.addEventListener("mouseenter", function () { select(i); });
      list.appendChild(li);
    });
  }

  function select(i) {
    if (!results.length) return;
    selected = (i + results.length) % results.length;
    var items = list.querySelectorAll(".docgen-search-result");
    items.forEach(function (el, idx) { el.classList.toggle("is-selected", idx === selected); });
  }

  function go() {
    if (results[selected]) window.location.href = "/" + results[selected].slug;
  }

  function onKey(ev) {
    if (ev.key === "ArrowDown") { ev.preventDefault(); select(selected + 1); }
    else if (ev.key === "ArrowUp") { ev.preventDefault(); select(selected - 1); }
    else if (ev.key === "Enter") { ev.preventDefault(); go(); }
    else if (ev.key === "Escape") { close(); }
  }

  function open() {
    if (!modal) buildModal();
    loadIndex().then(function () { render(search(input.value)); });
    modal.removeAttribute("hidden");
    input.value = ""; list.innerHTML = "";
    input.focus();
  }
  function close() { if (modal) modal.setAttribute("hidden", ""); }

  document.addEventListener("keydown", function (ev) {
    if ((ev.metaKey || ev.ctrlKey) && (ev.key === "k" || ev.key === "K")) {
      ev.preventDefault(); open();
    }
  });
  document.addEventListener("click", function (ev) {
    var t = ev.target.closest("[data-docgen-search]");
    if (t) { ev.preventDefault(); open(); }
  });
})();
```

- [ ] **Step 2: Write the minimal stylesheet**

Create `crates/docgen-render/assets/docgen.css`:

```css
/* docgen P1 styles: wikilinks, backlinks, search modal. */
.docgen-wikilink--broken { color: #b00020; text-decoration: underline dotted; cursor: help; }
.docgen-backlinks { margin-top: 2rem; border-top: 1px solid #ddd; padding-top: 1rem; }
.docgen-backlinks h2 { font-size: 1rem; }

.docgen-search-trigger { position: fixed; top: 1rem; right: 1rem; }
.docgen-search-modal[hidden] { display: none; }
.docgen-search-modal { position: fixed; inset: 0; z-index: 1000; }
.docgen-search-backdrop { position: absolute; inset: 0; background: rgba(0,0,0,.4); }
.docgen-search-box { position: relative; max-width: 36rem; margin: 10vh auto 0;
  background: #fff; border-radius: 8px; padding: .75rem; box-shadow: 0 10px 40px rgba(0,0,0,.3); }
.docgen-search-input { width: 100%; font-size: 1.1rem; padding: .6rem; box-sizing: border-box; }
.docgen-search-results { list-style: none; margin: .5rem 0 0; padding: 0; max-height: 50vh; overflow: auto; }
.docgen-search-result a { display: block; padding: .4rem .5rem; text-decoration: none; color: inherit; }
.docgen-search-result.is-selected { background: #eef; }
```

- [ ] **Step 3: Embed both assets via `include_str!`**

Add to `crates/docgen-render/src/lib.rs` (near `DEFAULT_PAGE_TEMPLATE`):

```rust
/// The vendored search client script, emitted to `dist/search.js`.
pub const SEARCH_JS: &str = include_str!("../assets/search.js");

/// Minimal stylesheet for wikilinks/backlinks/search, emitted to `dist/docgen.css`.
pub const DOCGEN_CSS: &str = include_str!("../assets/docgen.css");
```

- [ ] **Step 4: Add a smoke test that the constants are non-empty + self-contained**

Add to `crates/docgen-render/src/lib.rs` tests:

```rust
    #[test]
    fn ships_self_contained_search_assets() {
        assert!(SEARCH_JS.contains("search-index.json"));
        assert!(SEARCH_JS.contains("metaKey"));
        assert!(!SEARCH_JS.contains("import ")); // no module imports / npm
        assert!(DOCGEN_CSS.contains("docgen-search-modal"));
    }
```

- [ ] **Step 5: GREEN + clippy**

Run: `cargo test -p docgen-render` then `cargo clippy --all-targets -p docgen-render`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(render): vendored search.js + docgen.css assets"
```

---

### Task C3: Emit search index + assets in `build.rs`

**Files:**
- Modify: `crates/docgen/src/build.rs`

- [ ] **Step 1: Emit JSON + JS + CSS**

In `crates/docgen/src/build.rs`, replace the `// Cluster C adds:` comment line with:

```rust
    // Static search index + vendored client assets.
    fs::write(
        dist_dir.join("search-index.json"),
        docgen_core::search::index_json(&site.search),
    )?;
    fs::write(dist_dir.join("search.js"), docgen_render::SEARCH_JS)?;
    fs::write(dist_dir.join("docgen.css"), docgen_render::DOCGEN_CSS)?;
```

- [ ] **Step 2: Build**

Run: `cargo build -p docgen`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "feat(cli): emit search-index.json, search.js, docgen.css"
```

---

### Task C4: End-to-end search assertions

**Files:**
- Modify: `crates/docgen/tests/build_cli.rs`

- [ ] **Step 1: Write the failing assertions**

Append to `builds_fixture_site` in `crates/docgen/tests/build_cli.rs`:

```rust
    // Search index emitted with one entry per doc, plaintext, no markup.
    let idx = fs::read_to_string(tmp.join("dist/search-index.json")).unwrap();
    assert!(idx.contains(r#""slug":"index""#));
    assert!(idx.contains(r#""slug":"guide/intro""#));
    assert!(idx.contains(r#""title":"Home""#));
    assert!(!idx.contains("[[")); // wikilink brackets stripped from indexed text
    assert!(!idx.contains("<")); // no HTML markup in indexed text

    // Vendored client assets emitted.
    let js = fs::read_to_string(tmp.join("dist/search.js")).unwrap();
    assert!(js.contains("search-index.json"));
    assert!(tmp.join("dist/docgen.css").exists());

    // Template wires the search trigger + script.
    assert!(home.contains("data-docgen-search"));
    assert!(home.contains(r#"src="/search.js""#));
```

- [ ] **Step 2: Run RED then GREEN**

Run: `cargo test -p docgen --test build_cli`
Expected: PASS (assets/index emitted from C3, template wiring from B7).

- [ ] **Step 3: Full suite + clippy**

Run: `cargo test` then `cargo clippy --all-targets`
Expected: PASS, no warnings.

- [ ] **Step 4: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "test(cli): end-to-end search index + asset emission"
```

---

### Task C5: README + manual smoke test

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the Status section** to note P1 (syntax highlight, wikilinks/backlinks, search) is done.

- [ ] **Step 2: Manual smoke test**

```bash
cargo run -p docgen -- build fixtures/site-basic
```

Expected: prints `Built 2 page(s) -> fixtures/site-basic/dist`. Open `fixtures/site-basic/dist/index/index.html`: code blocks are colorized, the `[[guide/intro|Intro guide]]` link works, a "Backlinks" section lists the intro page, the broken `[[missing-page]]` shows a dotted-underline span, and Ctrl/Cmd-K opens a working search modal that navigates to the chosen slug. `dist/search-index.json`, `dist/search.js`, `dist/docgen.css` all exist.

- [ ] **Step 3: Commit**

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -am "docs: P1 README update (highlight, wikilinks, backlinks, search)"
```

---

## Self-Review

**Spec coverage (P1 scope):** Covers the three P1 parity items from the spec's phasing — `syntect` highlighting (Cluster A), wikilinks/backlinks via a clean two-pass AST pipeline (Cluster B), and a static JSON search index + vendored no-npm Cmd/Ctrl-K client (Cluster C). Out-of-P1 items (git diff timeline, KaTeX, mermaid, graph *view*, dev server, custom-component directives, `docgen-assets` embedding crate, Alpine) are explicitly deferred to P2–P6. The `LinkGraph.edges` are collected now (P1) so P4's graph view reuses them without re-deriving.

**Two-pass rationale:** Wikilink resolution requires the global slug set before any page renders, so P0's one-shot `assemble` is split into pure `prepare` (pass 1) + cross-doc `render_docs` (pass 2). `assemble` is retained as a single-doc wrapper so existing P0 unit tests stay valid. The AST is parsed once per doc with shared `comrak_options()` (so highlight + wikilink passes agree), transformed in the `Arena`, then formatted with the syntect plugin via `format_ast`.

**Type/key consistency:** `LinkEdge{from,to}`, `Backlink{slug,title}`, `SearchEntry{slug,title,text}`, `LinkGraph{edges,backlinks}`, `PreparedDoc{rel_path,slug,title,body_md}`, `SiteBuild{docs,graph,search}`, `SlugSet`, `WikilinkPass{resolved}`, `comrak_options()`, `SYNTECT_THEME`, `format_ast`, `plaintext`, `index_json`, `prepare`, `render_docs` are each defined once and used identically across clusters. Render context keys `backlinks` + `search_enabled` and asset filenames `search-index.json` / `search.js` / `docgen.css` are fixed in the contract section and referenced unchanged in template, build, and tests. Broken-link markup (`docgen-wikilink--broken` span with `data-target`) and resolved-link markup (`docgen-wikilink` anchor) are specified once and asserted verbatim in B3 and B9.

**API-drift guardrails:** Every comrak-0.52-specific call (`markdown_to_html_with_plugins`, `format_html_with_plugins`, `Plugins.render.codefence_syntax_highlighter`, `SyntectAdapter::new`, `parse_document` + `Arena`, `AstNode`/`NodeValue::{Text,HtmlInline,Code}`, `insert_before`/`detach`/`descendants`, `NodeCode.literal`) is annotated with a verification note instructing the implementer to defer to the compiler/crate source if the exact name differs — never downgrade comrak.

## Next phases (separate plans)

- **P2** git diff timeline (`git2` + port of existing diff logic).
- **P3** build-time KaTeX + mermaid + `docgen-assets` (Alpine + island embedding).
- **P4** graph view (reuses `LinkGraph.edges`).
- **P5** dev server (`axum` + `notify` + live reload) + CodeMirror editor.
- **P6** `docgen init` scaffold + custom-component directive system + binary distribution.
