# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-s3-v0.6.0...docgen-s3-v0.7.0) - 2026-07-16

### Fixed

- *(s3)* pin rustls to a single crypto provider

## [0.4.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-s3-v0.3.1...docgen-s3-v0.4.0) - 2026-07-14

### Added

- *(s3)* add list-once uploader with content-hashed idempotent puts
- *(s3)* add docgen-s3 crate with content-hashed asset manifest

### Fixed

- *(s3)* trim the list prefix so idempotency holds for slash-prefixed config

### Other

- apply rustfmt and satisfy clippy on the s3 branch
