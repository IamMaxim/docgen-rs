# docgen-rs P3 — Assets crate + build-time KaTeX + Mermaid island

**Date:** 2026-06-05
**Phase:** P3 (math + mermaid + the asset-hosting crate that P4/P5 build on)
**Branch:** `overnight/p1-p6` (local only — never push/PR)
**Status:** Plan approved, not yet implemented
**Spec:** `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`
(sections: *Islands*, *Custom-component system*, *Decisions* — KaTeX/Mermaid/Asset-embedding rows)

---

## 0. Scope, decisions, and what P3 inherits

### 0.1 Goal restated

Three clusters, landed strictly in order:

- **Cluster A — `docgen-assets` crate + Alpine infra.** A new lib crate that *owns* every
  vendored frontend file (Alpine.js, the docgen island glue, the shared CSS, and the big libs
  added in B/C). It embeds them with `include_dir!`, exposes a typed enumerate/emit API, and
  defines the **island-registration convention** that P4 (graph) and P5 (editor) will plug into.
  It generalizes P1's ad-hoc `SEARCH_JS` / `DOCGEN_CSS` consts so *all* assets flow through one
  crate.

- **Cluster B — build-time KaTeX.** Turn on comrak's math extension, walk the AST for math
  nodes, render each to HTML **at build time** with the `katex` Rust crate (zero runtime JS),
  and ship vendored KaTeX CSS + woff2 fonts so the rendered HTML displays correctly.

- **Cluster C — Mermaid island.** Detect ` ```mermaid ` fenced blocks, emit an inert container
  with the diagram source, and hydrate it via a lazy-loaded vendored `mermaid.min.js` driven by
  the *first real Alpine island* — loaded **only on pages that contain a diagram**.

Pure-Rust unit tests assert: KaTeX HTML output, asset enumeration/emission, the mermaid
container markup, and the `<head>`/bootstrap wiring. In-browser rendering is validated
separately by the architect (manual). No npm, no node, no bundler, no WASM.

### 0.2 FLAGGED DECISION — KaTeX path: **BUILD-TIME (the `katex` crate). Confirmed.**

Probed on this machine before writing the plan:

```
$ cargo new katexprobe --lib && cd katexprobe && cargo add katex   # → katex v0.4.6, default feature quick-js
$ cargo test    # render("E=mc^2") + render_with_opts(display_mode) → PASS
    Compiling libquickjs-sys v0.9.0
    Compiling quick-js v0.4.1
    Compiling katexprobe ...
    Finished in 8.40s ; test renders ... ok
```

The `katex` crate's default `quick-js` backend (a bundled QuickJS C engine via
`libquickjs-sys`) **compiles and renders cleanly in ~8s** on this darwin/arm machine, with a C
toolchain already present. Therefore we take the spec-preferred path:

> **Build-time render → zero runtime JS for math.** No `katex.js` / `auto-render.js` ships in the
> default build. We still vendor `katex.min.css` + the 16 woff2 fonts because the *output HTML*
> needs them to display. The QuickJS engine is a **build-time-only** dependency — it never ships
> in `dist/` and never touches npm.

**Fallback (documented, NOT taken):** if a future machine cannot build `libquickjs-sys`, switch
`katex` to its `wasm-js` feature, or fall back to the spec-sanctioned runtime path (vendor
`katex.min.js` + `auto-render.min.js`, lazy-load like Mermaid). We pre-vendor those two JS files
into `docgen-assets` anyway (cheap, ~280 KB) and gate them behind a feature so the fallback is a
one-line flip, not a re-vendor. See B-8.

### 0.3 What P3 inherits and must not break

- `docgen-core::pipeline` is **two-pass**: `prepare` (per-doc) then `render_docs` (cross-doc).
  Math + mermaid AST passes hook into `render_docs`, *after* `transform_wikilinks`, *before*
  `format_ast`, reusing the single shared `comrak_options()`.
- `comrak_options()` in `docgen-core::markdown` is the **single source of truth** for parse +
  render options. Math extension flags go here so the AST pass and `format_ast` agree.
- `comrak` 0.52: math fields are `options.extension.math_dollars` and
  `options.extension.math_code`; the AST node is `NodeValue::Math(NodeMath{ dollar_math,
  display_math, literal })` (verified in the vendored source).
- P1 shipped `docgen-render::{SEARCH_JS, DOCGEN_CSS}` as `include_str!` consts, emitted by
  `crates/docgen/src/build.rs` (the `build` subcommand — there is **no Cargo `build.rs` script**;
  the prompt's "build.rs" means this file). The page template lives at
  `crates/docgen-render/templates/page.html` and already injects `/docgen.css` + `/search.js`.
- The search trigger is a plain `<button data-docgen-search>` + `defer`-loaded `/search.js`. P3
  **keeps it working as-is** (see A-9 for the migration decision).
- Tests use temp dirs keyed by `std::process::id()`, `CARGO_BIN_EXE_docgen`, and copy fixtures
  from `fixtures/`. Match this exactly.

### 0.4 Public types kept consistent across clusters (define once in A, reused in B & C)

```rust
// docgen-assets — the contract every cluster depends on.
pub struct Asset {
    pub path: &'static str,      // dist-relative, e.g. "docgen.css", "vendor/katex/katex.min.css"
    pub bytes: &'static [u8],    // embedded contents
    pub kind: AssetKind,
}
pub enum AssetKind { Css, Js, Font, Json, Other }

pub struct EmitOptions {
    pub include_katex_runtime: bool, // false by default (build-time path); true = fallback
    pub include_mermaid: bool,       // true if any page used a mermaid block
}

