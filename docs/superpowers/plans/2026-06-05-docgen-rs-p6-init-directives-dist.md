# docgen-rs P6 — config + root-`/` fix, custom-component directives, `init` + distribution

**Date:** 2026-06-05
**Phase:** P6 (final — `init`/scaffold + custom-component system + binary distribution)
**Branch:** `overnight/p1-p6` (local only — never push/PR/tag/release)
**Status:** Plan approved, not yet implemented
**Spec:** `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`
(sections read carefully: *Custom-component system* (the whole thing), *Distribution*,
*Config* lines in Decisions + Distribution: `svelte.config.js` → `docgen.toml`)
**Original scaffolder reference:** `~/work/docgen/packages/create-docgen` (`lib/copy.mjs`
token-substitution + dotfile-rename + feature-gated file copy) and
`~/work/docgen/templates/{minimal,starter}`.

---

## 0. Scope, decisions, and what P6 inherits

### 0.1 Goal restated — three clusters, landed strictly in order

- **Cluster A — `docgen.toml` config + the root-`/` fix.** A new `docgen-config`
  crate parses an optional `docgen.toml` (`title`, `base` path, `[features]` graph
  / math / mermaid / search toggles, `[components] dir`). `build_site` loads it,
  threads a `SiteConfig` through the pipeline and into every template context
  (page/graph/history), and gates the `/graph/` page + per-page asset/feature
  emission on the config. **The long-standing P0 carry-over is fixed:** the doc
  whose slug is `index` (the home doc) is also written to `dist/index.html` so the
  site has a real page at `/`, *in addition to* `dist/index/index.html` (existing
  links to `/index` keep working). No existing test breaks.

- **Cluster B — custom-component directive system (the headline).** A new
  `docgen-components` crate discovers `components/<name>/{template.html, island.js?,
  style.css?}` from the project (and from a built-in set embedded in
  `docgen-assets`, project overrides built-in by name). A new
  `docgen-core::directivepass` pre-pass parses `:::name{attrs}` block directives
  (inner markdown, recursively rendered) and `:name[label]{attrs}` leaf directives
  out of the markdown **source**, renders each through the component's minijinja
  `template.html` (context: `attrs`, `content`, `label`, `id`), and substitutes the
  resulting HTML. Per-page island gating + concatenated `island.js` + `style.css`
  emission flows through `docgen-assets`/the build. **`callout` ships as a built-in
  implemented through this very mechanism (dogfood).** A project `components/callout/`
  overrides it. Unknown directive → a clearly-marked inert error span, never a crash.

- **Cluster C — `docgen init` + binary distribution tooling.** A new
  `docgen-init` crate scaffolds a fresh site (`docgen.toml`, `docs/` sample content
  exercising wikilinks/math/mermaid/a custom directive, a sample
  `components/note/`, `.gitignore`) into a target dir; wired as `docgen init [dir]`.
  Plus **release tooling only** (NOT executed/pushed/tagged): a
  `.github/workflows/release.yml` cross-compiling binaries, `cargo-binstall`
  `[package.metadata.binstall]` in `crates/docgen/Cargo.toml`, and README install
  docs. An `init → build` integration smoke scaffolds to a temp dir, runs
  `build_site`, and asserts the custom component rendered in the output.

No npm, no node, no bundler, no WASM. Everything authored as plain files; binaries
ship via GitHub Releases + binstall.

### 0.2 FLAGGED DECISION — directive parsing is a **source-level pre-pass**, not an AST pass. Confirmed.

comrak 0.52 has **no** generic `:::` directive extension. Two options:

1. **AST pass** (like `mathpass`/`mermaidpass`): let comrak parse, then walk the
   AST recognising paragraph/text patterns. **Rejected** — `:::name{...}` lines are
   not a comrak node type; they land as ordinary Paragraph/Text nodes mixed with
   surrounding prose, and a *block* directive's inner content must itself be parsed
   as markdown. Reconstructing block boundaries from a flattened inline AST is
   fragile and loses the raw inner-markdown span we need to recursively render.

2. **Source-level pre-pass** (chosen). Operate on the **raw markdown body string**
   *before* `parse_document`. A small hand-written scanner finds directive blocks
   (fenced by lines that are exactly `:::name{...}` … `:::`) and leaf directives
   (inline `:name[label]{attrs}`), extracts `attrs`/`label`/inner-source, renders
   the component, and substitutes a placeholder that survives comrak untouched.

   **Why a placeholder, not direct HTML injection?** If we spliced raw HTML into the
   markdown source, comrak would re-interpret it (indentation → code block, blank
   lines → paragraph splits) and could mangle multi-line component HTML. Instead the
   pre-pass replaces each directive with a unique sentinel
   `<!--docgen-directive:N-->` placed on its own HTML-block line; comrak passes HTML
   comments through verbatim (with `render.unsafe = true`). After `format_ast`, a
   **post-pass** string-substitutes each sentinel with the component's rendered HTML.
   Block inner content is rendered by **recursively** running the *same* markdown
   pipeline (`render_block_markdown`) on the inner source → that inner HTML becomes
   the template's `content`. Leaf directives have no inner content (`content=""`),
   only `label`.

   This keeps the directive system **orthogonal** to comrak's AST passes
   (wikilink/math/mermaid still run on the post-pre-pass source), and gives us the
   raw inner-markdown span block directives require.

**Ordering inside `render_docs` per doc (updated):**
1. `directivepass::extract` on `body_md` → `(rewritten_md, Vec<DirectiveInstance>)`.
2. comrak `parse_document(rewritten_md)`; search plaintext from pristine AST (now
   directive-free — search indexes prose, not directive markup).
3. wikilink AST pass → math pass → mermaid pass → `format_ast` (unchanged).
4. `directivepass::substitute(html, &instances, &registry)` → replace each
   `<!--docgen-directive:N-->` sentinel with rendered component HTML; collect the
   set of component names actually used on this page (drives per-page island gating).

> Nested block directives (a `:::callout` containing another `:::callout`) are
> supported because inner content is rendered by the **full** recursive pipeline,
> which itself runs `extract`. The scanner matches the **outermost** `:::…:::`
> pair by depth-counting `:::` open/close fence lines, so the inner pair is handed
> to the recursive call intact. (Tested.)

### 0.3 FLAGGED DECISION — directive attribute syntax. Confirmed.

`{attrs}` is a space-separated list of `key=value` or bare `key` (→ `key="true"`).
Values: bare token (`type=warning`) or double-quoted (`title="Back up first"`,
may contain spaces). Matches the spec's examples exactly (`type=warning
title="Back up first"`, `id=dQw4w9WgXcQ`, `max=5 label="Was this page helpful?"`).
A tiny hand-written parser (no regex crate dependency); `attrs` is a
`BTreeMap<String,String>` (deterministic ordering, minijinja-serializable). Bare
`{}` or absent `{...}` → empty map. `label` is the `[...]` text for leaf form
(absent for block form → empty string). Parsing is **total**: malformed attrs
degrade gracefully (best-effort token split), never panic.

### 0.4 FLAGGED DECISION — built-ins dogfood via embedded component files. Confirmed.

The built-in `callout` lives at
`crates/docgen-assets/components/callout/{template.html, style.css}` and is
embedded through the existing `include_dir!("$CARGO_MANIFEST_DIR/...")` mechanism
(a second `Dir` rooted at `components/`). `docgen-components::builtin_registry()`
reads these embedded files into `Component` structs **through the same code path**
that loads project components (same `Component::from_parts` constructor, same
minijinja render). A project `components/callout/` with the same folder name
**overrides** the built-in (project entries inserted last / win on name collision).
No special-casing: the built-in is just a component whose bytes happen to be
embedded. This is the spec's "built-ins dogfood the same mechanism."

### 0.5 FLAGGED DECISION — config is optional; absent `docgen.toml` = today's behaviour. Confirmed.

`SiteConfig::default()` reproduces the current hard-coded behaviour exactly:
`title=None` (→ per-page titles unchanged), `base=""`, all features on
(`graph/math/mermaid/search = true`). So **every existing test stays green** with
no `docgen.toml` present. The config only *subtracts* (toggles features off) or
*adds* (a site title prefix / base path), never silently changes current output
when absent. `base` defaults to `""` (empty) → all current absolute `/foo` links
are unchanged; a non-empty `base` (e.g. `/docs`) is a follow-on nicety wired but
defaulting to no-op (we implement parsing + threading + one template use, full
base-prefixing of every link is explicitly **minimal** in P6: only the
`<base href>` is emitted; see B-note).

### 0.6 What P6 inherits and must not break

- `docgen-core::pipeline::{prepare, render_docs, PreparedDoc, SiteBuild}` — the
  two-pass pipeline. `render_docs` gains a `&Registry` + `&SiteConfig` param
  (Cluster A adds config param; Cluster B adds registry param). Existing callers
  (`docgen-build`) updated; the `#[test]` callers in `pipeline.rs` updated to pass
  `Registry::empty()` / `SiteConfig::default()`.
- `docgen-render::{Renderer, PageContext, GraphContext, HistoryContext}` — page
  template + contexts. P6 adds a `config`/`site_title`/`base` field (threaded; old
  tests pass `..Default::default()` once `PageContext` derives `Default`, OR are
  updated explicitly — see A-6 note).
- `docgen-assets::{assets_for, emit, EmitOptions, Asset, AssetKind}` — P6 adds a
  `components` asset slice (`components.css`, `components.js`) via a new
  `EmitOptions.include_components: bool` + a function taking the concatenated
  authored bytes. Existing slices untouched.
