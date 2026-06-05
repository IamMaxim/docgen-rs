# docgen-rs P0: Core SSG Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the minimal end-to-end static-site core: discover `docs/**/*.md`, parse frontmatter, render markdown to HTML, build a sidebar tree, render pages through a minijinja template, and write a static `dist/` — driven by a `docgen build` CLI.

**Architecture:** A Cargo workspace with three crates — `docgen-core` (pure content pipeline: discovery, frontmatter, markdown, tree), `docgen-render` (minijinja templating), and `docgen` (the CLI binary that orchestrates them). P0 deliberately excludes search, git diff, math, mermaid, graph, islands, and the dev server — those are P1–P6, each with its own plan. The crate boundaries here match the approved spec so later phases slot in without restructuring.

**Tech Stack:** Rust, `comrak` (CommonMark+GFM), `serde_yml` (frontmatter), `walkdir` (discovery), `minijinja` (templating), `clap` (CLI), `anyhow`/`thiserror` (errors).

**Reference:** spec at `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`.

---

## File Structure

```
docgen-rs/
  Cargo.toml                          # workspace manifest
  crates/
    docgen-core/
      Cargo.toml
      src/
        lib.rs                        # re-exports; ties modules together
        model.rs                      # Doc, RawDoc, TreeNode types
        frontmatter.rs                # split + parse YAML frontmatter
        markdown.rs                   # comrak render
        discover.rs                   # walk docs dir -> Vec<RawDoc>
        assemble.rs                   # RawDoc -> Doc (frontmatter+md+title+slug)
        tree.rs                       # Vec<Doc> -> Vec<TreeNode>
      tests/
        pipeline.rs                   # integration test over a fixture
    docgen-render/
      Cargo.toml
      src/
        lib.rs                        # Renderer, PageContext
      templates/
        page.html                     # default page template (embedded via include_str!)
    docgen/
      Cargo.toml
      src/
        main.rs                       # clap CLI entry
        build.rs                      # build command: discover->assemble->tree->render->write
      tests/
        build_cli.rs                  # end-to-end: build a fixture site, assert dist output
  fixtures/
    site-basic/
      docs/
        index.md
        guide/
          intro.md
```

Responsibilities are split by stage, not layer: each `docgen-core` module owns one
transformation and is unit-tested in isolation; `docgen-render` owns only
templating; `docgen` owns only orchestration + filesystem I/O.

---

### Task 1: Workspace + crate scaffold

**Files:**
- Create: `Cargo.toml` (workspace)
- Create: `crates/docgen-core/Cargo.toml`, `crates/docgen-core/src/lib.rs`
- Create: `crates/docgen-render/Cargo.toml`, `crates/docgen-render/src/lib.rs`
- Create: `crates/docgen/Cargo.toml`, `crates/docgen/src/main.rs`

- [ ] **Step 1: Create the workspace and crates**

Run from `~/work/docgen-rs`:

```bash
cargo new --lib crates/docgen-core
cargo new --lib crates/docgen-render
cargo new --bin crates/docgen
```

- [ ] **Step 2: Write the workspace `Cargo.toml`**

Replace the root `Cargo.toml` (the inner `cargo new` calls may have created member manifests but not a workspace root) with:

```toml
[workspace]
resolver = "2"
members = ["crates/docgen-core", "crates/docgen-render", "crates/docgen"]

[workspace.package]
edition = "2021"
license = "MIT"
version = "0.0.0"
```

- [ ] **Step 3: Add dependencies via `cargo add` (fetches current versions)**

```bash
cargo add --package docgen-core comrak serde_yml walkdir thiserror
cargo add --package docgen-core serde --features derive
cargo add --package docgen-render minijinja serde --features derive
cargo add --package docgen clap --features derive
cargo add --package docgen anyhow
cargo add --package docgen --path crates/docgen-core
cargo add --package docgen --path crates/docgen-render
```

- [ ] **Step 4: Make each crate inherit workspace package fields**

