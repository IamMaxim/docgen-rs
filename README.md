# docgen-rs

A Cargo-only static documentation-site generator. No npm, no Node.

## Status

P0 (core SSG): markdown discovery, frontmatter, rendering, sidebar tree, static
`dist/` output via `docgen build`.

P1 (parity slice): server-side syntax highlighting of fenced code (comrak +
syntect, zero runtime JS), `[[wikilinks]]`/`[[target|label]]` resolution with a
backlinks section and broken-link marking, and a static search index
(`dist/search-index.json`) served by a vendored, dependency-free Cmd/Ctrl-K
search modal (`dist/search.js` + `dist/docgen.css`).

P2 (git diff timeline): every doc tracked in git gets a static
`/<slug>/history/index.html` page showing its commit timeline with line-level
and block-level diffs, plus a "History" link on the doc page. History is read
with `git2` (port of the original SvelteKit doc-diff timeline) and requires no
runtime JS. Scope and limitations:

- **Build-history mode only.** Per-doc commit history is rendered; the
  `dev-worktree` mode (uncommitted/untracked changes) is deferred to P5.
- **Graceful no-op.** Building a project that is not inside a git repo, or a doc
  with no commit history, simply skips the history page (and omits its link) —
  the build never fails on this.
- **Rename following** emulates `git log --follow` via first-parent rename
  chains (no copy detection), matching the original's effective behavior.
- **Depth.** Each doc walks up to 50 commits by default; override with the
  `DOC_DIFF_LIMIT` env var (clamped to 1..=200).

See `docs/superpowers/plans/` for the roadmap.

## Install

### Prebuilt binary (recommended — no toolchain)

With [cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

    cargo binstall docgen

Or download an archive for your platform from the
[Releases](https://github.com/iammaxim/docgen-rs/releases) page and put `docgen`
on your `PATH`.

### From source

    cargo install --path crates/docgen

## Quick start

    docgen init my-docs
    cd my-docs
    docgen dev          # http://localhost:4321 with live reload
    docgen build        # static site in ./dist

`docgen init` scaffolds a buildable site: a `docgen.toml`, a `docs/` tree with
sample content exercising wikilinks, math and mermaid, a sample custom component
under `components/`, and a `.gitignore`.

## Usage

```sh
cargo run -p docgen -- build path/to/project
```

The project must contain a `docs/` directory of `.md` files. Output is written to
`path/to/project/dist/`.

## Includes & partials

`:include{src="./_part.md"}` transcludes another markdown file (resolved relative
to the including doc) and renders it through the full pipeline. Any `.md` file
whose basename starts with `_` is an *include-only partial*: it is excluded from
page discovery (no standalone page, sidebar entry, or search result) but remains
a valid `:include` target. Missing targets and include cycles degrade to an
inert error span; the build never fails on a bad include.
