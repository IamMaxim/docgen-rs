<div align="center">

<img src="assets/docgen-logo.svg" alt="" width="88" height="88" />

# docgen

**A Cargo-only static documentation-site generator.**
No npm, no Node — just `cargo install` and a directory of Markdown.

[![crates.io](https://img.shields.io/crates/v/docgen-rs.svg)](https://crates.io/crates/docgen-rs)
[![docs.rs](https://docs.rs/docgen-rs/badge.svg)](https://docs.rs/docgen-rs)
[![CI](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/docgen-rs.svg)](./LICENSE)

**[Documentation](https://iammaxim.github.io/docgen-rs/)** · [Features](https://iammaxim.github.io/docgen-rs/features/) · [Releases](https://iammaxim.github.io/docgen-rs/releases/)

</div>

---

docgen turns a `docs/` tree of `.md` files into a fast, fully static site —
server-side syntax highlighting, `[[wikilinks]]` with backlinks, a zero-JS-build
search index, a per-document git-history timeline, and a knowledge graph — all
rendered ahead of time and served as plain HTML/CSS with a sprinkle of vendored,
dependency-free JavaScript.

The [documentation site](https://iammaxim.github.io/docgen-rs/) is built by
docgen from this repo, so it doubles as a live demo of every feature.

## Quick start

```sh
cargo install docgen-rs      # installs the `docgen` binary
docgen init my-docs          # scaffold a site
cd my-docs
docgen dev                   # http://localhost:4321, live reload
docgen build                 # static site in ./dist
```

`docgen init` scaffolds a buildable site: a `docgen.toml`, a `docs/` tree of
sample content (wikilinks, math, mermaid, a custom component), and a `.gitignore`.
Drop in your own `.md` files and the sidebar, search, links, and graph update on
their own — no configuration needed.

## Features

- **Markdown SSG** — frontmatter, an automatic sidebar tree, folder notes, and
  static `dist/` output.
- **Server-side highlighting** — code highlighted at build time (comrak +
  syntect); no runtime JS.
- **[Wikilinks & backlinks](https://iammaxim.github.io/docgen-rs/features/wikilinks/)**
  — `[[target]]` / `[[target|label]]` with a backlinks section and broken-link marking.
- **[Static search](https://iammaxim.github.io/docgen-rs/features/search-and-graph/)**
  — a prebuilt index served by a dependency-free ⌘K modal, plus a knowledge graph.
- **[Git-history timeline](https://iammaxim.github.io/docgen-rs/features/history/)**
  — every tracked doc gets a `/history/` page with line- and block-level diffs.
- **[Math & Mermaid](https://iammaxim.github.io/docgen-rs/features/math-and-mermaid/)**,
  **[PlantUML](https://iammaxim.github.io/docgen-rs/features/plantuml/)**,
  and **[Obsidian Bases](https://iammaxim.github.io/docgen-rs/features/bases/)**
  — diagrams and `.base` views rendered at build time.
- **[Includes & partials](https://iammaxim.github.io/docgen-rs/features/includes/)**
  and **[custom components](https://iammaxim.github.io/docgen-rs/features/components/)**.
- **[S3 asset offload](https://iammaxim.github.io/docgen-rs/features/s3-offload/)**
  — push large attachments to an S3-compatible bucket instead of `dist/`.
- **[Linting](https://iammaxim.github.io/docgen-rs/features/lint/)** — `docgen lint`
  finds broken links, missing assets, and malformed diagrams before you publish.
- **Live dev server** — `docgen dev` with live reload and an in-browser editor.

See the [feature guide](https://iammaxim.github.io/docgen-rs/features/) for the
full reference.

## Install

The package is `docgen-rs`; the installed binary is `docgen`.

```sh
# Prebuilt binary (no toolchain) via cargo-binstall:
cargo binstall docgen-rs

# Or from crates.io:
cargo install docgen-rs

# Or from source:
cargo install --path crates/docgen
```

Prebuilt archives for each platform are also on the
[Releases](https://github.com/IamMaxim/docgen-rs/releases) page. S3 offload is
opt-in at install time (`cargo install docgen-rs --features s3`); see the
[S3 guide](https://iammaxim.github.io/docgen-rs/features/s3-offload/) for the
build prerequisites.

## How it works

A project is any directory containing a `docs/` tree of `.md` files. `docgen
build path/to/project` writes `path/to/project/dist/`; `docgen dev` serves it
with live reload; `docgen lint` runs advisory pre-publish checks (it never
blocks the build).

Under the hood it's a Cargo workspace of fourteen crates: `docgen-rs` is the CLI,
and the rest are focused libraries (`docgen-core`, `docgen-render`,
`docgen-build`, `docgen-server`, `docgen-diff`, `docgen-assets`,
`docgen-components`, `docgen-config`, `docgen-init`, `docgen-lint`,
`docgen-plantuml`, `docgen-bases`, `docgen-s3`). Browse the API on
[docs.rs](https://docs.rs/docgen-rs).

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