In each of `crates/docgen-core/Cargo.toml`, `crates/docgen-render/Cargo.toml`,
`crates/docgen/Cargo.toml`, set the `[package]` block to inherit:

```toml
[package]
name = "docgen-core"   # (docgen-render / docgen respectively)
edition.workspace = true
license.workspace = true
version.workspace = true
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo build`
Expected: PASS — all three crates compile (empty lib/bin stubs).

- [ ] **Step 6: Commit**

```bash
git init
printf "/target\n/dist\n" > .gitignore
git add -A
git commit -m "chore: scaffold docgen-rs cargo workspace"
```

---

### Task 2: Core types (`model.rs`)

**Files:**
- Create: `crates/docgen-core/src/model.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write `model.rs`**

```rust
use serde::Serialize;

/// A discovered markdown file, before processing.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDoc {
    /// Path relative to the docs root, using `/` separators, e.g. `guide/intro.md`.
    pub rel_path: String,
    /// Raw file contents.
    pub raw: String,
}

/// A fully processed document ready to render.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Doc {
    /// Path relative to docs root, e.g. `guide/intro.md`.
    pub rel_path: String,
    /// URL slug without extension, e.g. `guide/intro`.
    pub slug: String,
    /// Resolved page title.
    pub title: String,
    /// Rendered body HTML.
    pub body_html: String,
}

/// A node in the sidebar tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TreeNode {
    Dir { name: String, children: Vec<TreeNode> },
    Doc { name: String, slug: String, title: String },
}
```

- [ ] **Step 2: Wire the module into `lib.rs`**

Replace `crates/docgen-core/src/lib.rs` with:

```rust
pub mod model;

pub use model::{Doc, RawDoc, TreeNode};
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p docgen-core`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): add Doc, RawDoc, TreeNode types"
```

---

### Task 3: Frontmatter parsing (`frontmatter.rs`)

**Files:**
- Create: `crates/docgen-core/src/frontmatter.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Append to `crates/docgen-core/src/frontmatter.rs`:

```rust
use serde_yml::Value;

