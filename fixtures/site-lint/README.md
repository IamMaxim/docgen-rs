# site-lint

A deliberately-unhealthy fixture for `docgen lint` integration tests. Pure
rules only — no network/external checks. Expected findings:

| File | Rule hits |
| --- | --- |
| `docs/index.md` | (clean; links every page, includes `_used.md`, references `attachments/ok.png`) |
| `docs/broken-links.md` | `broken-wikilink` (error), `broken-anchor` (warn) |
| `docs/assets.md` | `missing-asset` (error), `unknown-component` (error) |
| `docs/meta.md` | `invalid-frontmatter` (error) |
| `docs/headings.md` | `heading-level-jump` (re-leveled info -> warn via docgen.toml) |
| `docs/empty.md` | `empty-page` (warn), `missing-title` (warn) |
| `docs/untitled.md` | `missing-title` (warn) |
| `docs/diagrams.md` | `mermaid-unknown-type` (warn), `mermaid-empty` (warn), `plantuml-src-missing` (error) |
| `docs/books.md` + `docs/books.base` | `duplicate-slug` (error, reported on `books.base`) |
| `docs/orphan.md` | `orphan-page` (info) |
| `docs/_unused.md` | `unused-partial` (info) |
| `docs/_used.md` | (clean; included from index) |
| `docs/drafts/broken.md` | none — excluded by the `drafts/**` ignore glob despite its broken wikilink |

Totals: 6 errors, 7 warnings, 2 infos across 10 checked files.
