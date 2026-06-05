# Morning Report — docgen-rs P1–P6

**Branch:** `overnight/p1-p6` (local only — NOT pushed, no PR)
**Run started:** 2026-06-05 14:28 MSK · **Last updated:** 17:40 MSK
**Status:** IN PROGRESS — P1–P3 shipped & verified; P4 next (P5, P6 to follow)

---

## Shipped (verified, milestone by milestone)
Each was gated on `cargo test` + `cargo clippy --all-targets` green AND, for visual features,
validated live in Chrome by the architect (not just trusted from subagent reports).

- **P0 — Core SSG** (`master`, `290e5c4`; baseline of this branch). 3-crate Cargo workspace
  (docgen-core/render/cli), markdown→HTML, frontmatter, sidebar tree, clean-URL static `dist/`.
  Validated live. (Built in a prior session.)
- **P1 — Search + highlighting + wikilinks/backlinks** (`9b02dd5`). syntect code highlighting,
  `[[wikilinks]]` resolution + broken-link marking, per-page Backlinks, JSON search index +
  vendored ⌘K search modal. **59 tests.** Live-verified: highlighted code, links, backlinks,
  working search modal. Two-pass render pipeline + link graph landed here.
- **P2 — Git diff timeline** (`f779ec8`). New `docgen-diff` crate porting the original Svelte/TS
  diff logic (git2 history, line/block diff, timeline grouping, file-tree) with JSON parity +
  hermetic temp-git-repo tests. Per-doc `/<slug>/history/` pages. **104 tests.** Live-verified:
  History page with timeline buckets, commit metadata, colored diffs.
- **P3 — Islands infra + KaTeX + Mermaid** (`43248f3`). New `docgen-assets` crate owns vendored
  Alpine 3.14.1 / KaTeX 0.16.11 / Mermaid 11.2.1 (embedded via include_dir) + an island registry
  (`window.docgen.island/loadScript`). Build-time KaTeX (zero runtime JS); Mermaid as a lazy
  Alpine island. **144 tests.** Live-verified: typeset inline+display math; rendered Mermaid SVG.

## Decisions needing your review (most important section)
1. **KaTeX is rendered at BUILD time via the `katex` Rust crate (QuickJS/quick-js backend).**
   - *Implication:* compiling docgen **from source** now requires a C toolchain (QuickJS is C).
     Prebuilt-binary users (P6) are unaffected — the engine is embedded and runs at site-build time.
   - *Why:* it's the spec-preferred path (zero runtime JS for math, full fidelity) and built cleanly
     here in ~8s.
   - *Seam if you disagree:* a fully-vendored runtime-KaTeX fallback is already wired behind
     `EmitOptions.include_katex_runtime` (off by default) — flip it to ship `katex.js` + autorender
     and drop the build-time engine. Low effort to switch.
2. **`render.unsafe = true` in comrak** (from P1) so injected wikilink anchor HTML survives. Safe for
   trusted local docs; page titles/sidebar are still auto-escaped. Revisit if docgen ever renders
   untrusted markdown.

## Decisions made (FYI)
- Phases driven sequentially, one Workflow each (plan→build→gate→adversarial review→fix→verify);
  shared codebase + ordering deps make parallel phase builds unsafe.
- Vendored JS/CSS/fonts fetched at pinned versions via curl and committed (see `VENDOR.md`); no npm,
  no node, no bundler — consistent with the Cargo-only goal.
- P2 history pages are static HTML (no Alpine); diff interactivity can be enhanced later.
- Diff `throw_on_error=true` for KaTeX (fail-loud on bad math at build, with graceful escaped fallback).

## Blocked / parked
- _None._ All milestones so far went green honestly.

## State of the tree
- Branch `overnight/p1-p6`, HEAD `e332dac` (overnight bookkeeping commit on top of P3 `43248f3`).
- Builds clean; **144 tests pass, clippy clean** as left. Fixture builds 5 pages + history + all assets.
- New crates since baseline: `docgen-diff`, `docgen-assets`. `VENDOR.md` documents vendored assets.

## Recommended next steps
- **P4** graph view (consumes the already-built `graph::LinkGraph.edges`; plan: Rust force-layout →
  SVG Alpine island, no d3).
- **P5** dev server (axum + notify + live reload) + CodeMirror editor island.
- **P6** `docgen init` scaffold + custom-component directive system + binary distribution; also fold in
  the known P0 carry-over (no page at site root `/`).
- After P6: final full-suite gate, a consolidated CHANGELOG, and a from-scratch `docgen build` smoke test.
