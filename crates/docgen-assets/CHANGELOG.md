# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.8.1...docgen-assets-v0.9.0) - 2026-07-20

### Added

- *(bases)* note bodies in cards + single-column card list; base title

### Fixed

- *(bases)* baseline-align the card meta row

## [0.7.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.6.0...docgen-assets-v0.7.0) - 2026-07-16

### Added

- *(bases)* sort version columns as versions, not as text
- *(bases)* M6 — style the interactive control bar
- *(bases)* M4 — full interactive island (filter/sort/search/paginate)
- *(bases)* M3 — ship + gate the interactive island

### Fixed

- *(bases)* [**breaking**] `limit` caps rows again; warn on typo'd docgenInteractive keys
- *(bases)* address the three ultrareview findings
- *(plantuml)* scroll wide diagrams instead of distorting them
- *(bases)* hide interactive controls that set the `hidden` attribute
- *(bases)* M9 — address all 13 adversarial-review findings

### Other

- *(assets)* pass explicit files to node --test, not the directory
- *(bases)* M7 — cross-language sort-parity fixtures (Rust↔JS)

## [0.6.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.5.0...docgen-assets-v0.6.0) - 2026-07-15

### Added

- Obsidian Bases support (docgen-bases crate) ([#18](https://github.com/IamMaxim/docgen-rs/pull/18))

## [0.5.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.4.1...docgen-assets-v0.5.0) - 2026-07-15

### Added

- PlantUML build-time diagram rendering ([#16](https://github.com/IamMaxim/docgen-rs/pull/16))

## [0.4.1](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.4.0...docgen-assets-v0.4.1) - 2026-07-15

### Added

- responsive mobile layout — drawers, overflow menu, scrollable tables

### Fixed

- keep overflow-menu items at intrinsic width ([#15](https://github.com/IamMaxim/docgen-rs/pull/15))
- mobile right-rail height, overflow menu, and drawer/topbar overlap

## [0.3.1](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.3.0...docgen-assets-v0.3.1) - 2026-07-10

### Fixed

- preserve sidebar scroll position across page navigation

## [0.2.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.1.1...docgen-assets-v0.2.0) - 2026-07-08

### Fixed

- sub-path deploy — base on graph links + GitLab Pages auto-detect

## [0.1.1](https://github.com/IamMaxim/docgen-rs/compare/docgen-assets-v0.1.0...docgen-assets-v0.1.1) - 2026-06-10

### Fixed

- *(graph)* stop the doc graph rendering twice (duplicate on zoom)