- `docgen-build::{build, build_site, BuildOptions, BuildMode, BuildOutcome}` — the
  pipeline orchestrator. P6 loads config + registry, threads them, emits the root
  page + component assets. `BuildOptions`/`BuildMode` signatures unchanged
  (config/registry are loaded *inside* `build_site` from `project_root`).
- The atomic `StagingDir` swap — untouched; P6 just writes more files into staging.
- Per-page island gating convention (mermaid/graph) — the component island gating
  reuses it exactly (a `<script src="/components.js">` emitted only when a page used
  ≥1 component with an `island.js`).

### 0.7 Crates added / changed

- **NEW** `crates/docgen-config` (lib) — `SiteConfig`, `Features`, `load`.
- **NEW** `crates/docgen-components` (lib) — `Component`, `Registry`,
  `discover`, `builtin_registry`, `render_instance`.
- **NEW** `crates/docgen-init` (lib) — `scaffold(InitOptions) -> Result<()>` + an
  embedded template tree.
- **CHANGED** `docgen-core` — new `directivepass` module; `render_docs` signature.
- **CHANGED** `docgen-assets` — embed `components/`; `components_assets`,
  `EmitOptions.include_components`; built-in component files.
- **CHANGED** `docgen-render` — page/graph/history contexts grow config fields;
  page template grows `<base>`/title-prefix/`components.css`+`.js` hooks.
- **CHANGED** `docgen-build` — load config+registry, thread them, emit root page +
  component assets.
- **CHANGED** `docgen` (bin) — `init` subcommand; `[package.metadata.binstall]`.
- **NEW** `.github/workflows/release.yml`; **CHANGED** `README.md`.

Workspace `members` updated to include the three new crates.

---

## Cluster A — `docgen.toml` config + root-`/` fix

### A-1. `docgen-config` crate skeleton + `SiteConfig`/`Features` types (TDD)

**New crate** `crates/docgen-config/Cargo.toml`:

```toml
[package]
name = "docgen-config"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
serde = { version = "1.0.228", features = ["derive"] }
toml = "0.8"
thiserror = "2.0.18"
```

Add to workspace `members`: `"crates/docgen-config"`.

**`crates/docgen-config/src/lib.rs`** — write tests FIRST, then the types:

```rust
//! Parses an optional `docgen.toml`. When absent, `SiteConfig::default()`
//! reproduces docgen's pre-P6 hard-coded behaviour exactly, so a project with
//! no config builds identically to before.

use std::path::Path;

use serde::Deserialize;

/// Feature toggles. All default `true` — the pre-P6 behaviour (every feature on).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Features {
    /// Emit the `/graph/` page + its island.
    pub graph: bool,
    /// Render math (build-time KaTeX) + link its stylesheet.
    pub math: bool,
    /// Allow mermaid diagrams + lazy island.
    pub mermaid: bool,
    /// Emit the search index + search client.
    pub search: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self { graph: true, math: true, mermaid: true, search: true }
    }
}

/// `[components]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ComponentsConfig {
    /// Project-relative directory holding `<name>/template.html` components.
    pub dir: String,
}

impl Default for ComponentsConfig {
    fn default() -> Self {
        Self { dir: "components".to_string() }
    }
}

