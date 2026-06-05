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

See `docs/superpowers/plans/` for the roadmap.

## Usage

```sh
cargo run -p docgen -- build path/to/project
```

The project must contain a `docs/` directory of `.md` files. Output is written to
`path/to/project/dist/`.
