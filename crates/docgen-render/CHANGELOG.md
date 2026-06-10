# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-render-v0.1.0) - 2026-06-10

### Added

- *(sidebar)* persist folder collapse state to localStorage
- *(render)* page title/description header + folder notes in sidebar
- *(polish)* ctrl+c shutdown, full-bleed diff, graph hover/sizing, editor topbar, home dashboard
- *(preview)* render editor live-preview through the real page pipeline
- *(diff)* replace per-page history dump with the original's global /diff workspace
- *(theme)* dev edit icon in strip, single rail separator, graph on home
- *(theme)* P8 residuals — resizable sidebar + mermaid edge-label parity
- *(theme)* P8-A pixel-parity re-skin vs original docgen
- *(build)* registry wiring, component bundle emit, per-page island gating + template links
- *(build)* load docgen.toml and gate graph/search + title/base from config
- *(render)* config-driven site title suffix + optional <base> tag
- *(render)* /graph/ page template + GraphContext + render_graph + site-wide nav link (P4 B-4..B-5)
- *(cli)* emit per-doc /history pages from git timeline; skip when no git/history
- *(render)* static history timeline page + diff styles
- *(render)* vendored search.js + docgen.css assets
- *(render)* backlinks section + asset/search wiring in page template
- *(render)* minijinja page renderer with sidebar tree

### Fixed

- *(theme)* P8-C move history into topbar diff icon (chrome parity)
- *(theme)* P8-B residual parity fixes
- *(base)* make sub-path deployment actually work
- *(p0)* address code-review findings (escaping, frontmatter, discovery, tests)

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- P7 review: a11y + design-completeness fixes
- app shell, design tokens, light/dark themes, theme-toggle island + no-flash
- *(render)* link KaTeX css in head only on pages with math
- *(build)* emit all assets via docgen-assets planner; deprecate render consts
- *(render)* load Alpine bootstrap in page template, gate mermaid island on has_mermaid
- P1 review fixes: empty-label wikilinks, shared syntect adapter, single parse, search hardening
- P1 README update (highlight, wikilinks, backlinks, search)
- scaffold docgen-rs cargo workspace
