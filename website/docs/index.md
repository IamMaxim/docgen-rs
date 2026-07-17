---
title: docgen
---

# docgen

**A Cargo-only static documentation-site generator. No npm, no Node** — just
`cargo install` and a directory of Markdown.

docgen turns a `docs/` tree of `.md` files into a fast, fully static site:
server-side syntax highlighting, `[[wikilinks]]` with backlinks, a search index
with no JavaScript build step, a per-document git-history timeline, and an
interactive knowledge graph — all rendered ahead of time and served as plain
HTML/CSS with a little vendored, dependency-free JavaScript.

> This very site is built by docgen from its own `website/` directory. Every
> feature you read about below is live on the page describing it — the math
> renders, the diagrams draw, and <kbd>Ctrl</kbd>/<kbd>⌘</kbd>+<kbd>K</kbd>
> search works.

## Why docgen?

- **One tool, one binary.** No `node_modules`, no lockfile churn, no build
  pipeline. If you have Rust, you have a docs site.
- **Everything is static.** Syntax highlighting, math, and search indexes are
  computed at build time. The output is HTML/CSS you can host anywhere.
- **Batteries included.** Wikilinks, backlinks, a knowledge graph, full-text
  search, and a git-history timeline all work out of the box — no plugins.
- **It grows with you.** Start with a folder of Markdown; add a `docgen.toml`
  only when you want to change something.

## Get started in three commands

```sh
cargo install docgen-rs   # installs the `docgen` binary
docgen init my-docs       # scaffold a site
docgen dev my-docs        # live-reload server at http://localhost:4321
```

See [[getting-started]] for the full walkthrough, from empty directory to a
site deployed on the web.

## Features

Everything below works with zero configuration. Follow a link to see it in
action.

- **[[wikilinks]]** — link pages with `[[target]]`, get automatic backlinks and
  broken-link marking.
- **[[search-and-graph|Search & knowledge graph]]** — a
  <kbd>Ctrl</kbd>/<kbd>⌘</kbd>+<kbd>K</kbd> search modal and an interactive graph
  of how your docs connect.
- **[[math-and-mermaid|Math & diagrams]]** — LaTeX math and mermaid diagrams,
  rendered at build time.
- **[[includes|Includes & partials]]** — transclude shared snippets with
  `:include`, hide partials with a `_` prefix.
- **[[components]]** — built-in callouts plus your own HTML/CSS components.
- **[[history|Git-history timeline]]** — every tracked doc gets a page showing
  how it changed, with line- and block-level diffs.
- **[[s3-offload|S3 asset offload]]** — optionally push large attachments to an
  S3-compatible bucket instead of copying them into your site.
- **[[lint|Linting]]** — `docgen lint` catches broken links, missing assets,
  and malformed diagrams before you publish; it advises, never blocks a build.

Browse them all under [[features/index|Features]].

## Install

### Prebuilt binary (no toolchain)

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

## Next steps

- [[getting-started]] — build and deploy your first site.
- [[configuration]] — the `docgen.toml` reference.
- [[deployment]] — publish to GitHub Pages, GitLab Pages, or a custom domain.
