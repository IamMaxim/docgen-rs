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
- `islands/mermaid.js` — first Alpine island; lazy-loads `mermaid.min.js` (Cluster C).

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
