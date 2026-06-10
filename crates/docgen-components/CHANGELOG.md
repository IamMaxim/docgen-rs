# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-components-v0.1.0) - 2026-06-10

### Added

- *(components)* Registry + project discovery + builtin-override-by-name
- *(components)* Component type + minijinja directive render (escaped attrs/label, raw content)

### Fixed

- *(directives)* make pre-pass code-aware + quote-aware leaf attrs

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
