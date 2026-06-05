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

## Usage

```sh
cargo run -p docgen -- build path/to/project
```

The project must contain a `docs/` directory of `.md` files. Output is written to
`path/to/project/dist/`.
