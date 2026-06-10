# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/IamMaxim/docgen-rs/releases/tag/docgen-core-v0.1.0) - 2026-06-10

### Added

- *(core)* :include directive transcludes _partials, excluded from pages
- *(core)* partial-file helpers + include path resolution
- *(render)* page title/description header + folder notes in sidebar
- *(polish)* ctrl+c shutdown, full-bleed diff, graph hover/sizing, editor topbar, home dashboard
- *(preview)* render editor live-preview through the real page pipeline
- *(theme)* P8-A pixel-parity re-skin vs original docgen
- *(core)* render directives in render_docs (registry param, per-page used set, recursive inner)
- *(core)* directive substitution + recursive inner render + unknown→error span
- *(core)* source-level directive extract pass (block + leaf, nested, escaped)
- *(core)* thread SiteConfig into render_docs; math/mermaid feature gates
- *(core)* SiteBuild::graph_data builds layout from docs + link graph (P4 A-5)
- *(core)* deterministic force-directed graph layout (GraphData + JSON, degree/edge filtering, golden-angle seed, spring forces) (P4 A-1..A-4)
- *(core)* serialize search index to JSON
- *(core)* two-pass pipeline (prepare + render_docs) with links, graph, search
- *(core)* plaintext extraction for search index
- *(core)* link graph + inverted backlinks builder
- *(core)* wikilink resolver, parser, and AST transform pass
- *(core)* format_ast + raw-HTML render for AST pass
- *(core)* add LinkEdge, Backlink, SearchEntry model types
- *(core)* server-side syntect highlight + shared comrak options
- *(core)* discover markdown files under docs root
- *(core)* build sorted sidebar tree from docs
- *(core)* assemble RawDoc into Doc with slug and title
- *(core)* render markdown to html via comrak
- *(core)* parse YAML frontmatter
- *(core)* add Doc, RawDoc, TreeNode types

### Fixed

- *(discover)* prune hidden + vendor dirs (node_modules, target, .git, .obsidian)
- *(theme)* P8-B residual parity fixes
- *(base)* make sub-path deployment actually work
- *(directives)* make pre-pass code-aware + quote-aware leaf attrs
- *(p0)* address code-review findings (escaping, frontmatter, discovery, tests)

### Other

- publish to crates.io via release-plz + bump to 0.1.0
- cargo fmt --all
- P4 review fixes: cross-platform-determinism seeds, undirected edge dedup, golden test, idiom cleanups
- *(review)* wire same() helper into asset call sites; e2e broken-math test
- *(core)* run mermaid pass in render_docs; track Doc.has_mermaid + SiteBuild.any_mermaid
- *(core)* mermaid detection pass — fenced mermaid → Alpine island container
- *(core)* run build-time math pass in render_docs; track Doc.has_math
- *(core)* AST math pass replacing comrak math nodes with build-time KaTeX HTML
- *(core)* build-time KaTeX render_math helper (katex crate); shared util::escape_html
- *(core)* enable comrak math_dollars + math_code in shared options
- P1 review fixes: empty-label wikilinks, shared syntect adapter, single parse, search hardening
- *(core)* enable comrak syntect feature
- scaffold docgen-rs cargo workspace
