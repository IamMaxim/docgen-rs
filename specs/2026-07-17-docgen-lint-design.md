# `docgen lint` — Design

Date: 2026-07-17
Status: approved

## Goal

A `docgen lint` subcommand that pinpoints publishing problems — broken diagrams, links, assets, metadata, and structural issues — before a site ships. Linting is advisory: it never runs as part of `docgen build` and never blocks it. It follows established linter conventions: severity levels, config-file control, per-rule granularity, machine-readable output for CI.

## Architecture

A new workspace crate, **`docgen-lint`**, owns the diagnostics framework and all rules. It consumes existing docgen-core primitives so lint findings cannot drift from real build behavior:

- `discover::discover_docs` / `discover_assets` / `discover_diagrams` / `discover_bases`
- `pipeline::prepare` → `PreparedDoc` (raw markdown + parsed frontmatter)
- `SlugSet` + `wikilink::resolve_target` for link resolution
- `graph::build_link_graph` for orphan detection
- `PlantumlError` (already classified, with server-reported line numbers) for PlantUML syntax findings
- `docgen_bases::parse_base` + `unknown_key_warning` for `.base` validation

The CLI (`crates/docgen`) gains a `Lint` subcommand wired to this crate.

### Required docgen-core changes (minimal)

1. Extend `WikilinkPass` to also collect **unresolved** wikilink targets with source positions (today broken links only become a CSS-classed span).
2. Expose per-doc heading collection so anchor targets (`[[page#heading]]`) can be validated.
3. Make directive-body traversal reachable by the linker pass so links inside directive bodies are linted (closes the documented gap in `pipeline.rs`).

## Diagnostics model

```rust
struct Diagnostic {
    rule: RuleId,          // kebab-case, e.g. "broken-wikilink"
    severity: Severity,    // Error | Warn | Info  (Allow = suppressed, never emitted)
    file: PathBuf,         // relative to docs/
    line: Option<u32>,
    col: Option<u32>,
    message: String,
    note: Option<String>,  // optional help/fix hint
}
```

Every rule has: a kebab-case id, a default severity, and a one-line description (shown by `docgen lint --list-rules`).

## Rules (v1)

| Rule | Default | Description |
|---|---|---|
| **Links** | | |
| `broken-wikilink` | error | `[[target]]` that resolves to no page |
| `broken-relative-link` | error | Relative markdown link to a missing file |
| `broken-embed` | error | `![[embed]]` that resolves to nothing |
| `broken-anchor` | warn | `[[page#heading]]` where the heading doesn't exist |
| `external-url` | allow | HTTP-check external links (opt-in, network) |
| **Diagrams** | | |
| `plantuml-src-missing` | error | `:::plantuml src=` file doesn't exist |
| `plantuml-empty` | warn | PlantUML directive with no body and no src |
| `plantuml-syntax` | error, **opt-in** | Render via configured server; server line numbers surfaced. Skipped with an info note if server unreachable |
| `mermaid-empty` | warn | Empty ```mermaid fence |
| `mermaid-unknown-type` | warn | First line isn't a known mermaid diagram type (heuristic) |
| `mermaid-syntax` | error, **opt-in** | Validate by shelling out to configured `mmdc` |
| **Assets & refs** | | |
| `missing-asset` | error | Image/file referenced from a page doesn't exist |
| `unknown-component` | error | `:::name` directive with no matching component dir |
| **Frontmatter & metadata** | | |
| `invalid-frontmatter` | error | YAML frontmatter fails to parse |
| `missing-title` | warn | No title derivable |
| `duplicate-slug` | error | Two pages resolve to the same slug |
| `invalid-base` | error | `.base` file fails to parse |
| `base-unknown-key` | warn | Unrecognized `.base` keys (reuses `unknown_key_warning`) |
| **Structure** | | |
| `orphan-page` | info | No inbound links (via link graph) |
| `duplicate-heading` | warn | Same heading text twice in one page |
| `heading-level-jump` | info | e.g. h1 → h3 |
| `empty-page` | warn | Page with no body content |
| `unused-partial` | info | `_*.md` partial never included |

All external-tool checks (`plantuml-syntax`, `mermaid-syntax`, `external-url`) are opt-in via config: if the user sets up the environment, they can enable them.

## Configuration

`[lint]` section in `docgen.toml`, parsed by docgen-config. All defaults are sensible with no config present.

```toml
[lint]
ignore = ["drafts/**"]          # globs relative to docs/

[lint.rules]
orphan-page = "warn"            # re-level any rule: error|warn|info|allow
external-url = "error"          # enabling an opt-in rule = raising it above allow

[lint.plantuml]
check-syntax = true             # opt-in; uses [plantuml].server

[lint.mermaid]
check-syntax = true
mmdc = "mmdc"                   # path to the mermaid CLI binary

[lint.external-urls]
timeout-secs = 10
exclude = ["https://internal.example.com/**"]
```

### Per-page suppression

Frontmatter only:

```yaml
lint:
  ignore: [broken-wikilink, orphan-page]
```

## CLI

`docgen lint [root]`

- `--format pretty|json|github|gitlab` — default `pretty`; `github` emits `::error file=…,line=…::…` workflow commands; `gitlab` emits Code Quality report JSON
- `--deny-warnings` — promote warns to errors (CI)
- `--rules <name,…>` — run only the listed rules
- `--quiet` — errors only
- `--list-rules` — print all rules with default severity and description

Exit codes: `0` no error-level findings; `1` at least one error; `2` operational failure (bad config, unreadable root).

## Execution flow

1. Load `docgen.toml` (missing file → defaults).
2. Discover docs, assets, diagrams, bases.
3. `prepare` each doc; build `SlugSet`, heading index, link graph.
4. Run pure rules over all docs; then network/external rules (bounded concurrency).
5. Filter through config ignores + frontmatter suppressions.
6. Sort by file, then line; emit in chosen format; set exit code.

## Output (pretty)

Findings grouped by file, colored by severity:

```
docs/guide/setup.md
  error[broken-wikilink] 12:5  [[Instalation]] does not resolve to any page
  warn[missing-title]          page has no derivable title

3 errors, 2 warnings, 1 info · 47 files checked
```

## Testing

- Unit tests per rule (inline `#[cfg(test)]`, following repo convention).
- New fixture `fixtures/site-lint/` with deliberately broken wikilinks, missing assets, malformed frontmatter, bad `.base`, orphan pages, empty diagrams.
- Integration test `crates/docgen/tests/lint_cli.rs`: exit codes, `--format json` shape, `--deny-warnings`.
- Network rules (`plantuml-syntax`, `external-url`) tested against a local stub HTTP server.

## Out of scope (v1)

- Auto-fixing (`--fix`)
- Inline comment suppression (`<!-- docgen-lint-ignore -->`)
- Spell/style/prose checks
- Watch mode / editor (LSP) integration
