# Obsidian Bases support — design

**Status:** approved (autonomous overnight build; author is away)
**Date:** 2026-07-16
**Goal:** Faithfully render [Obsidian Bases](https://obsidian.md/help/bases) (`.base`
files and embedded ` ```base ` blocks) as static HTML at build time, as a new,
self-contained `docgen-bases` crate wired into the existing build pipeline.

## What Obsidian Bases are

A `.base` file is a YAML document describing database-like *views* over the
vault's notes. It has five optional top-level keys:

- `filters` — logical tree (`and`/`or`/`not` + expression strings) selecting notes.
- `formulas` — named computed properties (expression strings).
- `properties` — per-property display config (`displayName`).
- `views` — an ordered list of view configs (table/cards/list) each with its own
  `name`, `type`, `filters`, `order`, `sort`, `groupBy`, `limit`, `columnSize`,
  `summaries`.
- `summaries` — named custom aggregation expressions.

Expressions form a small language over typed values. Property references:
`note.<prop>` (frontmatter), `file.<prop>` (metadata: name/path/folder/ext/size/
ctime/mtime/tags/links/…), `formula.<name>`, `this.<…>`, and a **bare identifier**
is a note property (`type == "x"`, `categories.contains(...)`). There are global
functions (`link`, `date`, `now`, `today`, `list`, `number`, `if`, `min`, `max`,
`duration`, `icon`, `image`, …) and type methods (String/Number/Date/List/Link/
File/Object/Any — `.contains`, `.toFixed`, `.format`, `.inFolder`, `.hasTag`,
`.isEmpty`, `.mean`/`.sum`/`.round`, …). Operators: `+ - * / %`, `== != > < >= <=`,
`&& || !`, member `.`, index `[]`, call `()`, date±duration arithmetic.

Real vault examples (verified against a local Obsidian install):
```yaml
filters:
  and:
    - categories.contains(link("Categories/Books", "Books"))
    - '!file.inFolder("Misc")'
views:
  - type: table
    name: Table
    order: [file.name, file.size]
    sort:
      - property: file.name
        direction: DESC
    columnSize:
      file.name: 381
```

## Architecture

A new leaf crate **`docgen-bases`** (pure: no I/O, no networking) owns the whole
engine and renderer. It depends only on `serde`/`serde_yml`/`thiserror`. Layering:
`docgen-bases` ← `docgen-core` (embedded blocks) and ← `docgen-build` (standalone
pages + corpus). No dependency cycles.

Rendering is **fully static server-side HTML** (tables/cards/lists) styled by CSS
appended to the always-shipped `docgen.css`. No island JS, no runtime, no network
— matching docgen's build-time philosophy (like build-time KaTeX/PlantUML).

### `docgen-bases` modules

- `value.rs` — `Value` enum: `Null, Bool, Number(f64), Str(String), Date(BaseDate),
  Duration(i64 ms), List(Vec<Value>), Object(BTreeMap), Link(BaseLink)`. Truthiness,
  equality, ordering, `type_name`, display formatting. `BaseDate` (y/M/d/h/m/s/ms)
  and `BaseLink { path, display }`.
- `note.rs` — `Note` (the corpus row): `properties: BTreeMap<String, Value>` (from
  frontmatter), plus file metadata fields (`name, basename, path, folder, ext,
  size, ctime, mtime, tags, links`). `Corpus { notes: Vec<Note> }`. A
  `from_frontmatter` helper converts a `serde_yml::Value` frontmatter map + file
  facts into a `Note` (wikilink strings → `Link`, ISO date strings → `Date`, etc).
- `lexer.rs` — tokenizer: numbers, single/double-quoted strings, identifiers,
  `true/false/null`, operators/punctuation, `/regex/flags` literals.
- `ast.rs` + `parser.rs` — Pratt parser with correct precedence producing an `Expr`
  tree: literals, ident, `Member(a,b)`, `Index(a,b)`, `Call(callee,args)`, unary,
  binary. Method vs function call distinguished at eval (a `Call` whose callee is a
  `Member` is a method call).
- `eval.rs` — evaluator: `eval(expr, &EvalCtx) -> Value`. `EvalCtx` holds the
  current `&Note`, the `&Corpus` (for backlinks/link resolution), and resolved
  `formulas`. Namespaces (`note`/`file`/`formula`/`this`) and bare-identifier →
  note-property resolution. Never panics: unknown symbol/function/type-mismatch →
  `Value::Null` (Obsidian-tolerant), surfaced as empty cells.
- `functions.rs` — global function + method dispatch tables (the full documented
  set; unknowns degrade to `Null`).
- `format.rs` — Moment.js format-string subset for `date.format(...)` (`YYYY MM DD
  HH mm ss M D H h A a` + literals), and default value→cell rendering.
- `model.rs` — serde structs for the `.base` YAML (`BaseFile`, `View`, `Filter`,
  `SortKey`, `GroupBy`, …). Tolerant deserialization (unknown keys ignored).
- `filter.rs` — compile a `Filter` tree to a predicate; global + view filters
  AND-combined.
- `summary.rs` — built-in summaries (Sum/Average/Min/Max/Median/Range/Stddev/
  Count/Empty/Filled/Unique/Checked/Unchecked/Earliest/Latest) + custom `values.*`.
- `render.rs` — `render_base(&BaseFile, &Corpus, &RenderOptions) -> String` emits a
  `<div class="docgen-base">` with one section per view. Table view: filtered,
  sorted, grouped, limited rows; columns from `order` (default: all props seen);
  `displayName` headers; per-column summary footer; `columnSize` widths. Cards and
  list views render the same data in card/list layouts. Wraps in the existing
  horizontal-scroll table container idiom. `render_base_source(yaml, …)` parses and
  renders, emitting a styled **error block** (detailed message, never a panic) on
  malformed YAML/expressions — mirroring the PlantUML error-component ethos.
- `lib.rs` — re-exports the public API + `BaseError`.

### Corpus construction (`docgen-core::bases`)

`PreparedDoc` gains a `frontmatter: serde_yml::Value` field (retained from the
existing parse; currently discarded). `docgen-core::bases::note_from_doc(prepared,
file_facts) -> Note` builds a corpus row: frontmatter → typed `Value` properties;
`file.*` from slug/rel_path + caller-supplied `FileFacts { size, ctime, mtime }`;
`file.tags`/`file.links` parsed from body (reusing the wikilink scanner + a `#tag`
scan). `build_corpus(&[PreparedDoc], &dyn Fn(&str)->FileFacts) -> Corpus`.

