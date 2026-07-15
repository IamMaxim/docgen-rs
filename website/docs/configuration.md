---
title: Configuration
---

# Configuration

docgen reads an optional `docgen.toml` from your project root. **It is entirely
optional** — with no config file, docgen builds with sensible defaults (a site
served at the root, every feature enabled). Add `docgen.toml` only when you want
to change something.

## A complete example

```toml
# Optional site title. When set, page titles become "Page — My Docs"
# (the home page uses just the title). Omit for unchanged per-page titles.
title = "My Docs"

# Base path for a sub-path deployment (see below). Empty = served at the root.
base = ""

[features]
graph = true     # emit the /graph/ knowledge-graph page
math = true      # render LaTeX math at build time
mermaid = true   # render mermaid diagrams
search = true    # emit the search index + Ctrl/Cmd-K modal

[components]
dir = "components"   # directory holding your custom components
```

Every key above shows its default value, so this example is equivalent to having
no `docgen.toml` at all.

## `title`

An optional site title. When set, each page's `<title>` becomes
`"{page} — {title}"`, and the home page uses just `{title}`. When omitted,
per-page titles are left unchanged.

## `base` — deploying under a sub-path

`base` is the URL path your site is served from. Leave it empty (`""`) when the
site lives at a domain root (`https://docs.example.com/`). Set it when the site
lives under a sub-path — most commonly a GitHub **project** page:

```toml
base = "/my-project"   # for https://user.github.io/my-project/
```

docgen prefixes `base` onto every emitted asset URL, navigation link, and
wikilink, so a sub-path deployment resolves correctly. (It does this by writing
root-absolute URLs, not by relying on an HTML `<base>` tag.)

You can override `base` at build time without editing the file:

- **`DOCGEN_BASE`** — an explicit override. Setting it to empty forces the root,
  a handy escape hatch for a custom-domain deploy in CI.
- On **GitLab CI**, docgen auto-detects the base from `CI_PAGES_URL` (falling
  back to `CI_PROJECT_PATH`), so GitLab Pages needs no `base` at all.

The full precedence and the GitLab story live in [[deployment]].

## `[features]`

Four toggles, all `true` by default. Turn one off to drop its output and its
client-side JavaScript entirely — useful for a leaner build when you don't need
a feature.

| Key | Default | Effect when `true` |
|---|---|---|
| `graph` | `true` | Emits the `/graph/` page and its interactive island. |
| `math` | `true` | Renders LaTeX math at build time and links the math stylesheet. |
| `mermaid` | `true` | Enables mermaid diagram blocks (lazy-loaded island). |
| `search` | `true` | Emits `search-index.json` and the search modal. |

A partial `[features]` table keeps the unspecified toggles at their default:

```toml
[features]
search = false   # graph, math, mermaid all stay true
```

See [[search-and-graph]] and [[math-and-mermaid]] for what these produce.

## `[components]`

```toml
[components]
dir = "components"
```

`dir` is the project-relative directory docgen scans for custom components. Each
component is a `<name>/template.html` (optionally with a `style.css`). See
[[components]] for how to write and use them.

## `[s3]` — optional asset offload

An optional `[s3]` section offloads large attachments to an S3-compatible bucket
instead of copying them into `dist/`. It only activates in a binary built with
the `s3` feature, and credentials come from the environment, never this file.
The full reference is on [[s3-offload]].