pub fn core_assets() -> Vec<Asset>;                 // always emitted: alpine, bootstrap, css, search
pub fn katex_css_assets() -> Vec<Asset>;            // css + 16 fonts (build-time path)
pub fn katex_runtime_assets() -> Vec<Asset>;        // katex.min.js + auto-render.min.js (fallback only)
pub fn mermaid_assets() -> Vec<Asset>;              // mermaid.min.js + island glue
pub fn assets_for(opts: &EmitOptions) -> Vec<Asset>;// the planner used by the build subcommand
pub fn emit(assets: &[Asset], dist: &std::path::Path) -> std::io::Result<()>;
```

Cluster B adds `docgen-core::math` (pure render) and uses `katex_css_assets`. Cluster C adds the
mermaid detection pass in `docgen-core` and uses `mermaid_assets`. The build subcommand calls
`assets_for(&EmitOptions{..})` + `emit(...)` instead of P1's two hard-coded `fs::write`s.

---

## CLUSTER A — `docgen-assets` crate + Alpine infrastructure

Strict TDD. Each task: write the failing test, run it red, implement, run green, then
`cargo clippy --all-targets`. Commit per task with the overnight identity.

### A-1. Vendor the frontend assets (curl, pinned)

**No code yet — just fetch + commit the bytes**, so later `include_dir!` has files to embed.

Pinned versions (resolved against jsdelivr on 2026-06-05; all returned HTTP 200):

| lib | version | why pinned here |
| --- | --- | --- |
| alpinejs | **3.14.1** | island framework (spec) |
| katex (css+fonts) | **0.16.11** | matches `katex` crate 0.4.6 output markup |
| katex (js fallback) | **0.16.11** | same version as css → consistent fallback |
| mermaid | **11.2.1** | diagram renderer |

Commands (run from repo root; create dirs first):

```bash
cd /Users/maxim/work/docgen-rs
A=crates/docgen-assets/assets
mkdir -p $A/vendor/alpine $A/vendor/katex/fonts $A/vendor/mermaid

# Alpine (one file, no deps)
curl -fsSL https://cdn.jsdelivr.net/npm/alpinejs@3.14.1/dist/cdn.min.js \
  -o $A/vendor/alpine/alpine.min.js

# KaTeX CSS + the 16 woff2 fonts it @font-face-references
curl -fsSL https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css \
  -o $A/vendor/katex/katex.min.css
for f in AMS-Regular Caligraphic-Bold Caligraphic-Regular Fraktur-Bold Fraktur-Regular \
         Main-Bold Main-BoldItalic Main-Italic Main-Regular Math-BoldItalic Math-Italic \
         SansSerif-Bold SansSerif-Italic SansSerif-Regular Script-Regular Typewriter-Regular; do
  curl -fsSL "https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/fonts/KaTeX_${f}.woff2" \
    -o "$A/vendor/katex/fonts/KaTeX_${f}.woff2"
done

# KaTeX runtime fallback (committed but not emitted by default)
curl -fsSL https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js \
  -o $A/vendor/katex/katex.min.js
curl -fsSL https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js \
  -o $A/vendor/katex/auto-render.min.js

# Mermaid (one bundled file)
curl -fsSL https://cdn.jsdelivr.net/npm/mermaid@11.2.1/dist/mermaid.min.js \
  -o $A/vendor/mermaid/mermaid.min.js
```

**The 16 font names are exact** — derived from `grep -oE 'KaTeX_[A-Za-z]+-[A-Za-z]+\.woff2'`
against the pinned css. KaTeX css references only woff2 in modern builds; we ship woff2 only.

**Sanity gate (not a unit test, a curl-time check):** `katex.min.css` must reference
`fonts/KaTeX_Main-Regular.woff2` with a *relative* `url(fonts/...)`. It does — so emitting css to
`dist/vendor/katex/katex.min.css` and fonts to `dist/vendor/katex/fonts/` keeps the relative path
intact. **Do not** rewrite the css.

`.gitignore` review: ensure `target/` is the only ignore that could swallow these; the assets
live under `crates/`, so they commit normally.

**Commit:** `P3(assets): vendor alpine 3.14.1, katex 0.16.11 css+fonts+js, mermaid 11.2.1`

### A-2. Crate skeleton + workspace wiring (RED first via a trivial test)

`crates/docgen-assets/Cargo.toml`:

```toml
[package]
name = "docgen-assets"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
include_dir = "0.7.4"
```

Add `"crates/docgen-assets"` to the workspace `members` in root `Cargo.toml` (keep it before
`docgen-render` so render can later depend on it cleanly — order is cosmetic but tidy).

`crates/docgen-assets/src/lib.rs` (skeleton):

```rust
use include_dir::{include_dir, Dir};

/// Every vendored frontend file, embedded at compile time.
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");
```

**First test (RED → GREEN), in `src/lib.rs`:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embeds_the_vendored_tree() {
        // Alpine must be embedded and non-empty.
        let alpine = ASSETS.get_file("vendor/alpine/alpine.min.js").expect("alpine embedded");
        assert!(!alpine.contents().is_empty());
    }
}
```

Run `cargo test -p docgen-assets` (red until A-1 files exist + crate compiles), then green.

**Commit:** `P3(assets): docgen-assets crate skeleton with include_dir-embedded vendor tree`

### A-3. The authored (non-vendored) docgen frontend files

These are *our* hand-written files, also embedded. Author them under
`crates/docgen-assets/assets/docgen/`. They migrate P1's `search.js`/`docgen.css` into the new
crate so all assets live in one place.

- **`assets/docgen/docgen.css`** — start as a byte-for-byte copy of
  `crates/docgen-render/assets/docgen.css` (so the existing render tests that assert
  `docgen-search-modal`, `docgen-diff-line--added/removed` keep passing once render re-exports
  from here). Append P3 additions in B-6 (KaTeX block spacing) and C-5 (mermaid container).

- **`assets/docgen/search.js`** — byte-for-byte copy of
  `crates/docgen-render/assets/search.js`. Unchanged behavior.

- **`assets/docgen/bootstrap.js`** — NEW. The Alpine bootstrap + island registry (A-5).

- **`assets/docgen/islands/mermaid.js`** — NEW, authored in C-3.

**Test (extends A-2 tests):**

```rust
#[test]
fn embeds_authored_docgen_files() {
    for p in ["docgen/docgen.css", "docgen/search.js", "docgen/bootstrap.js"] {
        assert!(ASSETS.get_file(p).is_some(), "missing embedded {p}");
    }
}
```

**Commit:** `P3(assets): migrate search.js + docgen.css into docgen-assets, add bootstrap.js`

### A-4. `Asset` / `AssetKind` types + `core_assets()` enumeration

Implement the typed API from §0.4 (the slices the build subcommand will emit). Helper to read an
embedded file into an `Asset`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind { Css, Js, Font, Json, Other }

