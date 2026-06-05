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
| P6 | `docgen init` scaffold + custom-component directive system + binary distribution | P3 | GREEN | d8716db |
| P7 | **Design / theme** — real site CSS: layout/grid, typography, sidebar/topbar, dark-mode toggle, responsive. (User-requested follow-up — site was unstyled.) | P0–P6 | GREEN | 5aa0bfa |
| P8 | **Pixel-perfect parity with original Svelte docgen** — match tokens (dark default), Geist fonts, topbar (centered search + diff/full-width/right-rail strip + segmented theme pill + brand mark), 3-section right rail (On this page / Additional info / Referenced by), doc-shell typography, 4-type callouts, wikilink tooltips, class-based syntect code (dark+light), search grouping, mermaid dark-sync. Validated side-by-side vs baseline. | P0–P7 | GREEN | 825fe58 |

Status legend: TODO / IN-PROGRESS / GREEN / BLOCKED.

## P7 — Design/theme (user-requested follow-up, added 2026-06-05 ~18:50 MSK)
**Why:** user observed (correctly, confirmed via screenshots) the built site has essentially ZERO theme CSS —
P0 shipped none by design, P1–P6 only added component-scoped styles (search modal, diff, graph, math). No
global layout/typography/sidebar/topbar/dark-mode. Functionally complete, visually broken.
**Scope:** a real `docgen.css` theme — reset/tokens, page layout (sidebar + content + right rail), type scale,
code/callout/table styling, working light/dark ThemeToggle, responsive/mobile. Reference the original Svelte
`src/lib/styles/` for intended look.
**Validation bar (explicit user requirement):** drive by ACTUAL SCREENSHOTS in Chrome — every page type (home,
doc, history, graph, math, diagram, custom-component, search-open, dark mode) must look polished, not merely
"has CSS". Use frontend-design skill principles; iterate on screenshots.

## P8 — Pixel-perfect parity (user-requested, added 2026-06-05 ~21:00 MSK)
**Why:** user ran the built docgen-rs side-by-side against the original Svelte `docgen` (baseline served from
`~/work/psychoville/docgen/build`, dark theme default) and found "tons of regressions". P7 shipped a *different*
theme (warm-paper LIGHT default, no right rail, two-button theme toggle, no full-width switcher, syntect light-only
code). Target = pixel-perfect replica of the original in BOTH UI and features.
**Baseline:** `~/work/psychoville/docgen/build` served on :8801 (`/docs`). docgen-rs fixture on :8802.
**Ground truth (original source):** `~/work/docgen/packages/docgen/src/lib/{styles,components}` —
tokens.css (DARK is `:root` default; light is `[data-theme=light]`), doc-shell.css, controls.css, btn-strip.css,
diff.css, scrollbar.css; Topbar.svelte, RightRail.svelte, DocTree.svelte, SearchModal.svelte, ThemeToggle.svelte,
WikilinkTooltip.svelte; stores/ui-prefs.ts (localStorage keys: doc-theme, doc-full-width,
doc-right-rail-collapsed, doc-left-rail-width). Built Prism theme: `~/work/psychoville/docgen/build/css/code.css`.
**Parity gaps identified (architect, via screenshots + source):**
1. Tokens: dark must be DEFAULT (`:root` = #0d0c0a…); light = `[data-theme=light]`. docgen-rs has it inverted.
2. Topbar: centered search pill (min(360px,42vw), "Search pages, headings, Rust refs… ⌘K"); diamond brand mark;
   btn-strip of icon controls (full-width/maximize toggle, right-rail/menu toggle); segmented moon/sun theme pill.
3. Right rail (currently empty `<aside>`): 3 sections — "On this page" (h2/h3 TOC, scroll-spy), "Additional info"
   (Path/Layer/Commit/Built grid), "Referenced by" (backlink cards). Move backlinks OUT of content into rail.
4. Full-width switcher + right-rail collapse: localStorage-persisted islands toggling classes on `.docgen-app`.
5. doc-shell typography: 15px/1.6, h1 38px, exact spacing rhythm, link accent underline, table/blockquote styling.
6. Callouts: 4 types (TODO/warn, OPEN QUESTION/info, DISCUSSION/talk, CONTENT TODO) with left-accent gradient.
7. Code blocks: switch syntect to CLASS-based output (`ClassedHTMLGenerator`/`ClassStyle::Spaced`) + ship a
   token-aware `code.css` (dark + light) ported from the original Prism colors → fixes P7's light-only code card.
8. Wikilink tooltips (hover popover), rust-ref chips, broken-link styling.
9. Search: grouped fuzzy results (pages/headings + bonuses) closer to original SearchModal scoring.
**Decomposition (conflict-free, two disjoint file-sets):**
- **Track LOOK** (owns ALL CSS: `crates/docgen-assets/assets/docgen/docgen.css`): tokens→dark-default + original
  values, global resets, doc-shell typography, topbar/sidebar/right-rail/search-modal/callout layout. ~70% of parity.
- **Track STRUCTURE** (owns templates + Rust + island JS; sequential within track to avoid intra-file conflict):
  page.html chrome + right-rail markup + PageContext data (TOC, page meta, move backlinks to rail) →
  island JS (full-width, right-rail-collapse, wikilink tooltip, theme pill, scroll-spy) + emission in
  docgen-assets/src/lib.rs + script tags → search.js grouping + syntect class output + code.css.
  Tracks share a CONTRACT (class-name map, token block, dims, localStorage keys, island names) from the Extract phase.
**Validation bar (explicit):** I (architect) drive Claude-in-Chrome to compare docgen-rs (:8802) vs baseline (:8801)
page-by-page (home, doc, math, diagram, directives, graph, history, search, BOTH themes, full-width toggle) and
iterate with focused fix passes until pixel-close. Workflow gets it structurally close + green; I close the pixel gap.

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
