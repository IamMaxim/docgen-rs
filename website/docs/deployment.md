---
title: Deployment
---

# Deployment

`docgen build` produces a `dist/` directory of plain static files — HTML, CSS,
and a little vendored JavaScript. There's no runtime and no server requirement,
so you can host it anywhere. This page has copy-ready recipes for the common
targets, plus the details of the `base` path that makes sub-path hosting work.

## The `base` path, in one minute

Where your site lives on a domain determines `base` (see [[configuration]]):

- **Domain root** (`https://docs.example.com/`) → `base = ""` (the default).
- **Sub-path** (`https://user.github.io/my-project/`) → `base = "/my-project"`.

docgen prefixes `base` onto every link and asset URL so a sub-path site resolves
correctly. You can override it at build time with the `DOCGEN_BASE` environment
variable; setting `DOCGEN_BASE=` (empty) forces the root.

## GitHub Pages

The site you're reading is deployed this way. The workflow below builds with
docgen and publishes with GitHub's Pages actions. For a **project page** at
`https://<user>.github.io/<repo>/`, set `base = "/<repo>"` in `docgen.toml`.

```yaml
name: pages
on:
  push:
    branches: [master]
  workflow_dispatch:
permissions:
  contents: read
  pages: write
  id-token: write
concurrency:
  group: pages
  cancel-in-progress: false
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0        # full history for the git-history timeline
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install docgen-rs
      - run: docgen build .     # writes ./dist
      - uses: actions/upload-pages-artifact@v3
        with:
          path: dist
  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
    steps:
      - uses: actions/deploy-pages@v4
```

**One-time setup:** in your repository, go to **Settings → Pages** and set
**Source** to **"GitHub Actions"**. This can't be scripted; do it once and every
push to `master` will deploy.

:::callout{type=note title="fetch-depth: 0"}
The [[history|git-history timeline]] reads commit history, so the checkout needs
full history (`fetch-depth: 0`). Without it the timeline is empty.
:::

## GitLab Pages

GitLab needs **no `base` configuration** — docgen auto-detects the deploy path
from `CI_PAGES_URL` (falling back to `CI_PROJECT_PATH`), which is correct for
both the subdomain layout (`namespace.gitlab.io/project`) and the sub-path layout
(`host/group/project`).

```yaml
pages:
  stage: deploy
  image: rust:latest
  script:
    - cargo install docgen-rs
    - docgen build .
    - mv dist public          # GitLab Pages serves the `public/` directory
  artifacts:
    paths: [public]
  rules:
    - if: $CI_COMMIT_BRANCH == $CI_DEFAULT_BRANCH
```

To offload large assets to object storage in the same job, see the GitLab
example on [[s3-offload]].

## Custom domains

Serving from a custom domain at the root? Set `base = ""` (or leave it unset).
On GitHub Pages, add your domain under **Settings → Pages** (which commits a
`CNAME` file) and point your DNS at GitHub. In CI you can force the root
regardless of repo name with `DOCGEN_BASE=`:

```sh
DOCGEN_BASE= docgen build .
```

## Any other static host

`dist/` is just files. Netlify, Cloudflare Pages, S3 website hosting, nginx, a
USB stick — anything that serves static files works. Build locally or in CI:

```sh
docgen build
# then upload ./dist to your host
```

For root-hosted targets keep `base = ""`; for sub-path hosting set `base`
accordingly.

## Content-Security-Policy

The emitted site contains no inline scripts — every `<script>` tag either
loads a same-origin file or is a non-executable JSON data block — so
`script-src` needs neither `'unsafe-inline'` nor hash allowlisting. The theme
is applied before first paint by `/theme-preflight.js`, loaded as a blocking
script in `<head>`, and the deploy base is carried on
`<html data-docgen-base="...">`.

Two qualifiers, verified against a live strict-CSP deploy:

- `script-src` still needs `'unsafe-eval'`: the vendored Alpine.js evaluates
  the `x-data`/`@click` expressions in the markup at runtime. Without it the
  page renders and the content is fully readable, but Alpine-driven controls
  (theme toggle, sidebar/rail toggles, graph) go inert.
- Build-time-rendered math (KaTeX) emits inline `style=` attributes, so pages
  containing math need `style-src 'self' 'unsafe-inline'`.

A known-good policy for the full feature set:

```
default-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data:
```
