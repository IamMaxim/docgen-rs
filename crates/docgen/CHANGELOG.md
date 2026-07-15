# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.0](https://github.com/IamMaxim/docgen-rs/compare/v0.4.1...v0.5.0) - 2026-07-15

### Added

- PlantUML build-time diagram rendering ([#16](https://github.com/IamMaxim/docgen-rs/pull/16))

## [0.4.1](https://github.com/IamMaxim/docgen-rs/compare/v0.4.0...v0.4.1) - 2026-07-15

### Other

- build and deploy the docs site to GitHub Pages

## [0.4.0](https://github.com/IamMaxim/docgen-rs/compare/v0.3.1...v0.4.0) - 2026-07-14

### Added

- *(build)* offload attachments to S3 when [s3] configured and creds present

### Fixed

- *(s3)* trim the list prefix so idempotency holds for slash-prefixed config
- *(cli)* expose s3 feature on the docgen-rs binary crate

### Other

- note s3 feature build prerequisites and public-read requirement

## [0.3.0](https://github.com/IamMaxim/docgen-rs/compare/v0.2.0...v0.3.0) - 2026-07-08

### Other

- open-source README + MIT LICENSE; ignore .overnight

## [0.1.1](https://github.com/IamMaxim/docgen-rs/compare/v0.1.0...v0.1.1) - 2026-06-10

### Fixed

- *(diff)* drop git2 default features to remove openssl-sys
