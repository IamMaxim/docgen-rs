# docgen-rs

[![crates.io](https://img.shields.io/crates/v/docgen-rs.svg)](https://crates.io/crates/docgen-rs)
[![docs.rs](https://docs.rs/docgen-rs/badge.svg)](https://docs.rs/docgen-rs)
[![CI](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/docgen-rs.svg)](./LICENSE)

A Cargo-only static documentation-site generator. **No npm, no Node** — just
`cargo install` and a directory of Markdown.

docgen-rs turns a `docs/` tree of `.md` files into a fast, fully static site:
server-side syntax highlighting, `[[wikilinks]]` with backlinks, a zero-JS-build
search index, a per-document git history timeline, and a knowledge graph — all
rendered ahead of time, served as plain HTML/CSS with a sprinkle of vendored,
dependency-free JavaScript.

## Features

- **Markdown SSG** — frontmatter, automatic sidebar tree, folder notes, static
  `dist/` output via `docgen build`.
- **Server-side highlighting** — fenced code highlighted at build time with
  comrak + syntect; no runtime JS.
- **Wikilinks & backlinks** — `[[target]]` / `[[target|label]]` resolution with
  a backlinks section and broken-link marking.
- **Static search** — a prebuilt `search-index.json` served by a vendored,
  dependency-free Cmd/Ctrl-K search modal.
- **Git history timeline** — every tracked doc gets a `/<slug>/history/` page
  with line- and block-level diffs across its commit history.
- **Knowledge graph** — an interactive graph of links between documents.
- **Includes & partials** — `:include{src="./_part.md"}` transclusion, with
  `_`-prefixed files treated as include-only partials.
- **Live dev server** — `docgen dev` with live reload.

## Install

### Prebuilt binary (recommended — no toolchain)

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall docgen-rs
```

Or download an archive for your platform from the
[Releases](https://github.com/IamMaxim/docgen-rs/releases) page and put `docgen`
on your `PATH`.

### From crates.io

```sh
cargo install docgen-rs
```

The package is `docgen-rs`; the installed binary is `docgen`.

### From source

```sh
cargo install --path crates/docgen
```

## Quick start

```sh
docgen init my-docs
cd my-docs
docgen dev          # http://localhost:4321 with live reload
docgen build        # static site in ./dist
```

`docgen init` scaffolds a buildable site: a `docgen.toml`, a `docs/` tree with
sample content exercising wikilinks, math and mermaid, a sample custom component
under `components/`, and a `.gitignore`.

## Usage

A project is any directory containing a `docs/` directory of `.md` files.

```sh
docgen build path/to/project    # writes path/to/project/dist/
docgen dev   path/to/project    # serve with live reload
```

### Includes & partials

`:include{src="./_part.md"}` transcludes another markdown file (resolved relative
to the including doc) and renders it through the full pipeline. Any `.md` file
whose basename starts with `_` is an *include-only partial*: it is excluded from
page discovery (no standalone page, sidebar entry, or search result) but remains
a valid `:include` target. Missing targets and include cycles degrade to an
inert error span; the build never fails on a bad include.

### Git history timeline

Every doc tracked in git gets a static `/<slug>/history/index.html` page showing
its commit timeline with line-level and block-level diffs, plus a "History" link
on the doc page. Notes:

- **Build-history mode only** — per-doc commit history is rendered; uncommitted
  changes are not shown.
- **Graceful no-op** — a project that is not a git repo, or a doc with no
  history, simply skips the history page; the build never fails on this.
- **Rename following** emulates `git log --follow` via first-parent rename
  chains (no copy detection).
- **Depth** — each doc walks up to 50 commits by default; override with the
  `DOC_DIFF_LIMIT` env var (clamped to `1..=200`).

## Project layout

This is a Cargo workspace of ten crates. `docgen-rs` is the CLI; the rest are
libraries (`docgen-core`, `docgen-render`, `docgen-build`, `docgen-server`,
`docgen-diff`, `docgen-assets`, `docgen-components`, `docgen-config`,
`docgen-init`).

## Contributing

Issues and pull requests are welcome. Before opening a PR:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets
cargo test --workspace
```

CI runs the same checks on every push and PR.

## License

Licensed under the [MIT License](./LICENSE).
