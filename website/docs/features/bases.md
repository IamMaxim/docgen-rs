---
title: Obsidian Bases
---

# Obsidian Bases

**What it is.** docgen understands [Obsidian Bases](https://obsidian.md/help/bases)
— the `.base` file format that turns your notes into filtered, sorted, grouped
**database views**. A `.base` file becomes its own page, and a ` ```base ` fenced
block embeds a view inline in any markdown page. Everything is computed **at build
time** into static HTML — no JavaScript, no runtime, no database.

**Why you'd want it.** Point a view at your notes' frontmatter (`status`,
`rating`, `tags`, `due`, …) and file metadata (`file.folder`, `file.mtime`, …) and
get a live index — a reading list, a task board, a changelog — that stays in sync
with your content and versions in git as plain text.

## The two ways to use it

**A standalone `.base` file.** Drop a `.base` file anywhere under `docs/`. It
appears in the sidebar and gets its own page at a clean URL (a `Bases/Books.base`
file is served at `/Bases/Books`):

```yaml
# Bases/Books.base
filters:
  and:
    - file.hasTag("book")
    - '!file.inFolder("Archive")'
views:
  - type: table
    name: Reading list
    order: [file.name, note.author, note.rating]
    sort:
      - property: note.rating
        direction: DESC
```

**An embedded block.** Put the same YAML in a ` ```base ` fenced code block inside
any markdown page to render the view right there:

````markdown
```base
views:
  - type: cards
    order: [file.name, note.status]
```
````

## Live example

This page embeds a base that lists every page in this `features/` folder — a real
view computed over this very site at build time:

```base
filters:
  and:
    - file.inFolder("features")
    - file.ext == "md"
    - 'file.name != "index.md"'
views:
  - type: table
    name: Feature pages
    order:
      - file.name
      - file.folder
    sort:
      - property: file.name
        direction: ASC
```

## What's supported

docgen implements the Bases format faithfully:

- **Five sections** — `filters`, `formulas`, `properties`, `summaries`, `views`.
- **Filters** — the full `and`/`or`/`not` logical tree over expression strings.
- **Views** — `table`, `cards`, and `list`, each with its own `name`, `filters`,
  `order`, `sort`, `groupBy`, `limit`, `columnSize`, and `summaries`.
- **The expression language** — property references (`note.x`, `file.x`,
  `formula.x`, and a bare `x` for a note property), operators (`+ - * / %`,
  comparisons, `&& || !`, date ± duration), and the documented global functions
  (`link`, `date`, `if`, `list`, `number`, `min`, `max`, `duration`, …) and
  per-type methods (`.contains`, `.toFixed`, `.format`, `.inFolder`, `.hasTag`,
  `.isEmpty`, `.mean`, …).
- **Formulas & summaries** — computed columns and footer aggregations
  (Sum/Average/Min/Max/Median/Range/Count/Unique/… plus custom `values.*`).
- **Links** — a note reference in a cell resolves to a link to that note's page.

## Property types

Values are typed. Frontmatter is interpreted the way Obsidian does: `[[wikilinks]]`
become links, `YYYY-MM-DD` strings become dates, sequences become lists. File
properties (`file.name`, `file.folder`, `file.ext`, `file.size`, `file.ctime`,
`file.mtime`, `file.tags`, `file.links`) come from the file itself.

## Graceful degradation

A base never breaks your build. A malformed `.base` (invalid YAML) renders a
**detailed, visible error block** naming the problem; an unparsable filter shows a
non-fatal warning and excludes the row; an unknown function or missing property
evaluates to empty — matching Obsidian's own forgiving evaluation. The build
always succeeds.

## Turning it off

Bases are on by default but inert unless a `.base` file or ` ```base ` block is
present. To disable entirely — so `.base` files are ignored and ` ```base ` blocks
render as plain code:

```toml
[features]
bases = false
```
