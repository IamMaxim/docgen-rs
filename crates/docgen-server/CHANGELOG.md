# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-server-v0.1.0) - 2026-06-10

### Added

- *(core)* :include directive transcludes _partials, excluded from pages
- *(polish)* ctrl+c shutdown, full-bleed diff, graph hover/sizing, editor topbar, home dashboard
- *(preview)* render editor live-preview through the real page pipeline
- *(editor)* full CodeMirror 6 split editor at /edit/<slug>, replacing the CM5 overlay
- *(theme)* dev edit icon in strip, single rail separator, graph on home

### Fixed

- *(dev)* styled 404 page + stop SSE livereload exhausting the connection pool

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- P5 fix: stop editor panel flash + suppress duplicate save rebuild
- P5 security: Host/Origin allowlist on dev server to defeat DNS-rebinding
- *(B)* dev-only gating integration tests
- *(B)* editor island + path-guarded get/put source endpoints
- *(A)* reusable build_site (docgen-build) + localhost dev server (docgen-server)
