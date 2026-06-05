# Worklog — docgen-rs P1–P6 (append-only)

## 2026-06-05 14:28 MSK — Phase 0: orient & plan
- Starting state: P0 complete on `master` (12 commits, head `290e5c4`), 29 tests green, clippy clean,
  validated live in Chrome. Cargo-only workspace: `docgen-core`, `docgen-render`, `docgen`.
- Created branch `overnight/p1-p6` off `290e5c4`.
- Wrote PLAN.md (6 milestones P1–P6, dependency-ordered), seeded WORKLOG.md + REPORT.md.
- Decision (reversible): drive phases sequentially via one Workflow each (plan→build→gate→review→fix→verify),
  because the codebase is shared and phases have real ordering dependencies; parallel phase builds would
  conflict on shared files (lib.rs, build.rs, render). Rationale recorded; revisit if a phase is fully independent.
- Decision (reversible): P1 ships a minimal self-contained search script; the general island/embedding
  infrastructure (`docgen-assets` + Alpine) is deferred to P3 where it's the headline. Accept minor P1→P3
  rework risk to avoid building island infra before it's needed.
- Next: launch P1 workflow.

## 2026-06-05 15:03 MSK — P1 GREEN
- Workflow wpv8ecm1s: plan → 3 TDD build clusters (highlight / wikilinks+backlinks / search) → gate → 4 reviews → fix → verify. 11 agents.
- Result: 59 tests green (my re-run; verify agent counted 60), clippy clean, fixture builds 3 pages + search-index.json + search.js + docgen.css.
- Adversarial review: 16 findings, 11 applied (4 major: per-doc syntect adapter→OnceLock; double comrak parse→single; empty wikilink label fallback; search.js innerHTML slug-injection→createElement/setAttribute), 6 rejected with sound rationale (intentional behaviors/micro-churn).
- ARCHITECT VERIFICATION (not just trusting subagents): ran cargo test (59 pass / 7 binaries), clippy (clean), built fixture, and validated LIVE in Chrome: highlighted `fn main()`, resolved [[index]] link + broken [[missing-page]] marked span, Backlinks section, and the Cmd/Ctrl-K search modal returning live full-text results. All confirmed working.
- Decision (reversible, FYI): comrak 0.52 needed render.unsafe=true so injected wikilink anchor HTML survives; acceptable since our input is trusted local docs, and titles/sidebar are still auto-escaped by minijinja. Noted as a seam to revisit if docgen ever renders untrusted markdown.
- Two-pass render pipeline landed cleanly; link graph already built (P4 will consume it). Next: P2 git diff timeline.

## 2026-06-05 16:30 MSK — P2 GREEN
- Workflow wp6t76fb8 (~83 min): plan → 3 TDD build clusters → gate → 4 reviews → fix → verify. New crate **docgen-diff** porting the original Svelte TS diff modules (git2 history, line/block diff, timeline grouping, file-tree, payloads, report) with JSON parity + hermetic temp-git-repo tests.
- Result: 104 tests green, clippy clean (-D warnings), build emits per-doc /<slug>/history/ pages; graceful no-op when no git/history.
- Review: 9 findings, ALL 9 applied (2 major: merge commits → spurious duplicate revisions fixed via git-log TREESAME simplification; dead per-block markdown re-render removed from build path), 0 rejected.
- ARCHITECT VERIFICATION: re-ran cargo test (104 pass/10 binaries), clippy clean, built fixture (3 pages + 3 history pages), validated LIVE in Chrome — /markup/history/ shows "Today" bucket, commit 9b02dd5 w/ author+date+(+9/−0), file path, green added-line diff. Confirmed working.
- Decision (reversible, FYI): history pages are STATIC HTML (no Alpine) for P2; diff interactivity can be enhanced once island infra lands in P3. Per-doc history uses /<slug>/history/index.html.
- Next: P3 (build-time KaTeX, Mermaid, docgen-assets crate + Alpine).

## 2026-06-05 17:40 MSK — P3 GREEN
- Workflow wgmzafopb (~65 min): plan → 3 TDD clusters (docgen-assets+Alpine / build-time KaTeX / Mermaid island) → gate → 4 reviews → fix → verify.
- New crate **docgen-assets**: embeds vendored Alpine 3.14.1, KaTeX 0.16.11 (css+16 woff2 fonts), Mermaid 11.2.1 via include_dir; typed Asset API + emit(); VENDOR.md records sources/versions/licenses. Island registry: window.docgen.{island(name,fn), loadScript(src cached)} on alpine:init.
- KaTeX = BUILD-TIME via katex crate 0.4.6 (quick-js/QuickJS backend); zero runtime JS for math; runtime fallback vendored but OFF (EmitOptions.include_katex_runtime). Mermaid = lazy Alpine island, loads mermaid.js only on pages with diagrams.
- Result: 144 tests green, clippy clean (-D warnings), 5 fixture pages. Review 7 findings, 6 applied (display-math fallback layout, asset path same() helper wired, KaTeX error now logged, +3 tests incl. XSS-escape + broken-math E2E), 1 rejected (throw_on_error finding recommended no change).
- ARCHITECT VERIFICATION: re-ran cargo test (144/12 binaries), clippy clean, built fixture, validated LIVE in Chrome — /math/ shows typeset E=mc^2, display sum, Euler's identity; /diagram/ shows a rendered Mermaid SVG flowchart (Start→Choice→yes/no→Do thing/Skip). Both confirmed.
- FLAGGED DECISION (see REPORT): build-time KaTeX needs a C toolchain to COMPILE docgen from source (QuickJS). Prebuilt-binary users (P6) are unaffected (engine embedded); runtime-JS fallback exists behind a seam. Defensible + spec-sanctioned.
- Next: P4 (graph view — consumes existing graph::LinkGraph.edges).