/// The whole resolved site config. `Default` == pre-P6 behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SiteConfig {
    /// Optional site title; when set, page `<title>` becomes `"{page} — {title}"`
    /// (home page uses just `title`). When `None`, per-page titles are unchanged.
    pub title: Option<String>,
    /// Base path for the deployed site (e.g. `/docs`). Empty = served at root
    /// (unchanged behaviour). Emitted as `<base href>` only in P6.
    pub base: String,
    pub features: Features,
    pub components: ComponentsConfig,
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            title: None,
            base: String::new(),
            features: Features::default(),
            components: ComponentsConfig::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {path}: {source}")]
    Io { path: String, #[source] source: std::io::Error },
    #[error("parsing {path}: {source}")]
    Parse { path: String, #[source] source: toml::de::Error },
}

/// Load `docgen.toml` from `project_root`. Missing file → `SiteConfig::default()`
/// (not an error). Present-but-malformed → `Err`.
pub fn load(project_root: &Path) -> Result<SiteConfig, ConfigError> {
    let path = project_root.join("docgen.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SiteConfig::default()),
        Err(e) => return Err(ConfigError::Io { path: path.display().to_string(), source: e }),
    };
    toml::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}
```

**Tests** (`#[cfg(test)] mod tests` in the same file):

```rust
#[test]
fn default_is_pre_p6_behaviour() {
    let c = SiteConfig::default();
    assert_eq!(c.title, None);
    assert_eq!(c.base, "");
    assert!(c.features.graph && c.features.math && c.features.mermaid && c.features.search);
    assert_eq!(c.components.dir, "components");
}

#[test]
fn missing_file_yields_default() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load(dir.path()).unwrap(), SiteConfig::default());
}

#[test]
fn parses_title_base_and_feature_toggles() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("docgen.toml"),
        "title = \"My Docs\"\nbase = \"/docs\"\n[features]\ngraph = false\nmermaid = false\n",
    ).unwrap();
    let c = load(dir.path()).unwrap();
    assert_eq!(c.title.as_deref(), Some("My Docs"));
    assert_eq!(c.base, "/docs");
    assert!(!c.features.graph);
    assert!(!c.features.mermaid);
    // Unspecified toggles keep their default (true).
    assert!(c.features.math);
    assert!(c.features.search);
}

#[test]
fn partial_features_table_keeps_other_defaults() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docgen.toml"), "[features]\nsearch = false\n").unwrap();
    let c = load(dir.path()).unwrap();
    assert!(!c.features.search);
    assert!(c.features.graph);
}

#[test]
fn malformed_toml_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docgen.toml"), "title = = =\n").unwrap();
    assert!(load(dir.path()).is_err());
}
```

Add `tempfile = "3"` to `[dev-dependencies]`.

**Cargo:**
```
cargo test -p docgen-config
cargo clippy -p docgen-config --all-targets
```
**Commit:** `feat(config): docgen.toml SiteConfig/Features loader (default == pre-P6 behaviour)`

---

### A-2. Thread `SiteConfig` through `render_docs` (signature change, no behaviour change yet)

`render_docs` gains a `config: &docgen_config::SiteConfig` param. In A-2 it only
*gates* which passes run via features (math/mermaid), and is otherwise inert.

Add to `docgen-core/Cargo.toml` `[dependencies]`:
`docgen-config = { path = "../docgen-config" }`.

**`pipeline.rs` change:**

```rust
pub fn render_docs(
    prepared: Vec<PreparedDoc>,
    config: &docgen_config::SiteConfig,   // NEW
) -> SiteBuild {
    // ... unchanged setup ...
    for p in &prepared {
        // ... wikilink pass unchanged ...
        let math_count = if config.features.math {
            crate::mathpass::transform_math(root)
        } else { 0 };
        let mermaid_count = if config.features.mermaid {
            crate::mermaidpass::transform_mermaid(root)
        } else { 0 };
        // ...
    }
    // ...
}
```

> Note: when `math` is off, `$E=mc^2$` simply renders as comrak's default (the
> `math_dollars` extension still parses it but, with no pass, comrak emits its own
> `<span data-math-style>` — acceptable: the toggle's contract is "no build-time
> KaTeX + no css link", which holds because `has_math` is then false). Tested.

Update existing `pipeline.rs` tests to pass `&SiteConfig::default()`. Add:

```rust
#[test]
fn math_feature_off_skips_build_time_katex() {
    let prepared = vec![prepare(raw("m.md", "# M\n$E=mc^2$\n"))];
    let mut cfg = docgen_config::SiteConfig::default();
    cfg.features.math = false;
    let site = render_docs(prepared, &cfg);
    assert!(!site.docs[0].has_math);
    assert!(!site.docs[0].body_html.contains("katex"));
}

#[test]
fn mermaid_feature_off_leaves_code_block() {
    let prepared = vec![prepare(raw("d.md", "# D\n```mermaid\ngraph TD;A-->B;\n```\n"))];
    let mut cfg = docgen_config::SiteConfig::default();
    cfg.features.mermaid = false;
    let site = render_docs(prepared, &cfg);
    assert!(!site.docs[0].has_mermaid);
    assert!(!site.any_mermaid);
}
```

Update `docgen-build/src/lib.rs` call site: `render_docs(prepared, &config)` (config
loaded in A-5). To keep A-2 self-contained, `build_site` temporarily passes
`&docgen_config::SiteConfig::default()` (add `docgen-config` dep to docgen-build);
A-5 replaces it with the real load.

**Cargo:** `cargo test -p docgen-core -p docgen-build && cargo clippy --all-targets`
**Commit:** `feat(core): thread SiteConfig into render_docs; math/mermaid feature gates`

---

### A-3. Root-`/` fix: write the home doc to `dist/index.html`

The home doc is the doc whose `slug == "index"` (top-level `docs/index.md`). Today
it lands only at `dist/index/index.html`, so `/` 404s. Fix in `build_site` Phase 2:
after writing `dist/<slug>/index.html` for every doc, **additionally** write the
home doc's already-rendered HTML to `dist/index.html`.

Helper in `docgen-build/src/lib.rs`:

```rust
/// The slug docgen treats as the site home (served at `/`).
const HOME_SLUG: &str = "index";
```

In Phase 2's loop, capture the home doc's rendered html:

```rust
let mut home_html: Option<String> = None;
for doc in &site.docs {
    // ... render `html` as today ...
    let out_dir = dist_dir.join(&doc.slug);
    fs::create_dir_all(&out_dir)?;
    fs::write(out_dir.join("index.html"), &html)?;
    if doc.slug == HOME_SLUG {
        home_html = Some(html.clone());
    }
}
// Root page: serve the home doc at `/` too, so the site has a real index.
// Existing `/index` links keep working (that page is still emitted above).
if let Some(html) = home_html {
    fs::write(dist_dir.join("index.html"), html)?;
}
```

> Why *both* `dist/index.html` and `dist/index/index.html`? Keeping both is the
> zero-breakage choice: every current test asserts `index/index.html` and every
> existing `/index` wikilink resolves to `dist/index/index.html`. Adding the root
> file is purely additive. (A future phase may redirect or drop the nested copy;
> out of P6 scope.)

**Test** in `docgen-build/tests/build_site.rs` (fixture already has `docs/index.md`
with title `Home`):

```rust
#[test]
fn build_site_emits_a_real_root_index_page() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    }).unwrap();

    let root_html = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(root_html.contains("<title>Home</title>"));
    // The nested copy still exists (no link breakage).
    assert!(out.path().join("index/index.html").is_file());
    // Both are byte-identical (same rendered home doc).
    let nested = std::fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert_eq!(root_html, nested);
}
```

> Edge case: a site with no `docs/index.md`. Then `home_html` is `None`, no root
> page is written — same as today (no regression; the site simply has no `/`).
> Covered by a second test that builds a fixture lacking `index.md` and asserts
> `!out.path().join("index.html").exists()` while other pages exist.

**Cargo:** `cargo test -p docgen-build && cargo clippy --all-targets`
**Commit:** `fix(build): emit the home doc at dist/index.html so the site has a real / page`

---

### A-4. Site title + `<base>` in templates (config → contexts)

Add config-derived fields to `PageContext`, `GraphContext`, `HistoryContext` in
`docgen-render`. Keep them simple `&str`:

```rust
// added to each of PageContext / GraphContext / HistoryContext:
pub base: &'a str,        // "" by default → emits empty <base>, inert
pub site_title: &'a str,  // "" → no title prefix
```

`Renderer::render_page` passes `base => ctx.base, site_title => ctx.site_title`.
Page template `<head>`:

```html
{% if base %}<base href="{{ base }}/" />{% endif %}
<title>{% if site_title %}{{ title }} — {{ site_title }}{% else %}{{ title }}{% endif %}</title>
```

> `base` only emits a `<base>` tag in P6 (the minimal contract from §0.5). Full
> link rewriting is out of scope; the `<base>` makes root-relative assets resolve
> under a subpath for the common GitHub-Pages case.

**Tests** in `docgen-render/src/lib.rs`:

```rust
#[test]
fn page_title_gets_site_suffix_when_configured() {
    let html = renderer().render_page(&PageContext {
        title: "Intro", site_title: "My Docs", base: "",
        slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: false, has_math: false,
    }).unwrap();
    assert!(html.contains("<title>Intro — My Docs</title>"));
}

#[test]
fn no_site_title_leaves_plain_title_and_no_base() {
    let html = renderer().render_page(&PageContext {
        title: "Intro", site_title: "", base: "",
        slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: false, has_math: false,
    }).unwrap();
    assert!(html.contains("<title>Intro</title>"));
    assert!(!html.contains("<base"));
}

#[test]
fn base_emits_base_tag() {
    let html = renderer().render_page(&PageContext {
        title: "X", site_title: "", base: "/docs",
        slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: false, has_math: false,
    }).unwrap();
    assert!(html.contains(r#"<base href="/docs/" />"#));
}
```

> **Test-churn note:** every existing `PageContext { … }` literal in render/build
> tests must add `base: "", site_title: ""`. To minimise churn, this step is where
> we add those two fields to every existing literal (render tests, build code).
> (We choose explicit fields over `#[derive(Default)]` because `PageContext`
> borrows `&'a` references that have no sensible default.)

**Cargo:** `cargo test -p docgen-render && cargo clippy --all-targets`
**Commit:** `feat(render): config-driven site title suffix + optional <base> tag`

---

### A-5. `build_site` loads config + gates graph/search/title (real wiring)

`docgen-build/src/lib.rs`:

```rust
let config = docgen_config::load(opts.project_root)
    .with_context(|| format!("loading docgen.toml from {}", opts.project_root.display()))?;
// ...
let site = render_docs(prepared, &config);
```

Gate Phase 3 (`/graph/`) on `config.features.graph`:

```rust
let emit_graph = config.features.graph;
if emit_graph {
    // ... existing graph page render + write ...
}
```

Gate search index + `search_enabled` template flag on `config.features.search`:

```rust
if config.features.search {
    fs::write(dist_dir.join("search-index.json"),
        docgen_core::search::index_json(&site.search))?;
}
```
(plus thread `config.features.search` into the page/graph render → `search_enabled`;
add a `search_enabled: bool` to `PageContext`/`GraphContext`, replacing the
hard-coded `true` in `Renderer`.)

`EmitOptions.include_graph = config.features.graph`. Pass
`site_title => config.title.as_deref().unwrap_or("")` and `base => &config.base`
into every `render_page`/`render_graph`/`render_history` call (home doc uses just
the title with no suffix? — keep simple: every page uses the suffix; the home
page's own title is already the site name in practice. Tested behaviour: suffix on
all pages.)

**Tests** — `docgen-build/tests/build_site.rs`, using a fixture + a written
`docgen.toml`:

```rust
#[test]
fn graph_feature_off_skips_graph_page_and_island() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"),
        "[features]\ngraph = false\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();
    assert!(!out.path().join("graph/index.html").exists());
    assert!(!out.path().join("islands/graph.js").exists());
}

#[test]
fn search_feature_off_skips_index_and_client() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"),
        "[features]\nsearch = false\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();
    assert!(!out.path().join("search-index.json").exists());
    let home = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(!home.contains("data-docgen-search"));
}

#[test]
fn title_from_config_suffixes_page_titles() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"),
        "title = \"Acme Docs\"\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();
    let intro = std::fs::read_to_string(out.path().join("guide/intro/index.html")).unwrap();
    assert!(intro.contains("— Acme Docs</title>"));
}
```

**Cargo:** `cargo test --workspace && cargo clippy --all-targets`
**Commit:** `feat(build): load docgen.toml and gate graph/search + title/base from config`

---

## Cluster B — custom-component directive system

### B-1. `docgen-components` crate: `Component` + minijinja render (TDD)

**New crate** `crates/docgen-components/Cargo.toml`:

```toml
[package]
name = "docgen-components"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
minijinja = "2.20.0"
serde = { version = "1.0.228", features = ["derive"] }
thiserror = "2.0.18"
include_dir = "0.7"

[dev-dependencies]
tempfile = "3"
```

Add to workspace `members`.

**`crates/docgen-components/src/lib.rs`** — core types:

```rust
//! Custom-component directive registry. A `Component` is a directory
//! `<name>/{template.html, island.js?, style.css?}`. Built-ins ship embedded in
//! `docgen-assets` and load through the SAME `Component::from_parts` path that
//! reads project components — so built-ins dogfood the mechanism. A project
//! component overrides a built-in of the same name.

use std::collections::BTreeMap;
use std::path::Path;

use minijinja::{context, Environment};
use serde::Serialize;

/// One loaded component.
#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,
    pub template: String,
    pub island_js: Option<String>,
    pub style_css: Option<String>,
}

/// The render inputs for a single directive instance.
#[derive(Debug, Clone, Serialize)]
pub struct DirectiveContext {
    pub attrs: BTreeMap<String, String>,
    /// Rendered inner HTML (block form); empty for leaf form.
    pub content: String,
    /// The `[label]` text (leaf form); empty for block form.
    pub label: String,
    /// Unique per-instance id for island wiring.
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ComponentError {
    #[error("component `{name}`: template render failed: {source}")]
    Render { name: String, #[source] source: minijinja::Error },
}

impl Component {
    /// Build a component from its raw parts (used by BOTH project discovery and
    /// the embedded built-in loader).
    pub fn from_parts(
        name: impl Into<String>,
        template: impl Into<String>,
        island_js: Option<String>,
        style_css: Option<String>,
    ) -> Self {
        Self { name: name.into(), template: template.into(), island_js, style_css }
    }

    /// Render this component for one directive instance to HTML.
    pub fn render(&self, ctx: &DirectiveContext) -> Result<String, ComponentError> {
        let mut env = Environment::new();
        // Auto-escape via the `.html` template name so `{{ attrs.x }}`/`{{ label }}`
        // are HTML-escaped; `{{ content | safe }}` stays raw (already rendered HTML).
        env.add_template("c.html", &self.template)
            .map_err(|e| ComponentError::Render { name: self.name.clone(), source: e })?;
        let tmpl = env.get_template("c.html").unwrap();
        tmpl.render(context! {
            attrs => &ctx.attrs,
            content => &ctx.content,
            label => &ctx.label,
            id => &ctx.id,
        }).map_err(|e| ComponentError::Render { name: self.name.clone(), source: e })
    }
}
```

> minijinja note: `add_template` (borrowed) requires the env outlive the str; we
> build a fresh `Environment` per render for simplicity and correctness (component
> count × directive instances is small). If profiling shows cost, a future phase
> caches per-component `Environment`s — out of P6 scope.
> The component template author writes `{{ content | safe }}` for block content.

**Tests:**

```rust
#[test]
fn renders_block_component_with_attrs_and_content() {
    let c = Component::from_parts(
        "callout",
        "<aside class=\"c--{{ attrs.type | default('note') }}\">\
         {% if attrs.title %}<p>{{ attrs.title }}</p>{% endif %}\
         <div>{{ content | safe }}</div></aside>",
        None, None,
    );
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("type".into(), "warning".into());
    attrs.insert("title".into(), "Back up first".into());
    let html = c.render(&DirectiveContext {
        attrs, content: "<p>destructive</p>".into(), label: "".into(), id: "d0".into(),
    }).unwrap();
    assert!(html.contains("c--warning"));
    assert!(html.contains("Back up first"));
    assert!(html.contains("<p>destructive</p>")); // content raw
}

#[test]
fn renders_leaf_component_with_label() {
    let c = Component::from_parts(
        "youtube",
        "<figure><iframe title=\"{{ label }}\" \
         src=\"https://yt/embed/{{ attrs.id }}\"></iframe><figcaption>{{ label }}</figcaption></figure>",
        None, None,
    );
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("id".into(), "abc123".into());
    let html = c.render(&DirectiveContext {
        attrs, content: "".into(), label: "Intro to docgen".into(), id: "d1".into(),
    }).unwrap();
    assert!(html.contains("embed/abc123"));
    assert!(html.contains("Intro to docgen"));
}

#[test]
fn attrs_and_label_are_html_escaped() {
    let c = Component::from_parts("x", "<i title=\"{{ label }}\">{{ attrs.a }}</i>", None, None);
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("a".into(), "<script>".into());
    let html = c.render(&DirectiveContext {
        attrs, content: "".into(), label: "a&b".into(), id: "d".into(),
    }).unwrap();
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("a&amp;b"));
}
```

**Cargo:** `cargo test -p docgen-components && cargo clippy -p docgen-components --all-targets`
**Commit:** `feat(components): Component type + minijinja directive render (escaped attrs/label, raw content)`

---

### B-2. `Registry`: discover project components + built-ins + override (TDD)

Same crate. A `Registry` maps `name -> Component`, plus knows which components are
needed for asset gating.

```rust
/// A name → component map. Built-ins inserted first, project components last
/// (so a project `<name>` overrides a built-in `<name>`).
#[derive(Debug, Clone, Default)]
pub struct Registry {
    map: BTreeMap<String, Component>,
}

impl Registry {
    pub fn empty() -> Self { Self::default() }

    /// Insert (or override) a component by its `name`.
    pub fn insert(&mut self, c: Component) { self.map.insert(c.name.clone(), c); }

    pub fn get(&self, name: &str) -> Option<&Component> { self.map.get(name) }

    pub fn contains(&self, name: &str) -> bool { self.map.contains_key(name) }

    /// All components with an `island.js`, name-sorted — the concatenation order
    /// for the emitted `components.js`.
    pub fn islands(&self) -> Vec<&Component> {
        self.map.values().filter(|c| c.island_js.is_some()).collect()
    }

    /// All components with a `style.css`, name-sorted.
    pub fn styles(&self) -> Vec<&Component> {
        self.map.values().filter(|c| c.style_css.is_some()).collect()
    }
}

/// Read every `<name>/` subdir of `dir` into components. `template.html` is
/// required; a subdir without it is skipped (with no error — a stray dir is not
/// fatal). Missing `dir` → no components (empty). Deterministic (BTreeMap).
pub fn discover(dir: &Path) -> std::io::Result<Vec<Component>> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in rd {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    for name in names {
        let base = dir.join(&name);
        let tpl_path = base.join("template.html");
        let template = match std::fs::read_to_string(&tpl_path) {
            Ok(t) => t,
            Err(_) => continue, // no template.html → not a component
        };
        let island_js = std::fs::read_to_string(base.join("island.js")).ok();
        let style_css = std::fs::read_to_string(base.join("style.css")).ok();
        out.push(Component::from_parts(name, template, island_js, style_css));
    }
    Ok(out)
}

/// Build the full registry: embedded built-ins first, then project components
/// from `project_components_dir` (which override built-ins by name).
pub fn build_registry(
    builtins: Vec<Component>,
    project_dir: &Path,
) -> std::io::Result<Registry> {
    let mut reg = Registry::empty();
    for c in builtins { reg.insert(c); }
    for c in discover(project_dir)? { reg.insert(c); }
    Ok(reg)
}
```

**Tests** (project dir via tempfile):

```rust
fn write_component(root: &Path, name: &str, tpl: &str) {
    let d = root.join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("template.html"), tpl).unwrap();
}

#[test]
fn discovers_project_components_sorted_and_requires_template() {
    let dir = tempfile::tempdir().unwrap();
    write_component(dir.path(), "note", "<div>{{ content | safe }}</div>");
    // a stray dir with no template.html is ignored
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    let comps = discover(dir.path()).unwrap();
    assert_eq!(comps.len(), 1);
    assert_eq!(comps[0].name, "note");
}

#[test]
fn missing_components_dir_is_empty_not_error() {
    let dir = tempfile::tempdir().unwrap();
    let comps = discover(&dir.path().join("nope")).unwrap();
    assert!(comps.is_empty());
}

#[test]
fn project_component_overrides_builtin_of_same_name() {
    let dir = tempfile::tempdir().unwrap();
    write_component(dir.path(), "callout", "<div class=\"project-callout\">{{ content | safe }}</div>");
    let builtin = Component::from_parts("callout", "<div class=\"builtin-callout\"></div>", None, None);
    let reg = build_registry(vec![builtin], dir.path()).unwrap();
    let c = reg.get("callout").unwrap();
    assert!(c.template.contains("project-callout"));
    assert!(!c.template.contains("builtin-callout"));
}

#[test]
fn picks_up_island_and_style_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().join("rating");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("template.html"), "<div></div>").unwrap();
    std::fs::write(d.join("island.js"), "Alpine.data('r',()=>({}))").unwrap();
    std::fs::write(d.join("style.css"), ".r{}").unwrap();
    let comps = discover(dir.path()).unwrap();
    assert!(comps[0].island_js.is_some());
    assert!(comps[0].style_css.is_some());
    let mut reg = Registry::empty();
    reg.insert(comps.into_iter().next().unwrap());
    assert_eq!(reg.islands().len(), 1);
    assert_eq!(reg.styles().len(), 1);
}
```

**Cargo:** `cargo test -p docgen-components && cargo clippy --all-targets`
**Commit:** `feat(components): Registry + project discovery + builtin-override-by-name`

---

### B-3. Built-in `callout` embedded in `docgen-assets` + `builtin_components()` (dogfood)

Create built-in component files:

**`crates/docgen-assets/components/callout/template.html`** (mirrors spec Example A):
```html
<aside class="docgen-callout docgen-callout--{{ attrs.type | default('note') }}">
  {% if attrs.title %}<p class="docgen-callout__title">{{ attrs.title }}</p>{% endif %}
  <div class="docgen-callout__body">{{ content | safe }}</div>
</aside>
```

**`crates/docgen-assets/components/callout/style.css`**:
```css
.docgen-callout{border-left:4px solid var(--cl,#3b82f6);background:#0b1220;
  padding:.75rem 1rem;border-radius:6px;margin:1rem 0}
.docgen-callout--warning{--cl:#f59e0b}
.docgen-callout--danger{--cl:#ef4444}
.docgen-callout--note{--cl:#3b82f6}
.docgen-callout__title{font-weight:600;margin:0 0 .25rem}
.docgen-callout__body>:first-child{margin-top:0}
.docgen-callout__body>:last-child{margin-bottom:0}
```
(no `island.js` → callout is pure build-time HTML, exactly the spec's Example A.)

**`docgen-assets/src/lib.rs`** — embed a second `include_dir!` tree + expose the
built-ins as `(name, template, island_js, style_css)` tuples (bytes only — assets
crate must NOT depend on `docgen-components`, to avoid a dependency cycle, since
`docgen-components` will be a build-time consumer, but `docgen-build` depends on
both; we expose raw parts here and let the build assemble `Component`s):

```rust
/// Embedded built-in component sources (`components/<name>/...`), loaded through
/// the SAME parts a project component is. Returned as raw parts so this crate
/// stays dependency-free of `docgen-components`.
static COMPONENTS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/components");

/// `(name, template, island_js, style_css)` for each embedded built-in.
pub struct BuiltinComponent {
    pub name: &'static str,
    pub template: &'static str,
    pub island_js: Option<&'static str>,
    pub style_css: Option<&'static str>,
}

pub fn builtin_components() -> Vec<BuiltinComponent> {
    fn text(dir: &Dir, rel: &str) -> Option<&'static str> {
        dir.get_file(rel).and_then(|f| f.contents_utf8())
    }
    let mut out = Vec::new();
    for sub in COMPONENTS.dirs() {
        let name = sub.path().file_name().unwrap().to_str().unwrap();
        let tpl = text(&COMPONENTS, &format!("{name}/template.html"))
            .expect("builtin component needs template.html");
        out.push(BuiltinComponent {
            name,
            template: tpl,
            island_js: text(&COMPONENTS, &format!("{name}/island.js")),
            style_css: text(&COMPONENTS, &format!("{name}/style.css")),
        });
    }
    out
}
```

> `Dir::dirs()`/`get_file`/`contents_utf8` are the include_dir 0.7 API. `name` is a
> `&'static str` into the embedded tree → no allocation, lifetimes are `'static`.

**Tests** in `docgen-assets/src/lib.rs`:

```rust
#[test]
fn ships_builtin_callout_component() {
    let comps = builtin_components();
    let c = comps.iter().find(|c| c.name == "callout").expect("callout builtin");
    assert!(c.template.contains("docgen-callout"));
    assert!(c.style_css.is_some());
    assert!(c.island_js.is_none()); // pure build-time
}
```

**Cargo:** `cargo test -p docgen-assets && cargo clippy --all-targets`
**Commit:** `feat(assets): embed built-in callout component (template+style), dogfooding the component mechanism`

---

### B-4. `directivepass` — the source-level extract pass (TDD, the core)

**New** `crates/docgen-core/src/directivepass.rs`. Add `pub mod directivepass;` to
`lib.rs`. Add dep `docgen-components = { path = "../docgen-components" }` to
`docgen-core/Cargo.toml`.

Public surface:

```rust
//! Source-level directive pre/post pass. `extract` rewrites raw markdown,
//! replacing each directive with an HTML-comment sentinel and returning the
//! parsed instances; `substitute` swaps sentinels for rendered component HTML
//! after comrak has formatted the surrounding markdown. See plan §0.2.

use std::collections::BTreeMap;

/// One directive found in a doc body.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveInstance {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub label: String,        // leaf `[label]`; empty for block form
    pub inner_md: String,     // block inner markdown; empty for leaf form
    pub is_block: bool,
}

/// The sentinel a directive is replaced with in the rewritten source. `N` is the
/// instance index. Chosen as an HTML comment so comrak passes it through verbatim.
fn sentinel(idx: usize) -> String { format!("<!--docgen-directive:{idx}-->") }

/// Pass 1: scan `body_md`, replace directives with sentinels, return instances
/// (index-aligned with the sentinels). Unknown-vs-known is NOT decided here —
/// every syntactic directive is extracted; resolution happens in `substitute`.
pub fn extract(body_md: &str) -> (String, Vec<DirectiveInstance>);

/// Parse an attr string (`type=warning title="x y"`) → ordered map. Total.
pub fn parse_attrs(s: &str) -> BTreeMap<String, String>;
```

**Scanner algorithm (block):** iterate lines. A line whose trimmed form matches
`:::<name>{...}` (open) starts a block; collect following lines until a line whose
trimmed form is exactly `:::` (close), **depth-counting** nested `:::name{` opens
so the matching close is the outermost. The collected inner lines (verbatim, the
indentation as authored) become `inner_md`. Replace the whole span with the
sentinel line. A `name` is `[A-Za-z][A-Za-z0-9_-]*`.

**Scanner algorithm (leaf):** within non-directive lines, find inline
`:name[label]{attrs}` occurrences (label and attrs both optional in syntax, but the
leaf form per spec always has `[label]`; `{attrs}` optional). Replace each match
inline with its sentinel. Leaf scanning must NOT match a `:::` block opener (require
the char after `:name` to be `[` or `{`, and that the directive is not preceded by a
second colon). Use a hand-written scan (no regex crate) to stay dependency-light;
a small index walk over the line.

> **Escaping / opt-out:** a directive escaped as `\:::name` (leading backslash) is
> left literal (the backslash is consumed). Keeps authors able to *document* the
> directive syntax. (Tested.)

**Tests** (`#[cfg(test)]`):

```rust
#[test]
fn parse_attrs_handles_bare_quoted_and_empty() {
    let a = parse_attrs("type=warning title=\"Back up first\" wide");
    assert_eq!(a.get("type").unwrap(), "warning");
    assert_eq!(a.get("title").unwrap(), "Back up first");
    assert_eq!(a.get("wide").unwrap(), "true");
    assert!(parse_attrs("").is_empty());
}

#[test]
fn extracts_block_directive_with_inner_markdown() {
    let src = ":::callout{type=warning title=\"Heads up\"}\nThis is **bold**.\n:::\n";
    let (out, inst) = extract(src);
    assert_eq!(inst.len(), 1);
    assert!(inst[0].is_block);
    assert_eq!(inst[0].name, "callout");
    assert_eq!(inst[0].attrs.get("type").unwrap(), "warning");
    assert_eq!(inst[0].inner_md.trim(), "This is **bold**.");
    assert!(out.contains("<!--docgen-directive:0-->"));
    assert!(!out.contains(":::"));
}

#[test]
fn extracts_leaf_directive_with_label_and_attrs() {
    let src = "See :youtube[Intro]{id=abc123} now.\n";
    let (out, inst) = extract(src);
    assert_eq!(inst.len(), 1);
    assert!(!inst[0].is_block);
    assert_eq!(inst[0].name, "youtube");
    assert_eq!(inst[0].label, "Intro");
    assert_eq!(inst[0].attrs.get("id").unwrap(), "abc123");
    assert!(out.contains("See <!--docgen-directive:0--> now."));
}

#[test]
fn nested_block_directives_match_outermost() {
    let src = ":::callout{type=note}\nouter\n:::callout{type=warning}\ninner\n:::\n:::\n";
    let (_out, inst) = extract(src);
    assert_eq!(inst.len(), 1); // only the outer is extracted at this level
    assert!(inst[0].inner_md.contains(":::callout{type=warning}"));
    assert!(inst[0].inner_md.contains("inner"));
}

#[test]
fn escaped_directive_is_left_literal() {
    let src = "\\:::callout{}\nnot a directive\n:::\n";
    let (out, inst) = extract(src);
    assert!(inst.is_empty());
    assert!(out.contains(":::callout{}")); // literal, backslash removed
}

#[test]
fn plain_text_with_colons_is_not_a_directive() {
    let src = "time is 10:30 and ratio 3:4\n";
    let (out, inst) = extract(src);
    assert!(inst.is_empty());
    assert_eq!(out, src);
}
```

**Cargo:** `cargo test -p docgen-core directivepass && cargo clippy --all-targets`
**Commit:** `feat(core): source-level directive extract pass (block + leaf, nested, escaped)`

---

### B-5. `substitute` + recursive inner render + unknown-directive error span (TDD)

Same module. `substitute` needs to render inner markdown — but `directivepass`
lives in `docgen-core`, which owns the markdown pipeline. To avoid a circular call,
`substitute` takes a **closure** `render_inner: &dyn Fn(&str) -> String` (the
full per-doc pipeline, supplied by `render_docs`) so the pass stays decoupled and
unit-testable with a stub renderer.

```rust
/// Pass 2: replace each `<!--docgen-directive:N-->` sentinel in `html` with the
/// component's rendered HTML. `render_inner` renders a block directive's inner
/// markdown to HTML (the full pipeline, recursively). Returns the substituted
/// HTML and the set of component names that were actually rendered (for per-page
/// island/style gating). Unknown directives → a clearly-marked inert error span.
pub fn substitute(
    html: &str,
    instances: &[DirectiveInstance],
    registry: &docgen_components::Registry,
    render_inner: &dyn Fn(&str) -> String,
) -> (String, std::collections::BTreeSet<String>) {
    use docgen_components::DirectiveContext;
    let mut used = std::collections::BTreeSet::new();
    let mut out = html.to_string();
    for (idx, inst) in instances.iter().enumerate() {
        let rendered = match registry.get(&inst.name) {
            Some(component) => {
                let content = if inst.is_block { render_inner(&inst.inner_md) } else { String::new() };
                let ctx = DirectiveContext {
                    attrs: inst.attrs.clone(),
                    content,
                    label: inst.label.clone(),
                    id: format!("docgen-d-{idx}"),
                };
                match component.render(&ctx) {
                    Ok(h) => { used.insert(inst.name.clone()); h }
                    Err(_) => error_span(&inst.name, "template error"),
                }
            }
            None => error_span(&inst.name, "unknown directive"),
        };
        out = out.replace(&sentinel(idx), &rendered);
    }
    (out, used)
}

fn error_span(name: &str, reason: &str) -> String {
    format!(
        "<span class=\"docgen-directive-error\" data-directive=\"{}\">[docgen: {} `{}`]</span>",
        crate::util::escape_html(name), reason, crate::util::escape_html(name)
    )
}
```

**Tests** (stub registry + stub `render_inner` = identity-ish):

```rust
fn reg_with(name: &str, tpl: &str) -> docgen_components::Registry {
    let mut r = docgen_components::Registry::empty();
    r.insert(docgen_components::Component::from_parts(name, tpl, None, None));
    r
}

#[test]
fn substitutes_known_block_component_and_renders_inner() {
    let (html, inst) = extract(":::callout{type=note}\n**hi**\n:::\n");
    let reg = reg_with("callout", "<aside class=\"c--{{ attrs.type }}\">{{ content | safe }}</aside>");
    let render_inner = |md: &str| format!("<p>{}</p>", md.trim().replace("**", ""));
    let (out, used) = substitute(&html, &inst, &reg, &render_inner);
    assert!(out.contains("c--note"));
    assert!(out.contains("<p>hi</p>"));
    assert!(used.contains("callout"));
    assert!(!out.contains("docgen-directive:")); // sentinel gone
}

#[test]
fn unknown_directive_becomes_marked_error_span_not_panic() {
    let (html, inst) = extract(":bogus[x]{}\n");
    let reg = docgen_components::Registry::empty();
    let (out, used) = substitute(&html, &inst, &reg, &|s| s.to_string());
    assert!(out.contains("docgen-directive-error"));
    assert!(out.contains("unknown directive"));
    assert!(out.contains("bogus"));
    assert!(used.is_empty());
}

#[test]
fn directive_name_in_error_is_escaped() {
    let (html, inst) = extract(":<img>[x]{}\n"); // not a valid name → won't extract
    // craft an instance manually to exercise escaping
    let inst = vec![DirectiveInstance{ name:"<img>".into(), attrs:Default::default(),
        label:String::new(), inner_md:String::new(), is_block:false }];
    let html = sentinel_doc(&inst); // helper builds "…<!--docgen-directive:0-->…"
    let (out,_) = substitute(&html, &inst, &docgen_components::Registry::empty(), &|s| s.into());
    assert!(out.contains("&lt;img&gt;"));
    let _ = html;
}
```
(Provide a tiny `fn sentinel_doc` test helper that emits the sentinel for index 0.)

**Cargo:** `cargo test -p docgen-core directivepass && cargo clippy --all-targets`
**Commit:** `feat(core): directive substitution + recursive inner render + unknown→error span`

---

### B-6. Wire directives into `render_docs` (registry param + per-page used set) (TDD)

`render_docs` gains a `registry: &docgen_components::Registry` param. Per doc:

1. `let (rewritten, instances) = directivepass::extract(&p.body_md);`
2. parse `rewritten`; search plaintext from the directive-free AST (unchanged call
   on the new root).
3. wikilink/math/mermaid passes + `format_ast` → `body_html` (as today).
4. `let render_inner = |md: &str| render_block_markdown(md, config, registry);` —
   a free fn that runs the **same** extract→parse→passes→format→substitute pipeline
   on a fragment and returns *inner HTML only* (no page chrome). It recurses through
   `substitute` so nested directives render.
5. `let (body_html, used) = directivepass::substitute(&body_html, &instances, registry, &render_inner);`
6. record `used` on the `Doc` (new field `components_used: BTreeSet<String>`), and a
   site-level `any_components` + `components_used` union on `SiteBuild`.

Add to `model::Doc`:
```rust
/// Names of custom components rendered on this page (drives per-page island load).
#[serde(default)]
pub components_used: std::collections::BTreeSet<String>,
```
Add to `SiteBuild`:
```rust
/// True if any doc used ≥1 component (gates the components asset slice).
pub any_components: bool,
/// True if any *used* component had an island.js (gates components.js emit).
pub any_component_islands: bool,
```

`render_block_markdown` (new pub fn in `pipeline.rs` or a small `block.rs`):

```rust
/// Render a markdown fragment (a block directive's inner content) to inner HTML,
/// running the full directive + AST pipeline but emitting no page chrome.
pub fn render_block_markdown(
    md: &str,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
) -> String {
    let (rewritten, instances) = crate::directivepass::extract(md);
    let options = comrak_options();
    let arena = Arena::new();
    let root = parse_document(&arena, &rewritten, &options);
    // NOTE: wikilinks need the slug set; inner fragments resolve against the empty
    // set here (links inside directives still render as text/anchors per wikilink
    // pass rules). math/mermaid gated by config as in the top-level pass.
    if config.features.math { crate::mathpass::transform_math(root); }
    if config.features.mermaid { crate::mermaidpass::transform_mermaid(root); }
    let inner_html = format_ast(root, &options);
    let render_inner = |m: &str| render_block_markdown(m, config, registry);
    let (out, _used) = crate::directivepass::substitute(&inner_html, &instances, registry, &render_inner);
    out
}
```

> Wikilink resolution inside directive bodies: P6 renders inner fragments without
> the site slug set (a fragment doesn't know it). Wikilinks inside a callout render
> via the wikilink pass with an empty slug set → broken-span styling, which is
> acceptable and documented; full slug threading into fragments is a follow-on. To
> avoid surprising broken links, the top-level pass's wikilink transform still runs
> on the **outer** doc (the directive's *placeholder comment* carries no `[[..]]`),
> so wikilinks **outside** directives are unaffected. (Tested: a wikilink outside a
> callout still resolves.)

Update existing `pipeline.rs` tests + the `docgen-build` call site to pass
`&Registry::empty()`. Add:

```rust
#[test]
fn render_docs_renders_callout_directive_with_inner_markdown() {
    let mut reg = docgen_components::Registry::empty();
    reg.insert(docgen_components::Component::from_parts(
        "callout",
        "<aside class=\"docgen-callout--{{ attrs.type | default('note') }}\">{{ content | safe }}</aside>",
        None, None));
    let prepared = vec![prepare(raw("d.md",
        "# D\n\n:::callout{type=warning}\nBe **careful**.\n:::\n"))];
    let site = render_docs(prepared, &docgen_config::SiteConfig::default(), &reg);
    let h = &site.docs[0].body_html;
    assert!(h.contains("docgen-callout--warning"));
    assert!(h.contains("<strong>careful</strong>")); // inner markdown rendered
    assert!(site.docs[0].components_used.contains("callout"));
    assert!(site.any_components);
}

#[test]
fn unknown_directive_in_doc_yields_error_span_not_crash() {
    let prepared = vec![prepare(raw("d.md", "# D\n\n:nope[x]{}\n"))];
    let site = render_docs(prepared, &docgen_config::SiteConfig::default(),
        &docgen_components::Registry::empty());
    assert!(site.docs[0].body_html.contains("docgen-directive-error"));
    assert!(!site.any_components);
}

#[test]
fn wikilink_outside_directive_still_resolves() {
    let mut reg = docgen_components::Registry::empty();
    reg.insert(docgen_components::Component::from_parts("callout", "<aside>{{ content | safe }}</aside>", None, None));
    let prepared = vec![
        prepare(raw("index.md", "# Home\nSee [[guide]].\n\n:::callout{}\nx\n:::\n")),
        prepare(raw("guide.md", "# Guide\n")),
    ];
    let site = render_docs(prepared, &docgen_config::SiteConfig::default(), &reg);
    assert!(site.docs[0].body_html.contains(r#"href="/guide""#));
}
```

**Cargo:** `cargo test -p docgen-core && cargo clippy --all-targets`
**Commit:** `feat(core): render directives in render_docs (registry param, per-page used set, recursive inner)`

---

### B-7. Component asset emission: concatenated `components.js` + `components.css`

In `docgen-assets`, add a function that takes the concatenated authored bytes (the
build builds the concatenation from the `Registry`, since `docgen-assets` doesn't
depend on `docgen-components`):

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct EmitOptions {
    pub include_katex_runtime: bool,
    pub include_mermaid: bool,
    pub include_graph: bool,
    /// Emit `components.css` (set when any component had a style.css). Default false.
    pub include_component_css: bool,
    /// Emit `components.js` (set when any *used* component had an island.js). Default false.
    pub include_component_js: bool,
}
```

The concatenated component bytes are **authored content**, not embedded vendored
files, so they can't be `&'static`. We add a sibling emit entry point that writes
owned bytes directly (the existing `Asset` is `&'static [u8]`; keep it for embedded
files and add a small owned-write helper):

```rust
/// Write authored component CSS/JS the build concatenated from the registry.
/// `css` / `js` are the already-concatenated bytes (empty → file skipped).
pub fn emit_component_bundle(dist: &Path, css: &str, js: &str) -> std::io::Result<()> {
    if !css.is_empty() {
        std::fs::write(dist.join("components.css"), css)?;
    }
    if !js.is_empty() {
        std::fs::write(dist.join("components.js"), js)?;
    }
    Ok(())
}
```

Update `assets_for` to be a no-op for the two new flags (the bundle is emitted via
`emit_component_bundle`, not `assets_for`, since its bytes are dynamic). Keep the
flags so the planner's exhaustive `assets_for_never_includes_dev_assets` loop still
compiles — extend that loop to iterate the two new bools too.

**Test:**
```rust
#[test]
fn emit_component_bundle_writes_when_nonempty_and_skips_when_empty() {
    let tmp = tempfile::tempdir().unwrap();
    emit_component_bundle(tmp.path(), ".docgen-callout{}", "Alpine.data('x',()=>({}))").unwrap();
    assert!(tmp.path().join("components.css").is_file());
    assert!(tmp.path().join("components.js").is_file());
    let tmp2 = tempfile::tempdir().unwrap();
    emit_component_bundle(tmp2.path(), "", "").unwrap();
    assert!(!tmp2.path().join("components.css").exists());
    assert!(!tmp2.path().join("components.js").exists());
}
```
(Add `tempfile` to `docgen-assets` `[dev-dependencies]`.)

**Cargo:** `cargo test -p docgen-assets && cargo clippy --all-targets`
**Commit:** `feat(assets): emit concatenated component css/js bundle (authored bytes)`

---

### B-8. Build wiring: registry, bundle emission, per-page island gating, template links

`docgen-build/src/lib.rs`:

1. Build the registry:
```rust
let builtins: Vec<docgen_components::Component> = docgen_assets::builtin_components()
    .into_iter()
    .map(|b| docgen_components::Component::from_parts(
        b.name, b.template,
        b.island_js.map(str::to_string), b.style_css.map(str::to_string)))
    .collect();
let components_dir = opts.project_root.join(&config.components.dir);
let registry = docgen_components::build_registry(builtins, &components_dir)?;
```
2. `let site = render_docs(prepared, &config, &registry);`
3. Concatenate styles (all registry components, name-sorted) + islands (only
   components in `site.components_used` union, name-sorted):
```rust
let css: String = registry.styles().iter()
    .filter_map(|c| c.style_css.as_deref()).collect::<Vec<_>>().join("\n");
let used: BTreeSet<&str> = site.docs.iter()
    .flat_map(|d| d.components_used.iter().map(String::as_str)).collect();
let js: String = registry.islands().iter()
    .filter(|c| used.contains(c.name.as_str()))
    .filter_map(|c| c.island_js.as_deref()).collect::<Vec<_>>().join("\n");
docgen_assets::emit_component_bundle(dist_dir, &css, &js)?;
```
4. Per-page gating: add `has_components_css: bool` (any registry style exists) and
   `has_component_island: bool` (`!doc.components_used.is_empty()` AND that doc used
   a component with an island) to `PageContext`. Template:
```html
{% if has_components_css %}<link rel="stylesheet" href="/components.css" />{% endif %}
...
{% if has_component_island %}<script src="/components.js"></script>{% endif %}
```
   `components.css` linked on every page when any component style exists (styles are
   small + cacheable); `components.js` linked only on pages using an island
   component (per-page gating, matching mermaid).

**Integration test** — `docgen-build/tests/components.rs` (new file):

```rust
// Scaffold a project: a doc using the built-in callout + a project component, build,
// assert both rendered and the asset bundle exists.
#[test]
fn build_renders_builtin_callout_and_project_component() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/index.md"),
        "# Home\n\n:::callout{type=warning title=\"Heads up\"}\nBe **careful**.\n:::\n\n:note[hi]{}\n").unwrap();
    // project component `note` (leaf)
    let nd = root.path().join("components/note");
    std::fs::create_dir_all(&nd).unwrap();
    std::fs::write(nd.join("template.html"), "<span class=\"note\">{{ label }}</span>").unwrap();
    std::fs::write(nd.join("style.css"), ".note{color:teal}").unwrap();

    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();

    let home = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(home.contains("docgen-callout--warning")); // built-in callout
    assert!(home.contains("Heads up"));
    assert!(home.contains("<strong>careful</strong>")); // inner markdown
    assert!(home.contains("class=\"note\">hi")); // project leaf component
    assert!(home.contains(r#"href="/components.css""#));
    // callout + note are island-free → no components.js linked/emitted
    assert!(!out.path().join("components.js").exists());
    let css = std::fs::read_to_string(out.path().join("components.css")).unwrap();
    assert!(css.contains("docgen-callout")); // built-in style bundled
    assert!(css.contains(".note")); // project style bundled
}

#[test]
fn project_component_overrides_builtin_callout_in_build() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/index.md"),
        "# Home\n:::callout{}\nx\n:::\n").unwrap();
    let cd = root.path().join("components/callout");
    std::fs::create_dir_all(&cd).unwrap();
    std::fs::write(cd.join("template.html"),
        "<div class=\"my-callout\">{{ content | safe }}</div>").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();
    let home = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(home.contains("my-callout"));
    assert!(!home.contains("docgen-callout--note")); // builtin overridden
}

#[test]
fn island_component_emits_components_js_only_on_pages_that_use_it() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/index.md"),
        "# Home\n:::rating{id=p max=5}\n:::\n").unwrap();
    std::fs::write(root.path().join("docs/plain.md"), "# Plain\nno directive\n").unwrap();
    let rd = root.path().join("components/rating");
    std::fs::create_dir_all(&rd).unwrap();
    std::fs::write(rd.join("template.html"),
        "<div x-data=\"docgenRating()\" data-id=\"{{ attrs.id }}\"></div>").unwrap();
    std::fs::write(rd.join("island.js"), "Alpine.data('docgenRating',()=>({}))").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions { project_root: root.path(), out_dir: out.path(),
        mode: BuildMode::Production }).unwrap();
    assert!(out.path().join("components.js").is_file());
    let home = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    let plain = std::fs::read_to_string(out.path().join("plain/index.html")).unwrap();
    assert!(home.contains(r#"src="/components.js""#));
    assert!(!plain.contains(r#"src="/components.js""#)); // gated per-page
}
```

Add `docgen-components` + `docgen-config` deps to `docgen-build/Cargo.toml`.

**Cargo:** `cargo test --workspace && cargo clippy --all-targets`
**Commit:** `feat(build): registry wiring, component bundle emit, per-page island gating + template links`

---

## Cluster C — `docgen init` scaffold + binary distribution tooling

### C-1. `docgen-init` crate: embedded template tree + `scaffold` (TDD)

**New crate** `crates/docgen-init/Cargo.toml`:

```toml
[package]
name = "docgen-init"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
anyhow = "1.0.102"
include_dir = "0.7"

[dev-dependencies]
tempfile = "3"
```

Add to workspace `members`.

**Embedded template tree** `crates/docgen-init/template/` (authored files,
`_gitignore` → `.gitignore` on write, mirroring the original scaffolder's dotfile
rename):

- `template/docgen.toml`:
  ```toml
  title = "My Docs"
  base = ""

  [features]
  graph = true
  math = true
  mermaid = true
  search = true

  [components]
  dir = "components"
  ```
- `template/_gitignore`:
  ```
  /dist
  ```
- `template/docs/index.md` — home doc with a wikilink + the built-in callout:
  ```md
  ---
  title: Welcome
  ---
  # Welcome to your docs

  This is the home page. Edit `docs/index.md` to change it.

  Cross-link pages with `[[guide]]` wikilinks, get a sidebar, search (Ctrl+K),
  backlinks and a link graph for free.

  :::callout{type=warning title="Heads up"}
  This callout is a **built-in component**. Override it by adding
  `components/callout/template.html`.
  :::

  Try the project component below: :note[a project component]{}
  ```
- `template/docs/guide.md`:
  ```md
  # Guide

  Back to [[index|home]]. Inline math: $E = mc^2$.

  ```mermaid
  graph TD; A[Write] --> B[Build] --> C[Deploy];
  ```
  ```
  (NOTE: the embedded file uses a 4-backtick outer fence so the inner mermaid fence
  survives — authored carefully so the literal file contains a real ```` ```mermaid````
  block.)
- `template/components/note/template.html`:
  ```html
  <span class="docgen-note">📝 {{ label }}</span>
  ```
- `template/components/note/style.css`:
  ```css
  .docgen-note{background:#0f2a2a;color:#7fffd4;padding:.1rem .4rem;border-radius:4px}
  ```

**`crates/docgen-init/src/lib.rs`:**

```rust
//! `docgen init` scaffolds a fresh site from an embedded template tree. Replaces
//! the Node `create-docgen`. Plain-file copy + `_gitignore`→`.gitignore` rename.

use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};

static TEMPLATE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/template");

pub struct InitOptions {
    /// Target directory to scaffold into (created if missing).
    pub target: PathBuf,
    /// Overwrite existing files if the dir is non-empty. Default false → error.
    pub force: bool,
}

/// Scaffold a new docgen site into `opts.target`. Errors if the target is a
/// non-empty dir and `force` is false (so we never clobber an existing project).
pub fn scaffold(opts: &InitOptions) -> anyhow::Result<()> {
    if opts.target.exists()
        && opts.target.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false)
        && !opts.force
    {
        anyhow::bail!(
            "target {} is not empty (use --force to scaffold anyway)",
            opts.target.display()
        );
    }
    std::fs::create_dir_all(&opts.target)?;
    write_dir(&TEMPLATE, &opts.target)?;
    Ok(())
}

/// Map a template path component, applying the `_gitignore` → `.gitignore` rename.
fn rename_dotfile(name: &str) -> String {
    match name {
        "_gitignore" => ".gitignore".to_string(),
        other => other.to_string(),
    }
}

fn write_dir(dir: &Dir, target: &Path) -> std::io::Result<()> {
    for file in dir.files() {
        let rel = file.path();
        let mut dest = target.to_path_buf();
        for comp in rel.components() {
            dest.push(rename_dotfile(&comp.as_os_str().to_string_lossy()));
        }
        if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent)?; }
        std::fs::write(&dest, file.contents())?;
    }
    for sub in dir.dirs() {
        write_dir(sub, target)?;
    }
    Ok(())
}
```

> include_dir flattens nested paths in `file.path()` (e.g. `docs/index.md`), so the
> single `write_dir` recursion over `dirs()` + `files()` rebuilds the tree;
> `rename_dotfile` only special-cases the leaf `_gitignore`.

**Tests** `crates/docgen-init/src/lib.rs`:

```rust
#[test]
fn scaffolds_expected_tree_into_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let t = dir.path().join("site");
    scaffold(&InitOptions { target: t.clone(), force: false }).unwrap();
    assert!(t.join("docgen.toml").is_file());
    assert!(t.join(".gitignore").is_file());        // renamed from _gitignore
    assert!(t.join("docs/index.md").is_file());
    assert!(t.join("docs/guide.md").is_file());
    assert!(t.join("components/note/template.html").is_file());
    // _gitignore must NOT survive un-renamed
    assert!(!t.join("_gitignore").exists());
}

#[test]
fn refuses_nonempty_dir_without_force() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("existing.txt"), "x").unwrap();
    let err = scaffold(&InitOptions { target: dir.path().to_path_buf(), force: false });
    assert!(err.is_err());
}

