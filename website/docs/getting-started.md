---
title: Getting started
---

# Getting started

This walks you from nothing to a documentation site running locally, then points
you at [[deployment]] to put it on the web. It takes about five minutes.

## 1. Install docgen

```sh
cargo install docgen-rs
```

The crate is named `docgen-rs`; the binary it installs is `docgen`. If you'd
rather not compile it, grab a [prebuilt binary](https://github.com/IamMaxim/docgen-rs/releases)
or use `cargo binstall docgen-rs`.

## 2. Scaffold a project

```sh
docgen init my-docs
cd my-docs
```

`docgen init` creates a small, buildable site:

```
my-docs/
├── docgen.toml            # site configuration (see [[configuration]])
├── docs/                  # your Markdown lives here
│   ├── index.md           # the home page
│   └── guide.md           # a sample page with wikilinks, math, and mermaid
└── components/            # custom components (see [[components]])
    └── note/
        ├── template.html
        └── style.css
```

A docgen "project" is simply **any directory that contains a `docs/` folder of
`.md` files.** Everything else is optional.

## 3. Write and preview live

Start the dev server:

```sh
docgen dev            # serves the current directory
# or: docgen dev my-docs
```

Open <http://localhost:4321>. The server watches your files and live-reloads the
browser as you edit. It also has an in-browser editor for quick tweaks. The dev
server binds to loopback only — it is for local authoring, not hosting.

Useful flags:

```sh
docgen dev --port 8080   # bind a different port
docgen dev --open        # open your browser automatically
```

## 4. Add some content

Create `docs/architecture.md`:

```markdown
---
title: Architecture
---

# Architecture

This links back to the [[guide]] and forward to [[index|home]].

Inline math renders at build time: $a^2 + b^2 = c^2$.
```

Save it. The sidebar picks up the new page automatically, the wikilinks resolve,
and the [[search-and-graph|graph and search index]] update — no configuration
needed. See [[wikilinks]] for how linking works and [[features/index|Features]]
for everything else you can drop into a page.

## 5. Build the static site

```sh
docgen build            # writes ./dist
# or: docgen build my-docs   # writes my-docs/dist
```

`dist/` is a self-contained static site: plain HTML, CSS, and a little vendored
JavaScript. There is no runtime and no server requirement — copy it to any
static host.

## 6. Deploy

Head to [[deployment]] for ready-to-use recipes: GitHub Pages (with a workflow
you can copy verbatim), GitLab Pages with zero configuration, and custom
domains.

## Command reference

| Command | What it does |
|---|---|
| `docgen init [DIR] [--force]` | Scaffold a new site into `DIR` (default: current dir). |
| `docgen dev [DIR] [--port N] [--open]` | Live-reload server on `localhost` (default port 4321). |
| `docgen build [DIR]` | Build the static site into `DIR/dist`. |

Run `docgen --help` or `docgen <command> --help` for the authoritative list.
