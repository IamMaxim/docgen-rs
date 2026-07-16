---
title: Obsidian Bases
---

# Obsidian Bases

**What it is.** docgen understands [Obsidian Bases](https://obsidian.md/help/bases)
— the `.base` file format that turns your notes into filtered, sorted, grouped
**database views**. A `.base` file becomes its own page, and a ` ```base ` fenced
block embeds a view inline in any markdown page. Everything is computed **at build
time** into static HTML — no database, no server.

The static view is the baseline (it renders fully with JavaScript off, so it stays
fast and indexable). On top of it, a small script **progressively enhances** each
view into an [interactive one](#interactive-views): filter, search, sort, and page
through the data live in the browser, with shareable URLs.

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

## Interactive views

Every rendered view is enhanced in the browser with a control bar, so a reader can
explore the data without a rebuild:

- **Search** — a free-text box that filters across the visible columns.
- **Faceted filters** — auto-generated per column: a multi-select for
  low-cardinality text/tag columns (kind, area, status, …), a **date range** for
  date columns, a **number range** for numeric ones.
- **Sort / reorder** — click a table header to sort ascending → descending → off;
  cards and list views get a sort dropdown. Version columns sort as versions
  (see below), not as text.
- **Pagination** — large views page in the browser (see `pageSize` below).
- **Shareable URLs** — the active filters, sort, and page are encoded in the URL, so
  a filtered view (e.g. *releases touching one area in a date window*) is a
  copyable, reloadable link.

The controls are **derived automatically** from the columns — no configuration
needed. The static HTML remains the no-JavaScript baseline, so every row is still
present and readable with scripting disabled.

**A worked example** is the **Releases** page in the sidebar (`/releases`) — a
`.base` over docgen's own release history, filterable by kind / area / date range
/ number of crates published, searchable, and sorted by a real version column.

### Version columns sort as versions

Sorted as text, `1.19.20` lands *before* `1.2.12` — `'1' < '2'` at the third
character. So a column whose values **all** parse as versions is ordered
numerically instead, with no configuration:

| | order |
|---|---|
| as text | `1.0.23`, `1.19.20`, `1.2.12` |
| as versions | `1.0.23`, `1.2.12`, `1.19.20` |

Parsing is lenient: `v1.2.3` and `1.2` are accepted (`1.2` means `1.2.0`), build
metadata (`+build.5`) is ignored, and a pre-release sorts before its release
(`1.0.0-rc.1` → `1.0.0-rc.2` → `1.0.0-rc.11` → `1.0.0`).

A single non-version value (`nightly`) turns detection off and the column stays
text — ordering never depends on *some* rows. Columns of plain numbers are
untouched: a YAML `version: 1.5` is a number, and numbers already sort
numerically. Override either way with `sortAs` below.

This applies to the static build too, so a `sort:` on a version column is already
in version order before any JavaScript runs.

### Tuning the controls (`docgenInteractive`)

Controls are auto-derived, but you can override them per view (or disable the whole
feature) with a **docgen-specific** `docgenInteractive` key. It is namespaced so
Obsidian ignores it — your `.base` stays portable:

```yaml
views:
  - type: table
    order: [file.name, note.service, note.version]
    docgenInteractive:
      pageSize: 25            # rows per page (0 = no pagination)
      search: true           # show the search box (default true)
      maxEnum: 40            # ≤ this many distinct values ⇒ a facet, else text
      filters:
        note.version: text   # force a widget: none|text|enum|date|number|boolean
      sortAs:
        note.version: semver # force version order: semver|text
      sortable:
        note.notes: false    # disable sorting on a column
      defaultSort:
        - property: note.service
          direction: ASC
```

`sortAs: semver` forces version order on a column that would not be detected
(values that fail to parse sort last); `sortAs: text` opts a detected column back
out. Unlike the other keys here, `sortAs` also affects the static build, because
a view's `sort:` is a build-time ordering.

To turn interactivity off for a single base, add `docgenInteractive: false` at the
top level; the view still renders as static HTML.

## What's supported

docgen implements the Bases format faithfully:

- **Five sections** — `filters`, `formulas`, `properties`, `summaries`, `views`.
- **Filters** — the full `and`/`or`/`not` logical tree over expression strings.
- **Views** — `table`, `cards`, and `list`, each with its own `name`, `filters`,
  `order`, `sort`, `limit`, `columnSize`, and `summaries`. `groupBy` renders
  group headings on **`table` views only**; `cards` and `list` parse it and
  ignore it, so a grouped view of either renders ungrouped rather than failing.
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