#[test]
fn force_overwrites_nonempty_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("existing.txt"), "x").unwrap();
    scaffold(&InitOptions { target: dir.path().to_path_buf(), force: true }).unwrap();
    assert!(dir.path().join("docgen.toml").is_file());
}

#[test]
fn scaffolded_docgen_toml_parses_with_config_loader() {
    // cross-crate sanity: the embedded config is valid for docgen-config.
    let dir = tempfile::tempdir().unwrap();
    scaffold(&InitOptions { target: dir.path().to_path_buf(), force: true }).unwrap();
    let cfg = docgen_config::load(dir.path()).unwrap();
    assert_eq!(cfg.title.as_deref(), Some("My Docs"));
}
```
(Add `docgen-config = { path = "../docgen-config" }` to `docgen-init`
`[dev-dependencies]` for the last test.)

**Cargo:** `cargo test -p docgen-init && cargo clippy --all-targets`
**Commit:** `feat(init): docgen-init scaffold from embedded template (docs+components+config+gitignore)`

---

### C-2. `docgen init` CLI subcommand + init→build smoke (TDD)

`crates/docgen/Cargo.toml` — add `docgen-init = { path = "../docgen-init" }`.

`crates/docgen/src/main.rs` — new variant:

```rust
/// Scaffold a new docgen site (replaces create-docgen).
Init {
    /// Target directory (defaults to the current directory).
    #[arg(default_value = ".")]
    dir: PathBuf,
    /// Scaffold even if the target dir is non-empty.
    #[arg(long, default_value_t = false)]
    force: bool,
},
```
```rust
Command::Init { dir, force } => {
    docgen_init::scaffold(&docgen_init::InitOptions { target: dir.clone(), force })?;
    println!("Scaffolded a new docgen site at {}", dir.display());
    println!("Next: cd {} && docgen dev", dir.display());
    Ok(())
}
```

**Integration smoke** `crates/docgen/tests/init_build.rs` — the headline
init→build test:

```rust
use std::path::PathBuf;
use std::process::Command;

