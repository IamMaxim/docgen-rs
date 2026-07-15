# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.6.0](https://github.com/IamMaxim/docgen-rs/compare/docgen-bases-v0.5.0...docgen-bases-v0.6.0) - 2026-07-15

### Added

- Obsidian Bases support (docgen-bases crate) ([#18](https://github.com/IamMaxim/docgen-rs/pull/18))
- *(build)* offload attachments to S3 when [s3] configured and creds present

### Fixed

- *(s3)* trim the list prefix so idempotency holds for slash-prefixed config

### Other

- build and deploy the docs site to GitHub Pages
- note s3 feature build prerequisites and public-read requirement
- open-source README + MIT LICENSE; ignore .overnight
- *(include)* fixture + README for :include and _partials
- *(dist)* release workflow + cargo-binstall metadata + README install docs (tooling only)
- *(p2)* document git diff timeline + history pages
- P1 README update (highlight, wikilinks, backlinks, search)
- P0 README and gitignore fixture output
