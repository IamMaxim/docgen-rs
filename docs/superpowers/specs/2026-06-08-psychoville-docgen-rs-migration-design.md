# Psychoville docs → docgen-rs migration

**Date:** 2026-06-08
**Status:** Approved design

Rewrite `~/work/Psychoville/docs` from the Svelte-based `docgen` (SvelteKit +
mdsvex `.svx`) to **docgen-rs**: no Svelte, triple-colon directives instead of
Svelte components, all `.svx` → `.md`. Transformation is **in-place** on the
git-tracked tree.

The work spans two repos:

- **docgen-rs** (`~/work/docgen-rs`, cwd) — gains one reusable core feature: a
  built-in `:include` directive plus partial-file exclusion.
- **Psychoville** (`~/work/Psychoville`) — gains a `docgen.toml`, a `rustref`
  custom component, and the migrated `.md` content.

## Source survey (Psychoville/docs)

- **339 `.svx` files**, of which **228 are auto-generated `.gen.svx`** partials
  (`_systems.gen.svx`, `_deps.gen.svx`, `_symbols.gen.svx`, `_files.gen.svx`),
  plus **208 already-`.md`** files.
- Frontmatter is YAML `title` / `description` — already docgen-rs compatible.
- **147** files carry a leading `<script>…</script>` import block.
- Components actually used:
  - **`RustRef`** (660×): `<RustRef ref="a::b::c" href={rustdoc('a/b/c/index.html')} />`
    — inline link to rustdoc. `rustdoc(p)` expands to `${base}/rustdoc/${p}`.
    Some uses omit `href` (Svelte default: `/docs/dev/rust-linking#<anchor>`).
  - **`Systems` / `Symbols` / `Files` / `Deps`** (51× each): used self-closing
    (`<Systems />`) and bound by per-file `import` statements to a sibling
    generated partial (`import Systems from './_systems.gen.svx'`). These are
    transclusions of generated markdown partials.
  - `Badge` / `SrcEmbed` exist in the Svelte component dir but are **unused**.
- No `{#if}` / `{#each}` / `{@html}` in bodies. Stray `{ … }` occurrences are
  inside fenced code (Rust struct literals) — not Svelte interpolation.
- **50** files use ```` ```mermaid ```` fences (docgen-rs supports mermaid).
- A few hardcoded internal links like `](/docs/testing)`.

## Target survey (docgen-rs)

- Project layout: builds `project_root/docs/`. So **project root = `Psychoville/`**,
  docs stay at `Psychoville/docs/` — zero file moves.
- Frontmatter: YAML `title` / `description`.
- Directives: block `:::name{attrs}` and leaf `:name[label]{attrs}`. Built-in
  `callout`. Unknown directives degrade to an inert error span.
- Custom components: `components/<name>/{template.html,style.css}`. Engine is
  **MiniJinja** — `{{ attrs.x }}`, `{{ label }}`, `{{ content | safe }}`,
  `{% if %}`, `| default(...)`. Auto-escapes by default; `| safe` opts out.
- Wikilinks `[[target|label]]` supported and backlink-tracked.
- **Discovery** (`docgen-core/src/discover.rs`) walks every `.md`, pruning only
  hidden dirs (`.`-prefixed) and vendor dirs (`node_modules`, `target`,
  `vendor`, `.git`). It does **not** skip `_`-prefixed files today — so the 228
  generated partials would wrongly become standalone pages. The include feature
  must fix this.

## Design

### 1. docgen-rs core: `:include` directive + partial exclusion

A new **built-in** directive (sits alongside `callout` in the directive
pipeline, not a project component, because it transcludes and recursively
renders a *file*, which is a pipeline concern):

- **Form:** leaf `:include{src="./_systems.gen.md"}`. `src` is resolved
  **relative to the including doc's directory**.
- **Behavior:** read the target file, strip its frontmatter if present, and
  render its markdown through the same recursive directive/markdown pipeline so
  wikilinks, math, mermaid, and nested directives all apply. Splice the rendered
  HTML at the include point.
- **Partial exclusion:** files whose **basename starts with `_`** are excluded
  from *page* discovery (no standalone `/slug` page) but remain valid include
  targets. This matches the existing `_*.gen.*` naming and stops generated
  partials from becoming pages. Implemented in `discover.rs` (skip `_`-prefixed
  `.md` basenames).
- **Robustness:** missing `src` or an include **cycle** → inert error span (same
  failure model as an unknown directive); a depth guard bounds recursion. The
  build never panics on a bad include.
- **Integration:** the directive substitute pass needs (a) the current doc's
  directory to resolve `src`, and (b) a handle to render markdown→HTML. Both are
  threaded through the existing `DirectiveContext` / substitute path.
- **Deliverables:** unit + build-level tests (happy path, nested include,
  missing file, cycle, partial-excluded-from-pages), a `directives.md` fixture
  entry, and a README note.

### 2. Psychoville: project config

`~/work/Psychoville/docgen.toml` at the repo root:

```toml
title = "Psychoville Docs"
base = ""