#[test]
fn init_then_build_renders_the_scaffolded_site_with_custom_component() {
    let tmp = std::env::temp_dir().join(format!("docgen_init_build_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // `docgen init <tmp> --force`
    let st = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("init").arg(&tmp).arg("--force").status().unwrap();
    assert!(st.success());
    assert!(tmp.join("docgen.toml").is_file());
    assert!(tmp.join("docs/index.md").is_file());
    assert!(tmp.join("components/note/template.html").is_file());

    // `docgen build <tmp>`
    let st = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build").arg(&tmp).status().unwrap();
    assert!(st.success());

    // Root page exists (the A-3 fix) and renders BOTH the built-in callout and the
    // project `note` component from the scaffolded content.
    let home = std::fs::read_to_string(tmp.join("dist/index.html")).unwrap();
    assert!(home.contains("docgen-callout--warning")); // built-in, dogfooded
    assert!(home.contains("Heads up"));
    assert!(home.contains("docgen-note"));             // project leaf component
    assert!(home.contains("a project component"));     // its label
    assert!(home.contains("— My Docs</title>"));       // config title suffix
    assert!(tmp.join("dist/components.css").is_file()); // bundled component styles
    // guide page: wikilink resolved + mermaid island gated on
    let guide = std::fs::read_to_string(tmp.join("dist/guide/index.html")).unwrap();
    assert!(guide.contains(r#"href="/index""#));
    assert!(guide.contains(r#"src="/islands/mermaid.js""#));

    let _ = std::fs::remove_dir_all(&tmp);
}
```

**Cargo:** `cargo test -p docgen && cargo clippy --all-targets`
**Commit:** `feat(cli): docgen init subcommand + init→build integration smoke (custom component renders)`

---

### C-3. Binary distribution tooling (files only — NOT executed/pushed/tagged)

**`crates/docgen/Cargo.toml`** — cargo-binstall metadata + a richer `[package]`
block so released archives are discoverable:

```toml
[package]
name = "docgen"
edition.workspace = true
license.workspace = true
version.workspace = true
description = "Cargo-only static documentation-site generator"
repository = "https://github.com/iammaxim/docgen-rs"

[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/docgen-{ target }{ archive-suffix }"
bin-dir = "docgen-{ target }/{ bin }{ binary-ext }"
pkg-fmt = "tgz"

[package.metadata.binstall.overrides.x86_64-pc-windows-msvc]
pkg-fmt = "zip"
```

> binstall resolves `{ repo }`/`{ version }`/`{ target }`/`{ archive-suffix }` at
> install time. We DO NOT set a real repo release; the metadata is inert tooling.

**`.github/workflows/release.yml`** (tag-triggered cross-compile + upload — present
but never triggered here):

```yaml
name: release
on:
  push:
    tags: ['v*']
permissions:
  contents: write
jobs:
  build:
    name: ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: ubuntu-latest,  target: x86_64-unknown-linux-gnu,  ext: tgz }
          - { os: ubuntu-latest,  target: aarch64-unknown-linux-gnu, ext: tgz }
          - { os: macos-latest,   target: x86_64-apple-darwin,       ext: tgz }
          - { os: macos-latest,   target: aarch64-apple-darwin,      ext: tgz }
          - { os: windows-latest, target: x86_64-pc-windows-msvc,    ext: zip }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: '${{ matrix.target }}' }
      - name: Install cross deps (linux aarch64)
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          sudo apt-get update
          sudo apt-get install -y gcc-aarch64-linux-gnu
          echo 'CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc' >> "$GITHUB_ENV"
      - name: Build
        run: cargo build --release --locked -p docgen --target ${{ matrix.target }}
      - name: Package (unix)
        if: matrix.ext == 'tgz'
        run: |
          dir="docgen-${{ matrix.target }}"
          mkdir "$dir"
          cp "target/${{ matrix.target }}/release/docgen" "$dir/"
          tar czf "$dir.tgz" "$dir"
      - name: Package (windows)
        if: matrix.ext == 'zip'
        shell: pwsh
        run: |
          $dir = "docgen-${{ matrix.target }}"
          New-Item -ItemType Directory $dir | Out-Null
          Copy-Item "target/${{ matrix.target }}/release/docgen.exe" $dir
          Compress-Archive -Path $dir -DestinationPath "$dir.zip"
      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            docgen-${{ matrix.target }}.tgz
            docgen-${{ matrix.target }}.zip
```

**`README.md`** (create or extend at repo root) — install section:

```md
## Install

### Prebuilt binary (recommended — no toolchain)
With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

    cargo binstall docgen

Or download an archive for your platform from the
[Releases](https://github.com/iammaxim/docgen-rs/releases) page and put `docgen`
on your `PATH`.

### From source

    cargo install --path crates/docgen

## Quick start

    docgen init my-docs
    cd my-docs
    docgen dev          # http://localhost:4321 with live reload
    docgen build        # static site in ./dist
```

> **Distribution guardrail:** this step only WRITES files. We never run the
> workflow, never `git tag`, never push, never create a GitHub release. A CI test
> only validates the YAML is parseable (next).

**Test** — a lightweight repo-shape test in `crates/docgen/tests/dist_meta.rs`:

```rust
#[test]
fn release_workflow_and_binstall_metadata_exist() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().unwrap().parent().unwrap();
    let wf = repo.join(".github/workflows/release.yml");
    assert!(wf.is_file(), "release workflow missing");
    let wf_text = std::fs::read_to_string(&wf).unwrap();
    assert!(wf_text.contains("cargo build --release"));
    assert!(wf_text.contains("x86_64-unknown-linux-gnu"));

    let cargo = std::fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("[package.metadata.binstall]"));
    assert!(cargo.contains("pkg-url"));

    let readme = std::fs::read_to_string(repo.join("README.md")).unwrap();
    assert!(readme.contains("cargo binstall docgen"));
    assert!(readme.contains("docgen init"));
}
```

**Cargo:** `cargo test -p docgen && cargo clippy --all-targets`
**Commit:** `chore(dist): release workflow + cargo-binstall metadata + README install docs (tooling only)`

---

### C-4. Final green sweep + close-out

Run the full workspace gate. Fix any cross-crate fallout (mostly `PageContext`
literal churn from A-4/B-8 already handled in-step).

```
cargo test --workspace
cargo clippy --all-targets -- -D warnings
```

Update `crates/docgen-build` doc comment to mention config + components. Add a
`docs/` note (or extend the scaffolded README) documenting the directive syntax +
component convention for end users — but **do not** create a top-level summary
`.md` report.

**Commit:** `chore(overnight): mark P6 green — config, root-/, directives, init, distribution`

---

## Test inventory (what proves P6 done)

| Area | Test | Asserts |
| --- | --- | --- |
| A config | `docgen-config` unit (5) | default==pre-P6, missing→default, parse, partial, malformed→err |
| A core gates | `pipeline` (2) | math/mermaid feature-off |
| A root-/ | `build_site` (2) | `dist/index.html` == nested copy; absent index → no root |
| A render | `docgen-render` (3) | title suffix, plain title, `<base>` |
| A build | `build_site` (3) | graph-off, search-off, title suffix in output |
| B component | `docgen-components` (3) | block render, leaf render, escaping |
| B registry | `docgen-components` (4) | discover/sorted/req-template, missing→empty, override, island+style |
| B assets | `docgen-assets` (2) | builtin callout embedded; bundle write/skip |
| B extract | `directivepass` (6) | attrs, block, leaf, nested, escaped, plain-colon |
| B substitute | `directivepass` (3) | known render, unknown→error span, name escaped |
| B render_docs | `pipeline` (3) | callout in doc, unknown→span, wikilink-outside |
| B build e2e | `components.rs` (3) | builtin+project render, override, per-page island gating |
| C init | `docgen-init` (4) | tree, refuse-nonempty, force, config-parses |
| C smoke | `init_build.rs` (1) | init→build → callout+note rendered at `/`, title suffix |
| C dist | `dist_meta.rs` (1) | workflow + binstall meta + README present |

---

## Public API delta (the BUILD_STATUS surface)

- **Config:** `docgen_config::{SiteConfig, Features, ComponentsConfig, ConfigError, load}`.
  `SiteConfig { title: Option<String>, base: String, features: Features, components: ComponentsConfig }`,
  `Features { graph, math, mermaid, search: bool }` (all default `true`),
  `load(&Path) -> Result<SiteConfig, ConfigError>` (missing file → default).
- **Directive/component:**
  `docgen_components::{Component, Registry, DirectiveContext, ComponentError, discover, build_registry}`;
  `Component::from_parts(name, template, island_js, style_css)`,
  `Component::render(&DirectiveContext) -> Result<String, _>`,
  `Registry::{empty, insert, get, contains, islands, styles}`.
  `docgen_core::directivepass::{DirectiveInstance, extract, substitute, parse_attrs}`;
  `docgen_core::pipeline::render_docs(Vec<PreparedDoc>, &SiteConfig, &Registry) -> SiteBuild`
  (two new params); `render_block_markdown(&str, &SiteConfig, &Registry) -> String`.
  `docgen_assets::{BuiltinComponent, builtin_components, emit_component_bundle,
  EmitOptions{ +include_component_css, +include_component_js }}`.
  `Doc { +components_used: BTreeSet<String> }`,
  `SiteBuild { +any_components, +any_component_islands: bool }`.
  `PageContext { +site_title, +base, +has_components_css, +has_component_island, +search_enabled }`.
- **Init entry:** `docgen_init::{InitOptions{ target, force }, scaffold(&InitOptions) -> anyhow::Result<()>}`;
  CLI `docgen init [dir] [--force]`.