/// Result of splitting frontmatter from a markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub frontmatter: Value,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_yaml_frontmatter_and_body() {
        let raw = "---\ntitle: Hello\n---\n# Body\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("Hello"));
        assert_eq!(parsed.body, "# Body\n");
    }

    #[test]
    fn no_frontmatter_returns_null_and_full_body() {
        let raw = "# Just body\n";
        let parsed = parse_frontmatter(raw);
        assert!(parsed.frontmatter.is_null());
        assert_eq!(parsed.body, "# Just body\n");
    }

    #[test]
    fn strips_leading_bom() {
        let raw = "\u{feff}---\ntitle: X\n---\nbody\n";
        let parsed = parse_frontmatter(raw);
        assert_eq!(parsed.frontmatter["title"].as_str(), Some("X"));
        assert_eq!(parsed.body, "body\n");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p docgen-core frontmatter`
Expected: FAIL — `parse_frontmatter` is not defined.

- [ ] **Step 3: Implement `parse_frontmatter`**

Add above the `#[cfg(test)]` block in `frontmatter.rs`:

```rust
/// Split an optional leading `---`-delimited YAML frontmatter block from the body.
/// On malformed YAML, frontmatter is `Value::Null` and the whole input is the body.
pub fn parse_frontmatter(raw: &str) -> Parsed {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    if let Some(rest) = input.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let yaml = &rest[..end];
            // Skip past the closing `\n---`, then a trailing newline if present.
            let after = &rest[end + "\n---".len()..];
            let body = after.strip_prefix('\n').unwrap_or(after);
            let frontmatter = serde_yml::from_str(yaml).unwrap_or(Value::Null);
            return Parsed { frontmatter, body: body.to_string() };
        }
    }

    Parsed { frontmatter: Value::Null, body: input.to_string() }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Add to `crates/docgen-core/src/lib.rs`:

```rust
pub mod frontmatter;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p docgen-core frontmatter`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): parse YAML frontmatter"
```

---

### Task 4: Markdown rendering (`markdown.rs`)

**Files:**
- Create: `crates/docgen-core/src/markdown.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/markdown.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_heading_to_html() {
        let html = render_markdown("# Title");
        assert!(html.contains("<h1>"));
        assert!(html.contains("Title"));
    }

    #[test]
    fn renders_gfm_table() {
        let md = "| a | b |\n| - | - |\n| 1 | 2 |\n";
        let html = render_markdown(md);
        assert!(html.contains("<table>"));
    }

    #[test]
    fn renders_strikethrough() {
        let html = render_markdown("~~gone~~");
        assert!(html.contains("<del>"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p docgen-core markdown`
Expected: FAIL — `render_markdown` is not defined.

- [ ] **Step 3: Implement `render_markdown`**

Add above the test module in `markdown.rs`:

```rust
use comrak::{markdown_to_html, Options};

/// Render a markdown body (frontmatter already stripped) to HTML with GFM extensions.
pub fn render_markdown(body: &str) -> String {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    markdown_to_html(body, &options)
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Add to `crates/docgen-core/src/lib.rs`:

```rust
pub mod markdown;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p docgen-core markdown`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): render markdown to html via comrak"
```

---

### Task 5: Slug + title + assembly (`assemble.rs`)

**Files:**
- Create: `crates/docgen-core/src/assemble.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/assemble.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;

    #[test]
    fn slug_strips_md_extension() {
        assert_eq!(slug_for("guide/intro.md"), "guide/intro");
    }

    #[test]
    fn title_prefers_frontmatter() {
        let raw = RawDoc {
            rel_path: "a.md".into(),
            raw: "---\ntitle: From FM\n---\n# From Heading\n".into(),
        };
        let doc = assemble(raw);
        assert_eq!(doc.title, "From FM");
        assert_eq!(doc.slug, "a");
        assert!(doc.body_html.contains("From Heading"));
    }

    #[test]
    fn title_falls_back_to_first_h1_then_slug() {
        let with_h1 = assemble(RawDoc { rel_path: "b.md".into(), raw: "# Just Heading\n".into() });
        assert_eq!(with_h1.title, "Just Heading");

        let bare = assemble(RawDoc { rel_path: "c.md".into(), raw: "no heading here\n".into() });
        assert_eq!(bare.title, "c");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p docgen-core assemble`
Expected: FAIL — `slug_for` / `assemble` not defined.

- [ ] **Step 3: Implement assembly**

Add above the test module in `assemble.rs`:

```rust
use crate::frontmatter::parse_frontmatter;
use crate::markdown::render_markdown;
use crate::model::{Doc, RawDoc};

/// Derive a URL slug from a docs-relative path: strip a trailing `.md`.
pub fn slug_for(rel_path: &str) -> String {
    rel_path.strip_suffix(".md").unwrap_or(rel_path).to_string()
}

/// Extract the text of the first ATX `# ` heading, if any.
fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Process a RawDoc into a renderable Doc.
pub fn assemble(raw: RawDoc) -> Doc {
    let parsed = parse_frontmatter(&raw.raw);
    let slug = slug_for(&raw.rel_path);

    let fm_title = parsed.frontmatter["title"].as_str().map(|s| s.to_string());
    let title = fm_title
        .or_else(|| first_h1(&parsed.body))
        .unwrap_or_else(|| {
            // Last path segment of the slug as a final fallback.
            slug.rsplit('/').next().unwrap_or(&slug).to_string()
        });

    let body_html = render_markdown(&parsed.body);

    Doc { rel_path: raw.rel_path, slug, title, body_html }
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Add to `crates/docgen-core/src/lib.rs`:

```rust
pub mod assemble;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p docgen-core assemble`
Expected: PASS — 3 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): assemble RawDoc into Doc with slug and title"
```

---

### Task 6: Document tree (`tree.rs`)

**Files:**
- Create: `crates/docgen-core/src/tree.rs`
- Modify: `crates/docgen-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/docgen-core/src/tree.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Doc, TreeNode};

    fn doc(slug: &str, title: &str) -> Doc {
        Doc {
            rel_path: format!("{slug}.md"),
            slug: slug.into(),
            title: title.into(),
            body_html: String::new(),
        }
    }

    #[test]
    fn groups_docs_under_directories() {
        let docs = vec![doc("index", "Home"), doc("guide/intro", "Intro")];
        let tree = build_tree(&docs);

        // Directories come before loose docs; both sorted by name.
        assert_eq!(tree.len(), 2);
        match &tree[0] {
            TreeNode::Dir { name, children } => {
                assert_eq!(name, "guide");
                assert_eq!(children.len(), 1);
                assert!(matches!(&children[0], TreeNode::Doc { slug, .. } if slug == "guide/intro"));
            }
            other => panic!("expected dir, got {other:?}"),
        }
        assert!(matches!(&tree[1], TreeNode::Doc { slug, .. } if slug == "index"));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p docgen-core tree`
Expected: FAIL — `build_tree` not defined.

- [ ] **Step 3: Implement `build_tree`**

Add above the test module in `tree.rs`:

```rust
use std::collections::BTreeMap;

use crate::model::{Doc, TreeNode};

#[derive(Default)]
struct Builder {
    dirs: BTreeMap<String, Builder>,
    docs: BTreeMap<String, (String, String)>, // leaf name -> (slug, title)
}

fn insert(node: &mut Builder, parts: &[&str], slug: &str, title: &str) {
    match parts {
        [leaf] => {
            node.docs.insert(leaf.to_string(), (slug.to_string(), title.to_string()));
        }
        [head, rest @ ..] => {
            insert(node.dirs.entry(head.to_string()).or_default(), rest, slug, title);
        }
        [] => {}
    }
}

fn to_nodes(builder: Builder) -> Vec<TreeNode> {
    let mut out = Vec::new();
    // Directories first (BTreeMap keeps them name-sorted), then loose docs.
    for (name, child) in builder.dirs {
        out.push(TreeNode::Dir { name, children: to_nodes(child) });
    }
    for (name, (slug, title)) in builder.docs {
        out.push(TreeNode::Doc { name, slug, title });
    }
    out
}

/// Build a name-sorted sidebar tree from documents, keyed off their slugs.
pub fn build_tree(docs: &[Doc]) -> Vec<TreeNode> {
    let mut root = Builder::default();
    for doc in docs {
        let parts: Vec<&str> = doc.slug.split('/').collect();
        insert(&mut root, &parts, &doc.slug, &doc.title);
    }
    to_nodes(root)
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Add to `crates/docgen-core/src/lib.rs`:

```rust
pub mod tree;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p docgen-core tree`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): build sorted sidebar tree from docs"
```

---

### Task 7: Discovery (`discover.rs`)

**Files:**
- Create: `crates/docgen-core/src/discover.rs`
- Modify: `crates/docgen-core/src/lib.rs`
- Test: `crates/docgen-core/tests/pipeline.rs`

- [ ] **Step 1: Write the failing integration test**

Create `crates/docgen-core/tests/pipeline.rs`:

```rust
use std::fs;

use docgen_core::assemble::assemble;
use docgen_core::discover::discover_docs;
use docgen_core::tree::build_tree;

#[test]
fn discovers_and_processes_a_temp_site() {
    let dir = std::env::temp_dir().join("docgen_core_pipeline_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("docs/guide")).unwrap();
    fs::write(dir.join("docs/index.md"), "# Home\n").unwrap();
    fs::write(dir.join("docs/guide/intro.md"), "---\ntitle: Intro\n---\nbody\n").unwrap();

    let mut raws = discover_docs(&dir.join("docs")).unwrap();
    raws.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    assert_eq!(raws.len(), 2);
    assert_eq!(raws[0].rel_path, "guide/intro.md");
    assert_eq!(raws[1].rel_path, "index.md");

    let docs: Vec<_> = raws.into_iter().map(assemble).collect();
    let tree = build_tree(&docs);
    assert_eq!(tree.len(), 2); // one dir (guide) + one doc (index)

    let _ = fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p docgen-core --test pipeline`
Expected: FAIL — `discover_docs` not defined.

- [ ] **Step 3: Implement `discover_docs`**

Create `crates/docgen-core/src/discover.rs`:

```rust
use std::path::Path;

use walkdir::WalkDir;

use crate::model::RawDoc;

/// Walk `root` recursively and read every `.md` file into a RawDoc.
/// `rel_path` is the path relative to `root`, normalized to `/` separators.
pub fn discover_docs(root: &Path) -> std::io::Result<Vec<RawDoc>> {
    let mut docs = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let raw = std::fs::read_to_string(path)?;
        docs.push(RawDoc { rel_path: rel, raw });
    }
    Ok(docs)
}
```

- [ ] **Step 4: Wire the module into `lib.rs`**

Add to `crates/docgen-core/src/lib.rs`:

```rust
pub mod discover;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p docgen-core --test pipeline`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-core
git commit -m "feat(core): discover markdown files under docs root"
```

---

### Task 8: Renderer (`docgen-render`)

**Files:**
- Create: `crates/docgen-render/templates/page.html`
- Modify: `crates/docgen-render/src/lib.rs`

- [ ] **Step 1: Write the default page template**

Create `crates/docgen-render/templates/page.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{{ title }}</title>
</head>
<body>
  <nav class="docgen-sidebar">
    {% macro render_nodes(nodes) %}
    <ul>
      {% for node in nodes %}
        {% if node.kind == "dir" %}
          <li class="docgen-dir"><span>{{ node.name }}</span>{{ render_nodes(node.children) }}</li>
        {% else %}
          <li class="docgen-doc"><a href="/{{ node.slug }}">{{ node.title }}</a></li>
        {% endif %}
      {% endfor %}
    </ul>
    {% endmacro %}
    {{ render_nodes(tree) }}
  </nav>
  <main class="docgen-content">
    {{ body | safe }}
  </main>
</body>
</html>
```

- [ ] **Step 2: Write the failing tests**

Replace `crates/docgen-render/src/lib.rs` test section by creating the file with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use docgen_core::model::TreeNode;

    fn renderer() -> Renderer {
        Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap()
    }

    #[test]
    fn renders_title_and_body() {
        let html = renderer()
            .render_page(&PageContext {
                title: "My Page".into(),
                body_html: "<p>hello</p>".into(),
                tree: &[],
            })
            .unwrap();
        assert!(html.contains("<title>My Page</title>"));
        assert!(html.contains("<p>hello</p>"));
    }

    #[test]
    fn renders_sidebar_links() {
        let tree = vec![TreeNode::Doc {
            name: "intro".into(),
            slug: "guide/intro".into(),
            title: "Intro".into(),
        }];
        let html = renderer()
            .render_page(&PageContext { title: "X".into(), body_html: String::new(), tree: &tree })
            .unwrap();
        assert!(html.contains(r#"href="/guide/intro""#));
        assert!(html.contains(">Intro</a>"));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p docgen-render`
Expected: FAIL — `Renderer`, `PageContext`, `DEFAULT_PAGE_TEMPLATE` not defined.

- [ ] **Step 4: Implement the renderer**

Add above the test module in `crates/docgen-render/src/lib.rs`:

```rust
use docgen_core::model::TreeNode;
use minijinja::{context, Environment};
use serde::Serialize;

/// The built-in page template, embedded at compile time.
pub const DEFAULT_PAGE_TEMPLATE: &str = include_str!("../templates/page.html");

/// Everything a single page render needs.
#[derive(Serialize)]
pub struct PageContext<'a> {
    pub title: String,
    pub body_html: String,
    pub tree: &'a [TreeNode],
}

/// Owns a configured minijinja environment with the `page` template registered.
pub struct Renderer {
    env: Environment<'static>,
}

impl Renderer {
    /// Build a renderer from a page-template source string.
    pub fn new(page_template: &str) -> Result<Self, minijinja::Error> {
        let mut env = Environment::new();
        env.add_template_owned("page", page_template.to_string())?;
        Ok(Self { env })
    }

    /// Render one page to a full HTML document.
    pub fn render_page(&self, ctx: &PageContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("page")?;
        tmpl.render(context! {
            title => ctx.title,
            body => ctx.body_html,
            tree => ctx.tree,
        })
    }
}
```

Then add `docgen-core` as a dependency of `docgen-render`:

```bash
cargo add --package docgen-render --path crates/docgen-core
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p docgen-render`
Expected: PASS — 2 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/docgen-render
git commit -m "feat(render): minijinja page renderer with sidebar tree"
```

---

### Task 9: Build command (`docgen` binary)

**Files:**
- Modify: `crates/docgen/src/main.rs`
- Create: `crates/docgen/src/build.rs`

- [ ] **Step 1: Write `build.rs`**

Create `crates/docgen/src/build.rs`:

```rust
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use docgen_core::assemble::assemble;
use docgen_core::discover::discover_docs;
use docgen_core::tree::build_tree;
use docgen_render::{PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

/// Build the site at `project_root` (which must contain `docs/`) into `project_root/dist`.
pub fn build(project_root: &Path) -> Result<()> {
    let docs_dir = project_root.join("docs");
    let dist_dir = project_root.join("dist");

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;
    let docs: Vec<_> = raws.into_iter().map(assemble).collect();
    let tree = build_tree(&docs);

    let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;

    // Clean and recreate dist.
    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    for doc in &docs {
        let html = renderer.render_page(&PageContext {
            title: doc.title.clone(),
            body_html: doc.body_html.clone(),
            tree: &tree,
        })?;

        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), html)?;
    }

    println!("Built {} page(s) -> {}", docs.len(), dist_dir.display());
    Ok(())
}
```

- [ ] **Step 2: Write `main.rs` with the clap CLI**

Replace `crates/docgen/src/main.rs`:

```rust
mod build;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "docgen", version, about = "Static documentation-site generator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the static site from `docs/` into `dist/`.
    Build {
        /// Project root (defaults to the current directory).
        #[arg(default_value = ".")]
        root: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { root } => build::build(&root),
    }
}
```

- [ ] **Step 3: Verify it builds**

Run: `cargo build -p docgen`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/docgen
git commit -m "feat(cli): docgen build command"
```

---

### Task 10: End-to-end CLI test + fixture

**Files:**
- Create: `fixtures/site-basic/docs/index.md`
- Create: `fixtures/site-basic/docs/guide/intro.md`
- Create: `crates/docgen/tests/build_cli.rs`

- [ ] **Step 1: Create the fixture site**

Create `fixtures/site-basic/docs/index.md`:

```markdown
---
title: Home
---

# Welcome

This is the **basic** fixture site.
```

Create `fixtures/site-basic/docs/guide/intro.md`:

```markdown
# Introduction

Some intro prose with a [[wikilink]] (rendered literally in P0).
```

- [ ] **Step 2: Write the failing end-to-end test**

Create `crates/docgen/tests/build_cli.rs`:

```rust
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Copy the checked-in fixture into a temp dir, run `docgen build`, assert output.
#[test]
fn builds_fixture_site() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen
    let workspace = manifest.parent().unwrap().parent().unwrap(); // repo root
    let fixture = workspace.join("fixtures/site-basic");

    let tmp = std::env::temp_dir().join("docgen_build_cli_test");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), tmp.join("docs/index.md")).unwrap();
    fs::copy(fixture.join("docs/guide/intro.md"), tmp.join("docs/guide/intro.md")).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    let home = fs::read_to_string(tmp.join("dist/index/index.html")).unwrap();
    assert!(home.contains("<title>Home</title>"));
    assert!(home.contains("<strong>basic</strong>"));

    let intro = fs::read_to_string(tmp.join("dist/guide/intro/index.html")).unwrap();
    assert!(intro.contains("<title>Introduction</title>"));
    // Sidebar shows both entries on every page.
    assert!(intro.contains(r#"href="/index""#));

    let _ = fs::remove_dir_all(&tmp);
}
```

- [ ] **Step 3: Run the test to verify it fails (then passes)**

Run: `cargo test -p docgen --test build_cli`
Expected: PASS — the binary already exists from Task 9, so this test should pass immediately, proving the full pipeline end-to-end. If it fails, fix the build orchestration in `build.rs` until it passes.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test`
Expected: PASS — all unit + integration tests across the three crates.

- [ ] **Step 5: Commit**

```bash
git add fixtures crates/docgen/tests
git commit -m "test(cli): end-to-end build of fixture site"
```

---

### Task 11: README + manual smoke test

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write a minimal README**

Create `README.md`:

```markdown
# docgen-rs

A Cargo-only static documentation-site generator. No npm, no Node.

## Status

P0 (core SSG): markdown discovery, frontmatter, rendering, sidebar tree, static
`dist/` output via `docgen build`. See `docs/superpowers/plans/` for the roadmap.

## Usage

```sh
cargo run -p docgen -- build path/to/project
```

The project must contain a `docs/` directory of `.md` files. Output is written to
`path/to/project/dist/`.
```

- [ ] **Step 2: Manual smoke test against the fixture**

Run:

```bash
cargo run -p docgen -- build fixtures/site-basic
```

Expected: prints `Built 2 page(s) -> fixtures/site-basic/dist`, and
`fixtures/site-basic/dist/index/index.html` exists and opens in a browser showing
the heading, bold text, and a sidebar with both pages.

- [ ] **Step 3: Add `fixtures/**/dist` to `.gitignore` and commit**

```bash
printf "fixtures/**/dist\n" >> .gitignore
git add README.md .gitignore
git commit -m "docs: P0 README and gitignore fixture output"
```

---

## Self-Review

**Spec coverage (P0 scope only):** This plan covers the P0 slice of the spec —
crate layout (`docgen-core`, `docgen-render`, `docgen`; `docgen-assets` is
deliberately deferred to P3 when islands first need embedding), markdown via comrak,
frontmatter, doc tree, minijinja templating, static `dist/` output, and the
`docgen build` CLI. Out-of-P0 spec items (search, git diff timeline, KaTeX, mermaid,
graph, dev server, custom-component directives, `init`/distribution) are explicitly
reserved for later phase plans and are NOT gaps in this plan.

**Placeholder scan:** No TBD/TODO steps; every code step shows complete code; every
test step shows the command and expected result. The fixture's `[[wikilink]]` is
intentionally rendered literally in P0 (wikilinks are P1) and the test asserts only
the sidebar link, not wikilink processing.

**Type consistency:** `RawDoc { rel_path, raw }`, `Doc { rel_path, slug, title,
body_html }`, and `TreeNode::{Dir,Doc}` are defined once in Task 2 and used with
those exact field names in Tasks 5–10. `parse_frontmatter -> Parsed { frontmatter,
body }`, `render_markdown(&str) -> String`, `slug_for`, `assemble`, `build_tree`,
`discover_docs`, `Renderer::new` / `render_page`, `PageContext { title, body_html,
tree }`, and `DEFAULT_PAGE_TEMPLATE` are referenced consistently across tasks. The
minijinja template's `node.kind` matches the `#[serde(tag = "kind", rename_all =
"lowercase")]` on `TreeNode`.

## Next phases (separate plans, written when you reach them)

- **P1** search (JSON index + client search) + `syntect` highlight + wikilinks/backlinks
- **P2** git diff timeline (`git2` + port of existing diff logic)
- **P3** build-time KaTeX + mermaid + **`docgen-assets`** crate (Alpine + island embedding)
- **P4** graph view
- **P5** dev server (`axum` + `notify` + live reload) + CodeMirror editor
- **P6** `docgen init` scaffold + custom-component directive system + binary distribution
