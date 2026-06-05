# docgen-rs

A Cargo-only static documentation-site generator. No npm, no Node.

## Status

P0 (core SSG): markdown discovery, frontmatter, rendering, sidebar tree, static
`dist/` output via `docgen build`. See `docs/superpowers/plans/` for the roadmap.

## Usage

```sh
cargo run -p docgen -- build path/to/project
```

The project must contain a `docs/` directory of `.md` files. Output is written to
`path/to/project/dist/`.
