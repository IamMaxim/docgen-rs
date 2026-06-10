# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-diff-v0.1.0) - 2026-06-10

### Added

- *(editor)* full CodeMirror 6 split editor at /edit/<slug>, replacing the CM5 overlay
- *(diff)* replace per-page history dump with the original's global /diff workspace
- *(diff)* orchestrate build-history DocDiffReport per doc
- *(diff)* port timeline date bucketing
- *(diff)* port payload summarization
- *(diff)* port changed-file tree grouping
- *(diff)* port markdown block segmentation + block diff
- *(diff)* port line-level hunk diff (LCS + context grouping)
- *(diff)* git2 doc history walk (renames, first-commit, no-history)
- *(diff)* port git name-status + untracked parsing
- *(diff)* port git_refs base-ref selection
- *(diff)* scaffold docgen-diff crate + DocDiff types with JSON parity

### Fixed

- *(diff)* git-log history simplification for merge commits + history edge tests

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- *(diff)* add slice-based base_ref_for_parents helper
- *(diff)* stop rendering per-revision block HTML in the build path
