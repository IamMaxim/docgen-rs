# Editor bundle source

`editor-cm6.entry.js` is the ESM source for the dev-only full-page CodeMirror 6
editor (`/edit/<slug>`). It is **not** compiled by Cargo — it is bundled once,
out of band, into the vendored artifact `../docgen/dev/editor-cm6.js`, which is
what the dev server actually serves (and what `docgen-assets` embeds).

This keeps the project bundler-free at build/run time: the committed
`editor-cm6.js` is a self-contained esbuild IIFE; no npm/ESM resolution happens
when it loads. Only regenerating the artifact needs Node + the CodeMirror 6
packages.

## Regenerate the bundle

Requires the CodeMirror 6 packages on disk (they live in the original docgen
project's `node_modules`). From that project root:

```sh
cp /path/to/docgen-rs/crates/docgen-assets/assets/editor-src/editor-cm6.entry.js ./_editor_entry.js
node_modules/.bin/esbuild ./_editor_entry.js --bundle --format=iife --minify \
  --outfile=/path/to/docgen-rs/crates/docgen-assets/assets/docgen/dev/editor-cm6.js
rm ./_editor_entry.js
```

Then rebuild the `docgen` binary so the new bytes are embedded.

## What it ports

A faithful port of the original Svelte editor: a 50/50 split (CM6 source | live
rendered preview), `unifiedMergeView` against git HEAD, `[[wikilink]]` syntax
highlighting + autocomplete sourced from `/search-index.json`, a `Mod-Alt-L`
markdown-table formatter, dirty tracking, and `Cmd/Ctrl-S` save. It talks to the
dev endpoints `GET/PUT /__docgen/source` and `POST /__docgen/preview`.