[features]
graph = true
math = true
mermaid = true
search = true

[components]
dir = "components"
```

Build with `cargo run -p docgen -- build ~/work/Psychoville` (root contains
`docs/`).

### 3. Psychoville: `rustref` component

`~/work/Psychoville/components/rustref/template.html` + `style.css`, ported from
`RustRef.svelte`:

- **Usage:** `:rustref[server::http::websocket]{href="/rustdoc/server/http/websocket/index.html"}`
  — the `::`-path is the leaf **label**; the URL is the `href` attr.
- **template.html** (MiniJinja): renders the existing `<a class="rust-ref rust-ref-link">`
  markup with the rust icon **inlined as SVG** (no separate asset pipeline).
  `href` defaults via `| default('/docs/dev/rust-linking')` when the attr is
  absent. `rel="external"` when the href points at `/rustdoc/`.
- **style.css:** lifted verbatim from the Svelte component's `<style>`.

### 4. Psychoville: content migration (one-shot Python script under `tools/`)

A discardable Python script (kept under `Psychoville/tools/` for reproducibility)
transforms the tree in place. Per `.svx` file:

1. Rename `.svx` → `.md` (and `.gen.svx` → `.gen.md`).
2. Strip the leading `<script>…</script>` block (and only that — bodies are
   otherwise plain markdown).
3. Rewrite `RustRef`:
   - `<RustRef ref="X" href={rustdoc('P')} />` → `:rustref[X]{href="/rustdoc/P"}`
   - `<RustRef ref="X" href="/literal" />` → `:rustref[X]{href="/literal"}`
   - `<RustRef ref="X" />` (no href) → `:rustref[X]{}`
   - Handles multiline tags and both `'`/`"` quoting.
4. Rewrite the four transclusions using **each file's own `import` map** (the
   `import Systems from './_systems.gen.svx'` lines name the real partial path):
   `<Systems />` → `:include{src="./_systems.gen.md"}`, and likewise for
   `Deps` / `Symbols` / `Files`.
5. Rewrite hardcoded internal links `](/docs/<path>)` → docgen-rs's URL scheme
   (the handful that exist; scheme confirmed against docgen-rs output during
   planning). Fenced code, wikilinks, and mermaid fences are left untouched.

The `.gen.md` partials need no body edits (already plain markdown with an
`AUTO-GENERATED` comment); the `_` prefix excludes them from pages.

### 5. Verification

- **docgen-rs:** `cargo test` — include + discovery features green.
- **Psychoville:** `cargo run -p docgen -- build ~/work/Psychoville` →
  - build succeeds with 0 errors,
  - no `_*.gen` file produces a standalone page,
  - RustRef links, includes, and mermaid all render,
  - spot-check the `server` crate page (uses all four includes), a
    RustRef-dense page, and a mermaid page; optional `docgen dev` visual pass.

## Out of scope

- `/rustdoc/…` targets are generated externally (not by docgen-rs); migrated
  links are preserved as-is. Deploying rustdoc alongside the site is separate.
- The old Svelte `docgen/` app and the `tools/docs-gen` generator are left
  untouched.
- `Badge` / `SrcEmbed` components (unused) are not ported.
