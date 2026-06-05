# docgen-rs P1–P6 — Overnight Architect Plan

**Branch:** `overnight/p1-p6` (off P0 `290e5c4`, local only, no push)
**Started:** 2026-06-05 14:28 MSK
**Vision:** Drive the Cargo-only static doc-site generator from the P0 core SSG to a full-parity
product matching the approved spec — search, syntax highlight, wikilinks/backlinks, git diff timeline,
math, mermaid, JS islands (Alpine, vendored, no npm), graph view, dev server + editor, scaffolding,
custom-component directive system, and binary distribution.

**Spec:** `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`
**P0 plan (pattern template):** `docs/superpowers/plans/2026-06-05-docgen-rs-p0-core-ssg.md`

**Method (per phase):** I (main-loop architect) launch one `Workflow` per milestone that:
plan/design → sequential TDD build → gate (cargo test + clippy) → parallel adversarial review →
single fix pass → final verify. I review results skeptically between phases, validate interactive
phases live in Chrome, commit at each green milestone, and keep these files current.

**Hard rules:** green gate before a milestone counts done; no faked tests/results; local commits only,
no push/PR/remote; quarantine irreversible decisions behind a seam and flag them in REPORT.md.

---

## Milestones (ordered by dependency)

| # | Milestone | Depends on | Status | Commit |
|---|-----------|-----------|--------|--------|
| P1 | Search (JSON index + client search island) + `syntect` highlight + wikilinks→links + backlinks | P0 | GREEN | 9b02dd5 |
| P2 | Git diff timeline (`git2` + port of diff algorithms) | P0 | GREEN | f779ec8 |
| P3 | Build-time KaTeX + Mermaid + **`docgen-assets`** crate (Alpine + island embedding infra) | P1 | GREEN | 43248f3 |
| P4 | Graph view (Rust precomputes nodes/edges; JS renders SVG/canvas island) | P1, P3 | GREEN | e4414f6 |
| P5 | Dev server (`axum` + `notify` + live reload) + CodeMirror editor island | P3 | GREEN | 48b2701 |
| P6 | `docgen init` scaffold + custom-component directive system + binary distribution | P3 | TODO | — |

Status legend: TODO / IN-PROGRESS / GREEN / BLOCKED.

## Notes / open seams
- **Island infrastructure lands in P3** (the `docgen-assets` crate + Alpine bootstrap + glue-JS emission).
  P1's search island is the first interactive JS; to avoid a throwaway, P1 ships a minimal self-contained
  search script and P3 generalizes the island/embedding machinery. Flag if this causes rework.
- **Known P0 carry-over:** no page at site root `/` (home is `/index/`). Fix opportunistically in P1's
  routing touch-ups if cheap; otherwise track to P6.
- Interactive phases to validate live in Chrome: P3 (mermaid/katex render), P4 (graph), P5 (editor + live reload).

## Current position
P5 GREEN (48b2701), validated live (docgen dev: SSE live-reload auto-reloaded on disk edit; CodeMirror in-browser editor opened with md highlighting + Save). New crates docgen-build (reusable build_site) + docgen-server (axum dev server). Next: launch P6 (init + custom-component directives + distribution) — the FINAL phase.

**P1 API surface now in place** (later phases depend on these):
- `docgen-core`: `pipeline::{prepare, render_docs, PreparedDoc, SiteBuild{docs,graph,search}}` (two-pass render), `graph::{LinkGraph{edges,backlinks}, build_link_graph}`, `model::{LinkEdge, Backlink, SearchEntry}`, `wikilink::{parse_wikilink, resolve_target, transform_wikilinks}`, `markdown::{comrak_options, format_ast, syntect_adapter (OnceLock), SYNTECT_THEME}`, `search::{plaintext(&AstNode), index_json}`.
- `docgen-render`: `PageContext{..., backlinks: &[Backlink]}`; template refs `/docgen.css`, `/search.js`, `.docgen-search-trigger`; consts `SEARCH_JS`, `DOCGEN_CSS`.
- `docgen` build.rs emits `dist/{search-index.json, search.js, docgen.css}` + clean-URL pages.
- **The link graph (`graph::LinkGraph.edges`) is already built — P4 graph view consumes it directly.**
- **Asset-emission pattern** (include_str! const in a crate, written to dist by build.rs) is the seed P3 generalizes into `docgen-assets`.
