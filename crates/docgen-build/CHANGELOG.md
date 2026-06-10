# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-build-v0.1.0) - 2026-06-10

### Added

- *(core)* :include directive transcludes _partials, excluded from pages
- *(render)* page title/description header + folder notes in sidebar
- *(polish)* ctrl+c shutdown, full-bleed diff, graph hover/sizing, editor topbar, home dashboard
- *(preview)* render editor live-preview through the real page pipeline
- *(diff)* replace per-page history dump with the original's global /diff workspace
- *(theme)* dev edit icon in strip, single rail separator, graph on home
- *(theme)* P8-A pixel-parity re-skin vs original docgen
- *(build)* registry wiring, component bundle emit, per-page island gating + template links
- *(assets)* emit concatenated component css/js bundle (authored bytes)
- *(core)* render directives in render_docs (registry param, per-page used set, recursive inner)
- *(build)* load docgen.toml and gate graph/search + title/base from config
- *(render)* config-driven site title suffix + optional <base> tag
- *(core)* thread SiteConfig into render_docs; math/mermaid feature gates

### Fixed

- *(dev)* styled 404 page + stop SSE livereload exhausting the connection pool
- *(theme)* P8-B residual parity fixes
- *(base)* make sub-path deployment actually work
- *(directives)* make pre-pass code-aware + quote-aware leaf attrs
- *(assets)* gate search.js emission on [features] search
- *(build)* emit the home doc at dist/index.html so the site has a real / page

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- *(overnight)* mark P6 green — config, root-/, directives, init, distribution
- P5 fix: make build_site atomic (stage + swap) so a failed rebuild preserves the last good dist
- *(A)* reusable build_site (docgen-build) + localhost dev server (docgen-server)
