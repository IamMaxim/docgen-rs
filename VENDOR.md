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
| codemirror/codemirror.js | codemirror | 5.65.16 | https://cdn.jsdelivr.net/npm/codemirror@5.65.16/lib/codemirror.js | MIT |
| codemirror/codemirror.css | codemirror | 5.65.16 | https://cdn.jsdelivr.net/npm/codemirror@5.65.16/lib/codemirror.css | MIT |
| codemirror/markdown.js | codemirror | 5.65.16 | https://cdn.jsdelivr.net/npm/codemirror@5.65.16/mode/markdown/markdown.js | MIT |
| codemirror/xml.js | codemirror | 5.65.16 | https://cdn.jsdelivr.net/npm/codemirror@5.65.16/mode/xml/xml.js | MIT |
| codemirror/overlay.js | codemirror | 5.65.16 | https://cdn.jsdelivr.net/npm/codemirror@5.65.16/addon/mode/overlay.js | MIT |

## Authored (first-party) files — `crates/docgen-assets/assets/docgen/`

- `docgen.css`, `search.js` — migrated from `docgen-render` in P3.
- `bootstrap.js` — Alpine bootstrap + `docgen.island` registry.
- `islands/mermaid.js` — first Alpine island; lazy-loads `mermaid.min.js` (Cluster C).
- `islands/graph.js` — doc-graph SVG island (P4).
- `dev/livereload.js`, `dev/editor.js`, `dev/editor.css` — **dev-only** (see below).

## Dev-only assets — CodeMirror 5 + editor + live-reload (`docgen dev` ONLY)

The vendored CodeMirror 5 files plus the editor island (`dev/editor.js` + `dev/editor.css`)
and the live-reload client (`dev/livereload.js`) are emitted **exclusively** by `docgen dev`,
**never** by `docgen build`. Three independent gates enforce this:

1. They live in `docgen_assets::dev_assets()`, which `assets_for(&EmitOptions)` — the function
   `docgen build` uses to plan its emit set — **never** returns (locked by
   `assets_for_never_includes_dev_assets`, an all-flags matrix test).
2. Dev HTML (`<script>`/`<link>` tags for the editor + reload client) is injected only at
   *serve* time by `docgen_server::inject_dev_html`; the production renderer + `page.html`
   are untouched, so a static dist carries zero editor/reload markup
   (`static_build_has_no_editor_or_reload`).
3. The write/SSE/editor-asset routes exist only on the dev server's axum `Router`; a static
   dist is just files on disk with no server.

**Why CodeMirror 5, not 6:** CM6 ships as ESM with *bare* module specifiers
(`@codemirror/state`, …) that REQUIRE a bundler to assemble — incompatible with the
cargo-only / no-bundler constraint. CM5 (`5.65.16`) ships a single self-contained UMD
`lib/codemirror.js` (+ css + standalone `markdown`/`xml` modes + `overlay` addon) loadable as
plain `<script>`/`<link>` with **zero** import resolution. The `codemirror_is_umd_not_esm` test
locks this (no bare `import` lines). The markdown mode depends on the `xml` mode and the
`overlay` addon, so all five files are vendored and loaded in dependency order.

## KaTeX strategy (flagged decision)

Math is rendered to HTML **at build time** by the `katex` Rust crate (default `quick-js`
backend), verified to compile + render on this machine (~8s). `katex.min.js` and
`auto-render.min.js` are vendored as a **fallback only** — emitted solely when
`EmitOptions.include_katex_runtime` is set (if a future host cannot build `libquickjs-sys`).
The default build ships **zero runtime JS for math**; it does ship the KaTeX **css + fonts**,
which the build-time-rendered HTML requires for display.

`render_math` uses `throw_on_error = true` (not the plan's draft `false`): with `false`, KaTeX
silently emits its own red error markup and returns `Ok`, which would never reach our graceful
fallback. With `true`, a genuine parse failure returns `Err`, letting us substitute a clean
escaped `<code class="docgen-math-error">…</code>` (the behaviour the B-2 test asserts).

## Search trigger (non-migration note)

The P1 search modal (`search.js`) stays a standalone `defer`-loaded classic script, not an Alpine
island — kept as-is per the low-risk rule. It coexists with the island bootstrap without ordering
conflicts.