#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub kind: AssetKind,
}

fn embed(path: &'static str, dist_path: &'static str, kind: AssetKind) -> Asset {
    let bytes = ASSETS
        .get_file(path)
        .unwrap_or_else(|| panic!("embedded asset missing: {path}"))
        .contents();
    Asset { path: dist_path, bytes, kind }
}

/// Assets emitted on every build: Alpine, bootstrap, shared css, search client.
pub fn core_assets() -> Vec<Asset> {
    vec![
        embed("vendor/alpine/alpine.min.js", "vendor/alpine/alpine.min.js", AssetKind::Js),
        embed("docgen/bootstrap.js", "bootstrap.js", AssetKind::Js),
        embed("docgen/docgen.css", "docgen.css", AssetKind::Css),
        embed("docgen/search.js", "search.js", AssetKind::Js),
    ]
}
```

**Tests:**

```rust
#[test]
fn core_assets_cover_alpine_bootstrap_css_search() {
    let paths: Vec<_> = core_assets().iter().map(|a| a.path).collect();
    assert!(paths.contains(&"vendor/alpine/alpine.min.js"));
    assert!(paths.contains(&"bootstrap.js"));
    assert!(paths.contains(&"docgen.css"));
    assert!(paths.contains(&"search.js"));
}

#[test]
fn core_assets_are_nonempty_and_kinded() {
    for a in core_assets() {
        assert!(!a.bytes.is_empty(), "{} empty", a.path);
    }
    assert_eq!(
        core_assets().iter().find(|a| a.path == "docgen.css").unwrap().kind,
        AssetKind::Css
    );
}

// Lock the P1 contract that render tests rely on, now sourced from docgen-assets.
#[test]
fn shared_css_retains_p1_classes() {
    let css = core_assets().iter().find(|a| a.path == "docgen.css").unwrap();
    let s = std::str::from_utf8(css.bytes).unwrap();
    assert!(s.contains("docgen-search-modal"));
    assert!(s.contains("docgen-diff-line--added"));
    assert!(s.contains("docgen-diff-line--removed"));
}
```

**Commit:** `P3(assets): typed Asset/AssetKind API + core_assets() enumeration`

### A-5. Alpine bootstrap + island-registration convention

`assets/docgen/bootstrap.js` — the load-order contract every island (mermaid in C, graph in P4,
editor in P5) plugs into. Authored as a plain classic script (no ESM imports — matches the
`!SEARCH_JS.contains("import ")` discipline). Convention:

```js
// docgen island registry. Islands push a registrar; bootstrap runs them all,
// then starts Alpine exactly once. Lazy libs (mermaid, codemirror) are fetched
// by the island itself inside its registrar/x-init, only when present on the page.
(function () {
  window.docgen = window.docgen || {};
  const islands = (window.docgen.islands = window.docgen.islands || []);

  /** Register an island. fn receives the Alpine global once Alpine is ready. */
  window.docgen.island = function (name, fn) {
    islands.push({ name, fn });
  };

  /** Lazy-load a script once; returns a cached promise. Used by lazy islands. */
  const loaded = {};
  window.docgen.loadScript = function (src) {
    if (loaded[src]) return loaded[src];
    loaded[src] = new Promise((res, rej) => {
      const s = document.createElement('script');
      s.src = src; s.onload = res; s.onerror = rej;
      document.head.appendChild(s);
    });
    return loaded[src];
  };

  document.addEventListener('alpine:init', () => {
    for (const { fn } of islands) {
      try { fn(window.Alpine); } catch (e) { console.error('[docgen island]', e); }
    }
  });
})();
```

Load order in the page (A-7): `bootstrap.js` → each island `.js` (which call
`docgen.island(...)`) → `alpine.min.js` (Alpine fires `alpine:init`, our listener runs the
registrars, then Alpine auto-starts). Island scripts therefore load **before** Alpine.

**Tests (string-contract — JS isn't executed in Rust, but the contract must hold):**

```rust
#[test]
fn bootstrap_defines_registry_and_lazy_loader_without_esm() {
    let js = std::str::from_utf8(
        core_assets().iter().find(|a| a.path == "bootstrap.js").unwrap().bytes
    ).unwrap();
    assert!(js.contains("docgen.island"));
    assert!(js.contains("docgen.loadScript"));
    assert!(js.contains("alpine:init"));
    assert!(!js.contains("import ")); // no ESM / npm
}
```

**Commit:** `P3(assets): Alpine bootstrap + docgen.island registration convention`

### A-6. `emit()` + the `assets_for()` planner

```rust
use std::path::Path;

pub struct EmitOptions {
    /// Ship runtime katex.min.js + auto-render.min.js (fallback path). Default false.
    pub include_katex_runtime: bool,
    /// Ship mermaid.min.js + the mermaid island (only when a page used a diagram).
    pub include_mermaid: bool,
}

impl Default for EmitOptions {
    fn default() -> Self { Self { include_katex_runtime: false, include_mermaid: false } }
}

/// The full asset set to emit for this build. core + katex css (always, for math output)
/// + conditional runtime/mermaid. Idempotent ordering for stable tests.
pub fn assets_for(opts: &EmitOptions) -> Vec<Asset> {
    let mut out = core_assets();
    out.extend(katex_css_assets());                 // implemented in B-5
    if opts.include_katex_runtime { out.extend(katex_runtime_assets()); } // B-8
    if opts.include_mermaid { out.extend(mermaid_assets()); }             // C-4
    out
}

