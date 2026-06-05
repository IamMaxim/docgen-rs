# Morning Report — docgen-rs P1–P7

**Branch:** `overnight/p1-p6` (local only — NOT pushed, no PR)
**Run started:** 2026-06-05 14:28 MSK · **Last updated:** 2026-06-05 20:05 MSK
**Status:** ✅ COMPLETE — all 7 phases GREEN, tree builds + tests pass, validated live in Chrome.

The Cargo-only static-doc-site generator is done: full feature parity with the original
Svelte/SvelteKit `docgen`, **zero npm / Node / bundler**, and now a real theme. P0 (core SSG)
existed before this run; P1–P7 were built tonight, milestone by milestone.

---

## Shipped (verified, milestone by milestone)
Each milestone was gated on `cargo test` + `cargo clippy --all-targets` green AND, for visual
features, validated **live in Chrome by me** (not trusted from subagent reports).

- **P0 — Core SSG** (`master`, `290e5c4`; baseline). 3-crate workspace (core/render/cli),
  markdown→HTML, frontmatter, sidebar tree, clean-URL static `dist/`. (Prior session.)
- **P1 — Search + highlight + wikilinks/backlinks** (`9b02dd5`). syntect highlighting,
  `[[wikilink]]` resolution + broken-link marking, per-page backlinks, JSON search index + vendored
  ⌘K modal. Two-pass render pipeline + link graph landed here. Live-verified.
- **P2 — Git diff timeline** (`f779ec8`). New `docgen-diff` crate porting the original TS diff
  logic (git2 history, line/block diff, timeline buckets, file-tree) with JSON parity + hermetic
  temp-git-repo tests. Per-doc `/<slug>/history/` pages. Live-verified.
- **P3 — Islands infra + KaTeX + Mermaid** (`43248f3`). New `docgen-assets` crate owns vendored
  Alpine 3.14.1 / KaTeX 0.16.11 / Mermaid 11.2.1 (include_dir) + island registry
  (`window.docgen.island/loadScript`). Build-time KaTeX (zero runtime JS); Mermaid lazy island.
  Live-verified.
- **P4 — Graph view** (`e4414f6`). `graphlayout.rs` deterministic spring layout (port of graph.ts),
  consumes the existing `LinkGraph`; pure-SVG Alpine island (no d3) with hover-highlight, click-nav,
  pan/zoom. Cross-machine determinism pinned with a golden snapshot. Live-verified.
- **P5 — Dev server + editor** (`48b2701`). New `docgen-build` (reusable `build_site`) +
  `docgen-server` (axum 127.0.0.1, notify watcher, SSE live-reload, path-guarded source PUT).
  CodeMirror 5.65.16 editor island (dev-only). Live-verified: disk edit auto-reloaded; in-browser
  editor opened + saved.
- **P6 — init + custom components + distribution** (`d8716db`). New `docgen-config`,
  `docgen-components` (Registry + discover/override), `docgen-init` (scaffold). Source-level
  directive pre-pass (`:::block` + `:leaf`, fence/inline-code-aware); built-in callout dogfoods the
  component system. Root `/` now emitted (fixes P0 carry-over). release.yml + binstall metadata.
  Live-verified.
- **P7 — Design / theme** (`5aa0bfa`; also `4d2ffc8`, `d912952`). 959-line hand-authored
  `docgen.css`: full design-token system, "warm paper" light default + true dark
  (`:root[data-theme=dark]`), sticky topbar (brand + search pill + segmented theme toggle +
  hamburger), 272px sidebar doc-tree with active accent bar, centered 760px content measure,
  right rail, responsive 3-col→drawer @768px. Persisted ThemeToggle (localStorage) + no-flash
  pre-paint script honoring `prefers-color-scheme`. **Validated by actual screenshots** (see below).

**Final gate as left:** `cargo test` → 275 pass / 0 fail; `cargo clippy --all-targets` → clean;
`docgen build fixtures/site-basic` → 6 pages + history + graph + all assets. Tree clean.

## P7 screenshot validation (the explicit user requirement)
Drove Claude-in-Chrome over a static-served `fixtures/site-basic/dist`. Captured and reviewed
**every page type in both themes**:

| Page type | Result |
|-----------|--------|
| Home (dark + light) | ✅ topbar/sidebar/content/backlinks, active-state accent bar |
| Doc / Markup (dark + light) | ✅ prose, valid link accent-underline, broken wikilink muted |
| Math | ✅ KaTeX inline + display + Euler's identity typeset |
| Diagram | ✅ Mermaid SVG flowchart in framed card |
| Directives (custom components) | ✅ warning callout (mono-uppercase title) + nested info callout, inline leaf component, unknown-directive error span |
| Graph | ✅ nodes + edge rendered, themed frame |
| History / diff | ✅ "Today" bucket, commit card (hash chip, +9/−0), green added-line diff |
| Search modal | ✅ ⌘K + click trigger, dimmed backdrop, live results, active-row highlight |
| Theme toggle | ✅ light↔dark persists across navigation, **no flash** |

The starting state the user flagged ("zero CSS, completely broken") is fully resolved.

## Decisions needing your review (most important section)
1. **KaTeX renders at BUILD time** via the `katex` Rust crate (QuickJS/quick-js backend).
   - *Implication:* compiling docgen **from source** needs a C toolchain (QuickJS is C). Prebuilt
     binaries are unaffected — engine embedded, runs at site-build time.
   - *Seam if you disagree:* a fully-vendored runtime-KaTeX fallback is wired behind
     `EmitOptions.include_katex_runtime` (off). Flip to ship `katex.js` + autorender, drop build-time.
2. **`render.unsafe = true` in comrak** (P1) so injected wikilink/directive HTML survives. Safe for
   trusted local docs; titles/sidebar still auto-escaped. Revisit if docgen ever renders untrusted md.
3. **Syntect code card is theme-stable (stays light in dark mode), by design.** syntect's
   InspiredGitHub theme emits inline light-tuned span colors that beat class rules, so the fenced-code
   card uses a fixed light "paper" surface in both themes (documented in the docgen.css header). It
   reads as deliberate, not broken — but if you want dark code blocks in dark mode, the fix is to
   switch syntect to a theme that emits CSS classes (or a dark theme) and drop the stable card. Medium effort.

## Decisions made (FYI)
- Phases driven sequentially, one Workflow each (plan→build→gate→adversarial review→fix→verify);
  shared codebase + ordering deps make parallel phase builds unsafe.
- Vendored JS/CSS/fonts at pinned versions via curl, committed (see `VENDOR.md`); no npm/node/bundler.
- P2 history pages are static HTML (no Alpine); diff interactivity can be enhanced later.
- Light theme is the default; dark is opt-in via toggle but auto-selected when the OS prefers dark.

## Minor polish nits (non-blocking, observed in screenshots)
- **Graph nodes are small/faint dots.** Functional + themed, but larger/labeled nodes would read
  better. ~1 CSS/island tweak in `docgen-assets` graph island.
- **Mermaid edge labels carry light backgrounds** against the dark diagram card (Mermaid default-theme
  artifact). Cosmetic; fixable by configuring Mermaid's `themeVariables`.
- (3) above — code card light in dark mode — is the third, already covered as a reviewable decision.

## Blocked / parked
- _None._ All seven milestones went green honestly.

## State of the tree
- Branch `overnight/p1-p6`, HEAD `5aa0bfa`. Local only — **not pushed, no PR**.
- Builds clean; **275 tests pass, clippy clean** as left. Fixture builds 6 pages + history + graph + assets.
- New crates since baseline: `docgen-diff`, `docgen-assets`, `docgen-build`, `docgen-server`,
  `docgen-config`, `docgen-components`, `docgen-init`. `VENDOR.md` records all vendored assets.

## Recommended next steps
- Your call on the three reviewable decisions above (esp. dark code blocks — most user-visible).
- The two cosmetic nits (graph nodes, Mermaid labels) if you want them tightened.
- Point docgen at a real project's docs (one of your Svelte sites) for a parity smoke test, and
  rewrite one of your Svelte components as a `components/<name>/{template.html,island.js,style.css}`
  to exercise the custom-component path end-to-end.
- When satisfied: squash/curate the overnight bookkeeping commits, decide on `master` merge + push.
