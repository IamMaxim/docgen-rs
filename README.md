# docgen-rs

[![crates.io](https://img.shields.io/crates/v/docgen-rs.svg)](https://crates.io/crates/docgen-rs)
[![docs.rs](https://docs.rs/docgen-rs/badge.svg)](https://docs.rs/docgen-rs)
[![CI](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/IamMaxim/docgen-rs/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/docgen-rs.svg)](./LICENSE)

A Cargo-only static documentation-site generator. **No npm, no Node** — just
`cargo install` and a directory of Markdown.

📖 **[Documentation site](https://iammaxim.github.io/docgen-rs/)** — a full
feature guide, built by docgen itself.

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

### S3 asset offload

Large attachments (images, PDFs, etc. referenced from docs) can be offloaded
to an S3-compatible bucket instead of being copied into `dist/`. This keeps
generated sites small and lets CDNs serve binaries directly.

Add an `[s3]` section to `docgen.toml`:

```toml
[s3]
bucket = "my-bucket"
region = "auto"                      # "auto" works for R2/MinIO; use a real
                                      # AWS region (e.g. "us-east-1") for AWS S3
endpoint = "https://<account>.r2.cloudflarestorage.com"  # omit for AWS S3
prefix = "docs-assets"               # optional key prefix within the bucket
public_url = "https://cdn.example.com"
path_style = true                    # required by MinIO and some S3-compatibles
```

`public_url` must actually be reachable by readers: docgen uploads objects to
the bucket but does not set an ACL or otherwise configure bucket permissions.
The bucket policy (or the CDN/custom domain in front of it) must grant public
read access to the uploaded objects, or images and attachments will 403 on a
successfully built site.

Credentials are never stored in `docgen.toml`; they are read from the
environment: `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY`.

**Auto-activation:** offload only activates when *all* of these hold:

- the installed `docgen` binary was built with the `s3` cargo feature
  (`cargo install docgen-rs --features s3` — it is off by default);
- `[s3]` is present in `docgen.toml`;
- both credential env vars are set at build time.

Building with `--features s3` requires a C/C++ compiler and `cmake` on the
build machine (pulled in transitively for TLS); on a minimal CI image, install
`build-essential` and `cmake` (or your distro's equivalents) before
`cargo install`.

If the feature is off, or `[s3]` is absent, or credentials are missing,
`docgen build` falls back to copying attachments into `dist/` as usual and
prints a one-line explanation to stderr — it never fails the build for a
missing/incomplete S3 setup. `docgen dev` never uploads, regardless of
configuration.

**Limitations.** Only asset references written in Markdown syntax
(`![](…)` images and `[](…)` links) are rewritten to bucket URLs. A raw HTML
`<img src>` / `<a href>` embedded in a doc is left untouched — and because
offload mode skips the local copy, such a reference will 404 on the deployed
site. Keep attachment references in Markdown syntax when offload is active.
For non-AWS providers (R2, MinIO, B2, Spaces) you must set `endpoint`; without
it the client targets `https://s3.<region>.amazonaws.com`, so a bare
`region = "auto"` with no `endpoint` will not resolve.

Minimal GitLab CI example:

```yaml
pages:
  stage: deploy
  variables:
    AWS_ACCESS_KEY_ID: $S3_ACCESS_KEY_ID
    AWS_SECRET_ACCESS_KEY: $S3_SECRET_ACCESS_KEY
  script:
    - apt-get update && apt-get install -y build-essential cmake
    - cargo install docgen-rs --features s3
    - docgen build .
  artifacts:
    paths: [dist]
```

(Set `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` as masked CI/CD variables in
your project settings.)

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
