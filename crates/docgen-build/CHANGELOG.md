# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.8.1...docgen-build-v0.9.0) - 2026-07-20

### Added

- *(build)* skip /diff workspace + assets when features.diff = false
- *(bases)* note bodies in cards + single-column card list; base title
- *(core)* `sidebar: false` frontmatter to hide pages from the nav tree

## [0.7.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.6.0...docgen-build-v0.7.0) - 2026-07-16

### Added

- *(bases)* M3 — ship + gate the interactive island
- *(bases)* M1 — emit interactive payload + config (build-time)

### Fixed

- *(bases)* address the three ultrareview findings
- *(bases)* M9 — address all 13 adversarial-review findings

## [0.6.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.5.0...docgen-build-v0.6.0) - 2026-07-15

### Added

- Obsidian Bases support (docgen-bases crate) ([#18](https://github.com/IamMaxim/docgen-rs/pull/18))

## [0.5.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.4.1...docgen-build-v0.5.0) - 2026-07-15

### Added

- PlantUML build-time diagram rendering ([#16](https://github.com/IamMaxim/docgen-rs/pull/16))

## [0.4.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.3.1...docgen-build-v0.4.0) - 2026-07-14

### Added

- *(build)* offload attachments to S3 when [s3] configured and creds present
- *(core)* add AssetUrlResolver hook to the asset-URL rewrite pass

### Fixed

- *(build)* never offload to S3 in dev mode (only production builds)

### Other

- apply rustfmt and satisfy clippy on the s3 branch

## [0.3.1](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.3.0...docgen-build-v0.3.1) - 2026-07-10

### Fixed

- resolve relative asset and page URLs to base-absolute clean URLs

## [0.3.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.2.0...docgen-build-v0.3.0) - 2026-07-08

### Other

- Merge pull request #3 from IamMaxim/perf/incremental-dev-rebuild

## [0.2.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-build-v0.1.1...docgen-build-v0.2.0) - 2026-07-08

### Fixed

- sub-path deploy — base on graph links + GitLab Pages auto-detect