### Integration in `docgen-build`

1. Discover `.base` files: `discover::discover_bases(root) -> BTreeMap<slug, yaml>`
   (mirrors `discover_diagrams`; pruned dirs respected; `.base` excluded from
   `discover_assets`).
2. After `prepare`, build the `Corpus` from prepared docs + `fs::metadata`
   (size/mtime/ctime; best-effort, empty when unavailable — git doesn't preserve
   mtime, acceptable fidelity).
3. **Embedded blocks:** pass `Option<&Corpus>` into `render_docs`/`render_doc`
   (single new param, mirroring the PlantUML `Option<&PlantumlSupport>` threading).
   A new `basepass.rs` AST pass replaces top-level ` ```base ` fenced blocks with
   the rendered view HTML (feature-gated; inert when the feature is off or no block
   is present). Nested-in-directive base blocks render as plain code (documented
   limitation, parity with how rare that is).
4. **Standalone pages:** for each `.base` file, render its views to `body_html`,
   synthesize a `Doc { slug, title (from filename or config), body_html, … }`, and
   append to `site.docs` **before** `build_tree` — so base files appear in the
   sidebar, get a clean-URL page, and a search entry, for free. Corpus excludes
   `.base` files (bases query notes, not other bases).
5. CSS: append `.docgen-base*` rules to `docgen/docgen.css` (always shipped) — no
   new asset slice or gating.
6. Incremental dev + editor preview (`docgen-server`): rebuild the corpus on the
   full-rebuild path; a changed `.base` file or frontmatter triggers a full rebuild
   (bases are cross-doc, so incremental single-page rebuild can't be trusted —
   fall back to full, like the diagrams check already does).

### Config

`[features] bases` (default `true`, matching every other feature). Optional
`[bases]` section reserved for future defaults (none needed now). When the feature
is off, `.base` files are ignored (not emitted as pages) and ` ```base ` blocks
render as plain code fences.

## Faithfulness scope

Implemented faithfully: the five top-level keys; the full filter logical tree;
formulas; the documented global functions and per-type methods; operators incl.
date±duration; property namespaces + bare-identifier resolution; `table`, `cards`,
and `list` views with `order`/`sort`/`groupBy`/`limit`/`columnSize`/`displayName`/
`summaries`; default + custom summaries; Moment format subset.

Deliberately out of scope (documented): live interactivity (sorting/filtering in
the browser — output is static), image/canvas embeds rendered as diagrams,
`file.backlinks`/`file.embeds` performance-heavy properties (backlinks supported
via the corpus; embeds best-effort), and base blocks nested inside directive
bodies. Unknown functions/properties degrade to empty (Obsidian-tolerant), never
crash the build.

## Testing

- `docgen-bases`: exhaustive unit tests per module — lexer, parser precedence,
  every function/method, filter evaluation against a fixture corpus, formula
  resolution, each view's HTML, summaries, Moment formatting, and graceful errors.
  Golden tests using the real vault base-file shapes (categories/link/inFolder).
- `docgen-core`: `PreparedDoc.frontmatter` retained; `build_corpus`; `basepass`
  transform (block → table, feature off → code, escaping).
- `docgen-build`: standalone `.base` → page + sidebar entry; embedded block in a
  page; feature-off path; corpus file metadata.
- Full `cargo test` + `cargo clippy` + `cargo fmt --check` green before PR.

## Rollout

Feature branch → PR (squash-merge) → CI green → release-plz version bump PR
(merge commit) → verify all crates publish (new `docgen-bases` publishes after
`docgen-core`, per the 0.5.0 new-crate ordering precedent). Dogfood: add a
`website/docs/features/bases.md` page (fenced examples) and, if a small notes
fixture fits, a live embedded example.
