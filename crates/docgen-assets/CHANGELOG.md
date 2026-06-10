# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-assets-v0.1.0) - 2026-06-10

### Added

- *(sidebar)* persist folder collapse state to localStorage
- *(render)* page title/description header + folder notes in sidebar
- *(editor)* 8-theme switcher + settings menu; fix diff narrow-width reflow
- *(polish)* ctrl+c shutdown, full-bleed diff, graph hover/sizing, editor topbar, home dashboard
- *(preview)* render editor live-preview through the real page pipeline
- *(editor)* full CodeMirror 6 split editor at /edit/<slug>, replacing the CM5 overlay
- *(diff)* replace per-page history dump with the original's global /diff workspace
- *(theme)* dev edit icon in strip, single rail separator, graph on home
- *(theme)* P8 residuals — resizable sidebar + mermaid edge-label parity
- *(theme)* P8-A pixel-parity re-skin vs original docgen
- *(assets)* emit concatenated component css/js bundle (authored bytes)
- *(assets)* embed built-in callout component (template+style), dogfooding the component mechanism
- *(assets)* docgenGraph SVG island + graph_assets gate + graph styles (P4 B-1..B-3)

### Fixed

- *(dev)* styled 404 page + stop SSE livereload exhausting the connection pool
- *(theme)* nail rails to viewport edges, stop home overflow, fix tooltip + drop header dots
- *(theme)* wikilink tooltip never hid — CSS keyed on [hidden] attr but JS toggled .is-visible class
- *(theme)* P8-B residual parity fixes
- *(base)* prefix mermaid island lazy-load with deployed base
- *(base)* make sub-path deployment actually work
- *(assets)* gate search.js emission on [features] search

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- P7 review: a11y + design-completeness fixes
- component styling, callouts, diff/graph/math/mermaid, search modal, responsive drawer, dev editor
- app shell, design tokens, light/dark themes, theme-toggle island + no-flash
- P5 fix: stop editor panel flash + suppress duplicate save rebuild
- drop stale codemirror mention from bootstrap.js comment so static dist is leak-free
- *(B)* vendor CodeMirror 5 (UMD) as dev-only assets, gated out of static emit
- *(A)* dev-only live-reload client script + dev_assets() gated out of static emit
- P4 review fixes: cross-platform-determinism seeds, undirected edge dedup, golden test, idiom cleanups
- *(review)* wire same() helper into asset call sites; e2e broken-math test
- *(assets)* mermaid container styles
- *(assets)* mermaid_assets() slice (mermaid.min.js + island), gated by planner
- *(assets)* mermaid Alpine island — lazy-loads vendored mermaid.min.js
- *(assets)* vendored runtime-KaTeX fallback slice (off by default)
- *(assets)* KaTeX display spacing + math-error styles
- *(assets)* katex_css_assets() — stylesheet + 16 woff2 fonts
- *(assets)* typed Asset/AssetKind API, core_assets(), bootstrap contract, emit()/assets_for() planner with stubbed katex/mermaid slices
- *(assets)* migrate search.js + docgen.css into docgen-assets, add bootstrap.js
- *(assets)* docgen-assets crate skeleton with include_dir-embedded vendor tree
- *(assets)* vendor alpine 3.14.1, katex 0.16.11 css+fonts+js, mermaid 11.2.1