/// Write every asset under `dist`, creating parent dirs. Skips byte-identical rewrites
/// cheaply (always writes; dist is wiped per build, so no staleness concern).
pub fn emit(assets: &[Asset], dist: &Path) -> std::io::Result<()> {
    for a in assets {
        let out = dist.join(a.path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, a.bytes)?;
    }
    Ok(())
}
```

`katex_css_assets`, `katex_runtime_assets`, `mermaid_assets` are introduced as **stubs returning
`vec![]`** here (so the crate compiles), then filled in B-5/B-8/C-4. Note this in code comments.

**Tests:**

```rust
#[test]
fn emit_writes_core_assets_to_disk() {
    let tmp = std::env::temp_dir().join(format!("docgen_assets_emit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    emit(&assets_for(&EmitOptions::default()), &tmp).unwrap();
    assert!(tmp.join("vendor/alpine/alpine.min.js").is_file());
    assert!(tmp.join("bootstrap.js").is_file());
    assert!(tmp.join("docgen.css").is_file());
    assert!(tmp.join("search.js").is_file());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn planner_gates_mermaid_and_katex_runtime() {
    let base = assets_for(&EmitOptions::default());
    assert!(!base.iter().any(|a| a.path.contains("mermaid")));
    assert!(!base.iter().any(|a| a.path.ends_with("katex.min.js")));
    let full = assets_for(&EmitOptions { include_katex_runtime: true, include_mermaid: true });
    // (these assert true once B-8 and C-4 land; until then they document intent)
}
```

**Commit:** `P3(assets): emit() writer + assets_for() planner with stubbed katex/mermaid slices`

### A-7. Page template: load bootstrap + Alpine; keep search working

Edit `crates/docgen-render/templates/page.html`. Add to `<head>` (after the existing
`/docgen.css` link — KaTeX css link is added in B-7, conditionally):

Replace the current trailing script block:

```html
  {% if search_enabled %}
  <button class="docgen-search-trigger" data-docgen-search>Search <kbd>Ctrl K</kbd></button>
  <script src="/search.js" defer></script>
  {% endif %}
```

with:

```html
  {% if search_enabled %}
  <button class="docgen-search-trigger" data-docgen-search>Search <kbd>Ctrl K</kbd></button>
  <script src="/search.js" defer></script>
  {% endif %}
  <!-- island infra: bootstrap defines the registry; islands self-register; Alpine starts last -->
  <script src="/bootstrap.js"></script>
  {% if has_mermaid %}<script src="/islands/mermaid.js"></script>{% endif %}
  <script src="/vendor/alpine/alpine.min.js" defer></script>
```

Add `has_mermaid: bool` to `PageContext` (default false; threaded from C-6) and pass it in
`render_page`. **All existing render tests must keep passing** — they don't set `has_mermaid`, so
add the field with the others and update every `PageContext{..}` literal in tests to
`has_mermaid: false`.

**New render test:**

```rust
#[test]
fn page_loads_bootstrap_and_alpine_and_gates_mermaid_island() {
    let html = renderer().render_page(&PageContext {
        title: "X", slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: false,
    }).unwrap();
    assert!(html.contains(r#"src="/bootstrap.js""#));
    assert!(html.contains(r#"src="/vendor/alpine/alpine.min.js""#));
    assert!(!html.contains("islands/mermaid.js")); // gated off

    let withm = renderer().render_page(&PageContext {
        title: "X", slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: true,
    }).unwrap();
    assert!(withm.contains(r#"src="/islands/mermaid.js""#));
}
```

**Commit:** `P3(render): load Alpine bootstrap in page template, gate mermaid island on has_mermaid`

### A-8. Wire the build subcommand to emit through docgen-assets

In `crates/docgen/Cargo.toml` add `docgen-assets = { path = "../docgen-assets" }`. In
`crates/docgen/src/build.rs` replace P1's two hard-coded asset writes:

```rust
    // OLD (delete):
    fs::write(dist_dir.join("search.js"), docgen_render::SEARCH_JS)?;
    fs::write(dist_dir.join("docgen.css"), docgen_render::DOCGEN_CSS)?;
```

with the planner (the `EmitOptions` flags are computed in B/C; for A use defaults):

```rust
    let emit_opts = docgen_assets::EmitOptions {
        include_katex_runtime: false,         // build-time math path (default)
        include_mermaid,                       // set in C-6 from the site build
    };
    docgen_assets::emit(&docgen_assets::assets_for(&emit_opts), &dist_dir)?;
```

For A, hardcode `let include_mermaid = false;` (C-6 replaces it with the real signal). The search
index `fs::write(... search-index.json ...)` stays. Keep
`docgen_render::{SEARCH_JS, DOCGEN_CSS}` as **deprecated re-exports** for one phase so nothing
else breaks; add `#[deprecated(note = "use docgen-assets")]` and a doc line. (Render's own tests
still reference them — keep them green; they assert the same bytes, now copied into
docgen-assets.)

**Build-CLI test additions** (`crates/docgen/tests/build_cli.rs`):

```rust
    // docgen-assets emitted the island infra.
    assert!(tmp.join("dist/bootstrap.js").is_file());
    assert!(tmp.join("dist/vendor/alpine/alpine.min.js").is_file());
    // search + css still emitted (now via docgen-assets).
    assert!(tmp.join("dist/search.js").is_file());
    assert!(tmp.join("dist/docgen.css").is_file());
```

**Commit:** `P3(build): emit all assets via docgen-assets planner; deprecate render consts`

### A-9. MIGRATION DECISION — search trigger stays as-is (documented)

The P1 search trigger is a `defer`-loaded classic script keyed off
`document.querySelector('[data-docgen-search]')` + a `keydown` listener — it has **no Alpine
dependency** and is fully working. Per the prompt ("migrate IF low-risk; otherwise keep it
working as-is and document why") the decision is: **keep search.js as a standalone script.**
Rationale:

- Zero functional gain from porting: it's a self-contained modal, not reactive state shared with
  other islands.
- Lower risk: rewriting it as an Alpine island would re-touch tested P1 behavior right as we
  introduce Alpine, conflating two changes.
- Coexists cleanly: `search.js` loads `defer`; `bootstrap.js`+islands+Alpine load alongside it
  without ordering conflicts (search doesn't call `docgen.island`).

This is recorded here and in `VENDOR.md`'s notes. A future phase may fold search into the island
system once Alpine is proven by the mermaid island. **No code change in A-9** — it's the explicit
non-migration record. (No separate commit.)

---

## CLUSTER B — build-time KaTeX

Math renders to HTML at build time; no runtime JS in the default path.

### B-1. Turn on the comrak math extension (single source of truth)

Edit `docgen-core::markdown::comrak_options()`:

```rust
    options.extension.math_dollars = true; // $inline$ and $$display$$
    options.extension.math_code = true;    // $`inline`$ code-math form
```

**Test in `markdown.rs`** — proves the AST now carries math nodes (we don't assert comrak's raw
math HTML; the math pass replaces it):

```rust
#[test]
fn math_extension_is_enabled_in_shared_options() {
    let opts = comrak_options();
    assert!(opts.extension.math_dollars);
    assert!(opts.extension.math_code);
}
```

**Commit:** `P3(core): enable comrak math_dollars + math_code in shared options`

### B-2. Pure KaTeX render helper in `docgen-core::math`

Add `katex = "0.4.6"` to `docgen-core/Cargo.toml` (default `quick-js` feature — verified to build
in ~8s). New module `crates/docgen-core/src/math.rs`, declared `pub mod math;` in `lib.rs`.

```rust
use std::sync::OnceLock;

/// Render one math expression to KaTeX HTML at build time.
/// `display` selects block (`$$`) vs inline (`$`) layout.
/// On a KaTeX parse error we fall back to an escaped `<code>` so a bad
/// expression degrades gracefully instead of failing the whole build.
pub fn render_math(src: &str, display: bool) -> String {
    let opts = katex::Opts::builder()
        .display_mode(display)
        .throw_on_error(false)
        .build()
        .expect("katex opts build");
    match katex::render_with_opts(src, &opts) {
        Ok(html) => html,
        Err(_) => format!("<code class=\"docgen-math-error\">{}</code>", escape_html(src)),
    }
}

fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}
```

(`OnceLock` import is reserved for a future opts cache; drop it if clippy flags unused — keep
clippy green.)

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_inline_math_to_katex_html() {
        let html = render_math("E=mc^2", false);
        assert!(html.contains("katex"));
        assert!(!html.contains("katex-display")); // inline → no display wrapper
    }
    #[test]
    fn renders_display_math_with_display_wrapper() {
        let html = render_math("\\int_0^1 x\\,dx", true);
        assert!(html.contains("katex-display"));
    }
    #[test]
    fn bad_expression_degrades_to_escaped_code() {
        let html = render_math("\\frac{", false);
        assert!(html.contains("docgen-math-error"));
        assert!(!html.contains("<script"));
    }
}
```

**Commit:** `P3(core): build-time KaTeX render_math helper (katex crate, throw_on_error=false)`

### B-3. AST math pass — replace `Math` nodes with rendered HTML

New `crates/docgen-core/src/mathpass.rs` (declared `pub mod mathpass;`). Walks the AST and
swaps each `NodeValue::Math` for a `NodeValue::HtmlInline` holding the KaTeX HTML (raw HTML is
allowed through because `comrak_options().render.unsafe = true`). Mirrors the wikilink-pass
recursion style.

```rust
use comrak::nodes::{AstNode, NodeValue};
use crate::math::render_math;

/// Replace every math node in the tree with its build-time KaTeX HTML.
/// Returns the count rendered (lets callers know whether the page used math —
/// reserved for a future `has_math` page signal; not required for output).
pub fn transform_math<'a>(root: &'a AstNode<'a>) -> usize {
    let mut count = 0;
    transform(root, &mut count);
    count
}

fn transform<'a>(node: &'a AstNode<'a>, count: &mut usize) {
    // Collect the math literal/flags first to avoid holding the borrow across mutation.
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
```

Note: display math becomes `HtmlInline` but KaTeX's own `<span class="katex-display">` wrapper is
block-level, so layout is correct; B-6 css adds vertical spacing. (If a future comrak emits
display math as a block node, this still works — we key off `display_math`, not node position.)

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena};
    use crate::markdown::{comrak_options, format_ast};

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
```

**Commit:** `P3(core): AST math pass replacing comrak math nodes with build-time KaTeX HTML`

### B-4. Hook the math pass into `render_docs`

In `pipeline.rs::render_docs`, run the math pass right after `transform_wikilinks`, before
`format_ast`:

```rust
        let pass = transform_wikilinks(root, &arena, &slugs);
        outbound.insert(p.slug.clone(), pass.resolved);
        let math_count = crate::mathpass::transform_math(root);  // NEW
        let body_html = format_ast(root, &options);
```

Plaintext for search is extracted from the pristine AST *before* both passes (already the case at
the top of the loop), so math literals don't leak into the search index — good. Record per-doc
math usage on `Doc` for a future `has_math` head-link toggle:

- Add `pub has_math: bool` to `docgen_core::model::Doc`, set `has_math: math_count > 0`.
- Update every `Doc { .. }` construction + any exhaustive test match. (Search the tree:
  `pipeline.rs` is the only constructor; `assemble.rs`/tests may pattern-match — fix all.)

**Pipeline test** (extends existing `render_docs_*` tests or a new one):

```rust
#[test]
fn render_docs_renders_math_at_build_time() {
    let prepared = vec![prepare(raw("m.md", "# M\nmass: $E=mc^2$\n"))];
    let site = render_docs(prepared);
    assert!(site.docs[0].body_html.contains("katex"));
    assert!(site.docs[0].has_math);
    assert!(!site.docs[0].body_html.contains("$E=mc^2$"));
}
```

**Commit:** `P3(core): run build-time math pass in render_docs; track Doc.has_math`

### B-5. `katex_css_assets()` — fill the stub

Implement in `docgen-assets`, listing css + the 16 fonts (paths derived from the woff2 set
vendored in A-1):

```rust
const KATEX_FONTS: &[&str] = &[
    "AMS-Regular", "Caligraphic-Bold", "Caligraphic-Regular", "Fraktur-Bold",
    "Fraktur-Regular", "Main-Bold", "Main-BoldItalic", "Main-Italic", "Main-Regular",
    "Math-BoldItalic", "Math-Italic", "SansSerif-Bold", "SansSerif-Italic",
    "SansSerif-Regular", "Script-Regular", "Typewriter-Regular",
];

/// KaTeX stylesheet + fonts, always emitted (the build-time-rendered HTML needs them).
pub fn katex_css_assets() -> Vec<Asset> {
    // Font dist paths must stay siblings of the css under vendor/katex/ so the css's
    // relative url(fonts/...) resolves. We use a macro-free static table; include_dir
    // resolves each path at the embedded tree.
    let mut out = vec![embed(
        "vendor/katex/katex.min.css", "vendor/katex/katex.min.css", AssetKind::Css,
    )];
    for f in KATEX_FONTS {
        // Both source and dist paths are the same string; build them via a small leak-free
        // lookup over the embedded Dir, mapping name → &'static [u8] + &'static str path.
        out.push(katex_font_asset(f));
    }
    out
}
```

Because `Asset.path`/`Asset.bytes` are `&'static`, and the 16 font filenames are known constants,
implement `katex_font_asset` with a `match` over the 16 names returning hard-coded
`embed("vendor/katex/fonts/KaTeX_X.woff2", "vendor/katex/fonts/KaTeX_X.woff2", AssetKind::Font)`
calls (no runtime string formatting needed for `&'static str`). A `macro_rules!` over the names
keeps it DRY; or just write the 16 lines explicitly (clearer, clippy-clean). **Prefer explicit 16
lines** to keep `&'static str` literal and avoid leaking.

**Tests:**

```rust
#[test]
fn katex_css_and_all_16_fonts_present() {
    let a = katex_css_assets();
    assert!(a.iter().any(|x| x.path == "vendor/katex/katex.min.css"));
    let fonts = a.iter().filter(|x| x.kind == AssetKind::Font).count();
    assert_eq!(fonts, 16);
    for x in &a { assert!(!x.bytes.is_empty(), "{} empty", x.path); }
}
#[test]
fn katex_css_url_paths_are_relative_to_fonts_dir() {
    let css = katex_css_assets().into_iter().find(|x| x.path.ends_with(".css")).unwrap();
    let s = std::str::from_utf8(css.bytes).unwrap();
    assert!(s.contains("fonts/KaTeX_Main-Regular.woff2")); // relative, matches our dist layout
}
```

**Commit:** `P3(assets): katex_css_assets() — stylesheet + 16 woff2 fonts`

### B-6. KaTeX block spacing CSS (append to docgen.css)

Append to `assets/docgen/docgen.css`:

```css
/* KaTeX build-time math */
.katex-display { margin: 1em 0; overflow-x: auto; overflow-y: hidden; }
.docgen-math-error { color: #b00020; }
```

**Test (in docgen-assets):**

```rust
#[test]
fn shared_css_has_katex_display_spacing() {
    let s = std::str::from_utf8(
        core_assets().iter().find(|a| a.path == "docgen.css").unwrap().bytes
    ).unwrap();
    assert!(s.contains(".katex-display"));
}
```

**Commit:** `P3(assets): KaTeX display spacing + math-error styles`

### B-7. Conditionally link KaTeX css in the page `<head>`

Add `has_math: bool` to `PageContext`, threaded from `Doc.has_math` in the build subcommand. In
`page.html` `<head>`, after `/docgen.css`:

```html
  {% if has_math %}<link rel="stylesheet" href="/vendor/katex/katex.min.css" />{% endif %}
```

Update render tests' `PageContext{..}` literals to add `has_math: false`. New test asserts the
gate both ways (mirrors A-7's mermaid test). The build subcommand sets
`has_math: doc.has_math` in the `render_page` call.

**Commit:** `P3(render): link KaTeX css in head only on pages with math`

### B-8. `katex_runtime_assets()` — fallback slice (vendored, not emitted by default)

Fill the stub with the two pre-vendored JS files (A-1):

```rust
/// Runtime KaTeX (katex.js + auto-render). Emitted ONLY when EmitOptions
/// .include_katex_runtime is set — the documented fallback if the build-time
/// `katex` crate ever cannot compile. Not used by the default build.
pub fn katex_runtime_assets() -> Vec<Asset> {
    vec![
        embed("vendor/katex/katex.min.js", "vendor/katex/katex.min.js", AssetKind::Js),
        embed("vendor/katex/auto-render.min.js", "vendor/katex/auto-render.min.js", AssetKind::Js),
    ]
}
```

**Test:**

```rust
#[test]
fn katex_runtime_slice_has_both_scripts_but_is_off_by_default() {
    let rt = katex_runtime_assets();
    assert!(rt.iter().any(|a| a.path.ends_with("katex.min.js")));
    assert!(rt.iter().any(|a| a.path.ends_with("auto-render.min.js")));
    assert!(!assets_for(&EmitOptions::default()).iter().any(|a| a.path.ends_with("katex.min.js")));
}
```

**Commit:** `P3(assets): vendored runtime-KaTeX fallback slice (off by default)`

---

## CLUSTER C — Mermaid island

The first real Alpine island. Lazy-loaded; mermaid.js loads only on pages with a diagram.

### C-1. Mermaid detection pass in `docgen-core`

New `crates/docgen-core/src/mermaidpass.rs` (`pub mod mermaidpass;`). Walks the AST, finds
fenced code blocks whose info string is `mermaid`, and replaces each with an `HtmlBlock` holding
the island container. The diagram source is preserved verbatim (HTML-escaped) inside the
container so the island can read it without a network round-trip.

```rust
use comrak::nodes::{AstNode, NodeValue};

/// Replace ```mermaid fenced blocks with an island container. Returns the count
/// (lets the page know whether to load the mermaid lib + island).
pub fn transform_mermaid<'a>(root: &'a AstNode<'a>) -> usize {
    let mut count = 0;
    transform(root, &mut count);
    count
}

fn transform<'a>(node: &'a AstNode<'a>, count: &mut usize) {
    let replacement = {
        let data = node.data.borrow();
        if let NodeValue::CodeBlock(cb) = &data.value {
            let lang = cb.info.split_whitespace().next().unwrap_or("");
            if lang.eq_ignore_ascii_case("mermaid") {
                Some(container_html(&cb.literal, *count))
            } else { None }
        } else { None }
    };
    if let Some(html) = replacement {
        node.data.borrow_mut().value = NodeValue::HtmlBlock(comrak::nodes::NodeHtmlBlock {
            block_type: 0,
            literal: html,
        });
        *count += 1;
        return; // replaced; no children to recurse into
    }
    for child in node.children() {
        transform(child, count);
    }
}

fn container_html(src: &str, idx: usize) -> String {
    // x-data island; the raw source sits in a hidden <pre> the island reads + renders.
    format!(
        "<div class=\"docgen-mermaid\" x-data=\"docgenMermaid\" x-init=\"render()\" \
         data-mermaid-id=\"docgen-mermaid-{idx}\">\
         <pre class=\"docgen-mermaid__src\" hidden>{}</pre>\
         <div class=\"docgen-mermaid__out\"></div></div>",
        escape_html(src)
    )
}

fn escape_html(s: &str) -> String { /* same 4-char escaper as math.rs; share via crate util */ }
```

Verify `NodeHtmlBlock`'s exact fields against comrak 0.52 before coding (the vendored
`nodes.rs` shows `block_type: u8` + `literal: String`). Reuse one `escape_html` — lift it into a
small `crate::util` to avoid duplication across `math.rs`/`mermaidpass.rs` (clippy-clean).

**Tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use comrak::{parse_document, Arena};
    use crate::markdown::{comrak_options, format_ast};

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
}
```

**Commit:** `P3(core): mermaid detection pass — fenced mermaid → Alpine island container`

### C-2. Hook the mermaid pass into `render_docs`

After the math pass, before `format_ast`:

```rust
        let math_count = crate::mathpass::transform_math(root);
        let mermaid_count = crate::mermaidpass::transform_mermaid(root); // NEW
        let body_html = format_ast(root, &options);
```

Add `pub has_mermaid: bool` to `Doc`, set `has_mermaid: mermaid_count > 0`. Update all `Doc{..}`
constructions/matches. Add a `pub any_mermaid: bool` to `SiteBuild` (true if any doc has a
diagram) so the build subcommand can flip `EmitOptions.include_mermaid` once for the whole site.

**Pipeline test:**

```rust
#[test]
fn render_docs_marks_mermaid_pages_and_site() {
    let prepared = vec![
        prepare(raw("d.md", "# D\n```mermaid\ngraph TD;A-->B;\n```\n")),
        prepare(raw("p.md", "# P\nplain\n")),
    ];
    let site = render_docs(prepared);
    assert!(site.docs[0].has_mermaid && site.docs[0].body_html.contains("docgen-mermaid"));
    assert!(!site.docs[1].has_mermaid);
    assert!(site.any_mermaid);
}
```

**Commit:** `P3(core): run mermaid pass in render_docs; track Doc.has_mermaid + SiteBuild.any_mermaid`

### C-3. The mermaid island JS (lazy-loads mermaid.min.js)

`crates/docgen-assets/assets/docgen/islands/mermaid.js` — registers via the A-5 convention and
lazy-loads the vendored lib through `docgen.loadScript`:

```js
// Mermaid island: renders each .docgen-mermaid container by lazy-loading the
// vendored mermaid.min.js exactly once, only on pages where a container exists.
window.docgen.island('docgenMermaid', (Alpine) => {
  Alpine.data('docgenMermaid', () => ({
    async render() {
      const el = this.$el;
      const src = el.querySelector('.docgen-mermaid__src');
      const out = el.querySelector('.docgen-mermaid__out');
      if (!src || !out) return;
      await window.docgen.loadScript('/vendor/mermaid/mermaid.min.js');
      const mermaid = window.mermaid;
      mermaid.initialize({ startOnLoad: false });
      const id = el.dataset.mermaidId || ('m-' + Math.random().toString(36).slice(2));
      try {
        const { svg } = await mermaid.render(id + '-svg', src.textContent);
        out.innerHTML = svg;
      } catch (e) {
        out.innerHTML = '<pre class="docgen-mermaid__error"></pre>';
        out.firstChild.textContent = String(e);
      }
    },
  }));
});
```

`docgen.loadScript` caches by URL, so multiple diagrams on one page fetch mermaid once. Classic
script, no ESM imports (the spec's no-npm discipline).

**Test (string contract, in docgen-assets):** add the island to the embedded set and assert:

```rust
#[test]
fn mermaid_island_registers_and_lazy_loads_without_esm() {
    let js = std::str::from_utf8(
        ASSETS.get_file("docgen/islands/mermaid.js").unwrap().contents()
    ).unwrap();
    assert!(js.contains("docgen.island"));
    assert!(js.contains("docgenMermaid"));
    assert!(js.contains("loadScript"));
    assert!(js.contains("/vendor/mermaid/mermaid.min.js"));
    assert!(!js.contains("import "));
}
```

**Commit:** `P3(assets): mermaid Alpine island — lazy-loads vendored mermaid.min.js`

### C-4. `mermaid_assets()` — fill the stub

```rust
/// Mermaid library + island glue. Emitted only when a page used a diagram.
pub fn mermaid_assets() -> Vec<Asset> {
    vec![
        embed("vendor/mermaid/mermaid.min.js", "vendor/mermaid/mermaid.min.js", AssetKind::Js),
        embed("docgen/islands/mermaid.js", "islands/mermaid.js", AssetKind::Js),
    ]
}
```

**Test:**

```rust
#[test]
fn mermaid_slice_has_lib_and_island_and_is_gated() {
    let m = mermaid_assets();
    assert!(m.iter().any(|a| a.path.ends_with("mermaid.min.js")));
    assert!(m.iter().any(|a| a.path == "islands/mermaid.js"));
    let full = assets_for(&EmitOptions { include_mermaid: true, ..Default::default() });
    assert!(full.iter().any(|a| a.path == "islands/mermaid.js"));
    assert!(!assets_for(&EmitOptions::default()).iter().any(|a| a.path.contains("mermaid")));
}
```

**Commit:** `P3(assets): mermaid_assets() slice (mermaid.min.js + island), gated by planner`

### C-5. Mermaid container CSS (append to docgen.css)

```css
/* Mermaid island */
.docgen-mermaid { margin: 1em 0; }
.docgen-mermaid__out svg { max-width: 100%; height: auto; }
.docgen-mermaid__error { color: #b00020; white-space: pre-wrap; }
```

**Test:** assert `core_assets()` css contains `.docgen-mermaid`.

**Commit:** `P3(assets): mermaid container styles`

### C-6. Wire `has_mermaid` + `EmitOptions.include_mermaid` through the build subcommand

In `crates/docgen/src/build.rs`:

- Replace A-8's `let include_mermaid = false;` with `let include_mermaid = site.any_mermaid;`.
- Thread `has_mermaid: doc.has_mermaid` and `has_math: doc.has_math` into the `render_page`
  `PageContext`.

```rust
    let emit_opts = docgen_assets::EmitOptions {
        include_katex_runtime: false,
        include_mermaid: site.any_mermaid,
    };
    docgen_assets::emit(&docgen_assets::assets_for(&emit_opts), &dist_dir)?;
```

**Build-CLI test additions** — add a mermaid + math doc to a fixture (or extend the temp setup)
and assert end-to-end:

```rust
    // A page with a mermaid diagram gets the island wiring + lazy lib emitted.
    let diag = fs::read_to_string(tmp.join("dist/<mermaid-doc-slug>/index.html")).unwrap();
    assert!(diag.contains("docgen-mermaid"));
    assert!(diag.contains(r#"src="/islands/mermaid.js""#));
    assert!(tmp.join("dist/vendor/mermaid/mermaid.min.js").is_file());
    assert!(tmp.join("dist/islands/mermaid.js").is_file());

    // A page with math gets build-time KaTeX HTML + the css link; no katex JS anywhere.
    let math = fs::read_to_string(tmp.join("dist/<math-doc-slug>/index.html")).unwrap();
    assert!(math.contains("katex"));
    assert!(math.contains(r#"href="/vendor/katex/katex.min.css""#));
    assert!(!tmp.join("dist/vendor/katex/katex.min.js").exists()); // build-time path
    assert!(tmp.join("dist/vendor/katex/fonts/KaTeX_Main-Regular.woff2").is_file());
```

Add fixture docs: `fixtures/site-basic/docs/math.md` (`$E=mc^2$` + a `$$...$$` block) and
`fixtures/site-basic/docs/diagram.md` (a `mermaid` fence). Copy them in the test setup like the
existing files. Keep the existing fixture assertions intact.

**Commit:** `P3(build): emit mermaid only when used; thread has_math/has_mermaid into pages`

### C-7. Full green sweep + plan close-out

```bash
cd /Users/maxim/work/docgen-rs
cargo test --workspace
cargo clippy --all-targets -- -D warnings
```

Both must be green. If clippy flags an unused import (e.g. the reserved `OnceLock`), remove it.

**Commit:** `P3: green sweep — all clusters tested, clippy clean`

---

## VENDOR.md (create at repo root in A-1; this is the file's content)

```markdown
# Vendored frontend assets

All files under `crates/docgen-assets/assets/vendor/` are third-party builds fetched from
jsdelivr at pinned versions and committed verbatim. **No npm / node / bundler is used** — these
are static prebuilt files, embedded into the `docgen` binary via `include_dir!`. Re-fetch with
the exact `curl` commands in `docs/superpowers/plans/2026-06-05-docgen-rs-p3-assets-katex-mermaid.md`.

| File (under assets/vendor/) | Package | Version | Source URL | License |
| --- | --- | --- | --- | --- |
| alpine/alpine.min.js | alpinejs | 3.14.1 | https://cdn.jsdelivr.net/npm/alpinejs@3.14.1/dist/cdn.min.js | MIT |
| katex/katex.min.css | katex | 0.16.11 | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css | MIT |
| katex/fonts/KaTeX_*.woff2 (16 files) | katex | 0.16.11 | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/fonts/KaTeX_*.woff2 | MIT (SIL OFL fonts) |
| katex/katex.min.js | katex | 0.16.11 | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js | MIT |
| katex/auto-render.min.js | katex | 0.16.11 | https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js | MIT |
| mermaid/mermaid.min.js | mermaid | 11.2.1 | https://cdn.jsdelivr.net/npm/mermaid@11.2.1/dist/mermaid.min.js | MIT |

## Authored (first-party) files — `crates/docgen-assets/assets/docgen/`

- `docgen.css`, `search.js` — migrated from `docgen-render` in P3.
- `bootstrap.js` — Alpine bootstrap + `docgen.island` registry.
- `islands/mermaid.js` — first Alpine island; lazy-loads `mermaid.min.js`.

## KaTeX strategy (flagged decision)

Math is rendered to HTML **at build time** by the `katex` Rust crate (default `quick-js`
backend), verified to compile + render on this machine (~8s). `katex.min.js` and
`auto-render.min.js` are vendored as a **fallback only** — emitted solely when
`EmitOptions.include_katex_runtime` is set (if a future host cannot build `libquickjs-sys`).
The default build ships **zero runtime JS for math**; it does ship the KaTeX **css + fonts**,
which the build-time-rendered HTML requires for display.

## Search trigger (non-migration note)

The P1 search modal (`search.js`) stays a standalone `defer`-loaded classic script, not an Alpine
island — kept as-is per the low-risk rule. It coexists with the island bootstrap without ordering
conflicts.
```

(`fonts/` woff2 are SIL OFL under KaTeX's distribution; KaTeX code is MIT. Note both.)

---

## Execution order & invariants

1. **A first, end to end** (A-1 … A-9) — the crate + Alpine infra everything else emits through.
2. **B** (B-1 … B-8) — math; depends only on A's `katex_css_assets` stub + page head.
3. **C** (C-1 … C-7) — mermaid; depends on A's island convention + planner gating.

Invariants enforced at every commit:

- `cargo test --workspace` green; `cargo clippy --all-targets -- -D warnings` clean.
- No `import ` / ESM in any shipped JS (matches P1's `!SEARCH_JS.contains("import ")` rule).
- Default build emits **no** `katex.min.js` and emits mermaid **only** when a page uses it.
- All existing P0–P2 tests stay green (page/history render tests get the two new
  `PageContext` fields set to `false`; `Doc` gains `has_math`/`has_mermaid` — update every
  constructor + exhaustive match).
- Commit identity, every time:
  `git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -m "<msg>"`

## Verified facts the plan relies on (probed 2026-06-05)

- `katex` crate v0.4.6, `quick-js` feature — **builds + renders in ~8.4s** on this machine.
- comrak 0.52: `extension.math_dollars`, `extension.math_code`; node
  `NodeValue::Math(NodeMath{ dollar_math, display_math, literal })`; `NodeValue::HtmlInline(String)`,
  `NodeValue::HtmlBlock(NodeHtmlBlock{ block_type: u8, literal: String })`.
- `include_dir` 0.7.4 resolves.
- jsdelivr returns HTTP 200 for all six pinned asset URLs; KaTeX 0.16.11 css references exactly
  16 `KaTeX_*-*.woff2` fonts via **relative** `url(fonts/...)`.
```
