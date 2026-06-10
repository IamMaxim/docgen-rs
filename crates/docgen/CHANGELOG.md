# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/v0.1.0) - 2026-06-10

### Added

- *(diff)* replace per-page history dump with the original's global /diff workspace
- *(theme)* dev edit icon in strip, single rail separator, graph on home
- *(theme)* P8-A pixel-parity re-skin vs original docgen
- *(cli)* docgen init subcommand + init→build integration smoke (custom component renders)
- *(docgen)* emit /graph/ page + graph island in build; e2e graph emission test (P4 B-6..B-7)
- *(cli)* emit per-doc /history pages from git timeline; skip when no git/history
- *(cli)* emit search-index.json, search.js, docgen.css
- *(cli)* build via two-pass pipeline with per-page backlinks
- *(cli)* docgen build command

### Fixed

- *(p0)* address code-review findings (escaping, frontmatter, discovery, tests)

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- *(include)* fixture + README for :include and _partials
- *(dist)* release workflow + cargo-binstall metadata + README install docs (tooling only)
- *(A)* reusable build_site (docgen-build) + localhost dev server (docgen-server)
- P4 review fixes: cross-platform-determinism seeds, undirected edge dedup, golden test, idiom cleanups
- *(review)* wire same() helper into asset call sites; e2e broken-math test
- *(build)* emit mermaid only when used; thread has_mermaid into pages; diagram fixture + CLI test
- *(build)* end-to-end build-time KaTeX test + math fixture; assert css/fonts emitted, no runtime JS
- *(render)* link KaTeX css in head only on pages with math
- *(build)* emit all assets via docgen-assets planner; deprecate render consts
- *(render)* load Alpine bootstrap in page template, gate mermaid island on has_mermaid
- *(p2)* document git diff timeline + history pages
- P1 review fixes: empty-label wikilinks, shared syntect adapter, single parse, search hardening
- P1 README update (highlight, wikilinks, backlinks, search)
- *(cli)* end-to-end search index + asset emission
- *(cli)* end-to-end wikilinks, backlinks, highlight on fixture
- P0 README and gitignore fixture output
- *(cli)* end-to-end build of fixture site
- scaffold docgen-rs cargo workspace
