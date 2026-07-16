---
title: Linting
---

# Linting

**What it is.** `docgen lint` checks a site for publishing problems — broken
links, missing assets, malformed diagrams and frontmatter, structural oddities —
before it ships. Linting is **advisory**: it is a separate command that never
runs as part of `docgen build` and never blocks it. A site with a hundred lint
findings still builds; the linter just tells you what a reader would trip over.

**Why you'd want it.** A broken `[[wikilink]]` renders as a marked-up span, a
missing image renders as a broken img — the build degrades gracefully, so
nothing forces you to notice. The linter turns those silent degradations into a
list you can review, and into an exit code your CI can gate on.

Because it is built on the same discovery, slug-resolution, and link-graph code
as `docgen build`, what the linter sees is exactly what the build sees.

## Quickstart

```sh
docgen lint            # lint the current directory
docgen lint path/to/project
```

Findings are grouped by file and colored by severity on a TTY:

```
guide/setup.md
  error[broken-wikilink] 12  wikilink `[[Instalation]]` does not resolve to any page
  warn[missing-title] -  page has no frontmatter title and no h1 heading

1 error, 1 warning · 47 files checked
```

Exit codes:

| Code | Meaning |
|---|---|
| `0` | no error-level findings (warnings and infos are fine) |
| `1` | at least one error-level finding |
| `2` | the lint run itself failed (bad config, unreadable root) |

## CLI flags

- `--format pretty|json|github|gitlab` — output format (default `pretty`).
  `json` emits `{"diagnostics": [...], "summary": {...}}`; `github` and
  `gitlab` are covered under [CI usage](#ci-usage) below.
- `--deny-warnings` — promote every warning to an error, so warnings affect
  the exit code. Suppressed warnings are never promoted.
- `--rules broken-wikilink,missing-asset` — run only the listed rules.
- `--quiet` — pretty output shows error-level findings only (the summary line
  keeps the true counts).
- `--list-rules` — print every rule with its default severity and description,
  then exit.

## The rules

Every rule has a kebab-case id and a default severity: `error`, `warn`, or
`info`. Any of them can be re-leveled or disabled in config (see below). This
table is what `docgen lint --list-rules` prints.

### Links & includes

| Rule | Default | Checks |
|---|---|---|
| `broken-wikilink` | error | wikilink target does not resolve to any page |
| `broken-relative-link` | error | relative markdown link does not resolve to a known page |
| `broken-include` | error | include directive target does not exist |
| `broken-anchor` | warn | wikilink anchor does not match any heading on the target page |

### Diagrams

| Rule | Default | Checks |
|---|---|---|
| `plantuml-src-missing` | error | plantuml directive `src` does not name a known `.puml` file |
| `plantuml-empty` | warn | plantuml directive has no `src` and an empty body |
| `mermaid-empty` | warn | mermaid fence has an empty body |
| `mermaid-unknown-type` | warn | mermaid fence does not start with a known diagram type |

### Assets & components

| Rule | Default | Checks |
|---|---|---|
| `missing-asset` | error | referenced image or asset file does not exist |
| `unknown-component` | error | directive does not name a known component |

### Frontmatter & metadata

| Rule | Default | Checks |
|---|---|---|
| `invalid-frontmatter` | error | frontmatter block is not valid YAML |
| `missing-title` | warn | page has no frontmatter title and no h1 heading |
| `duplicate-slug` | error | two pages resolve to the same slug |
| `invalid-base` | error | `.base` file is not valid YAML |
| `base-unknown-key` | warn | `.base` docgenInteractive block has unrecognized keys |

### Structure

| Rule | Default | Checks |
|---|---|---|
| `orphan-page` | info | page has no inbound links |
| `duplicate-heading` | warn | same heading text appears more than once on a page |
| `heading-level-jump` | info | heading level increases by more than one |
| `empty-page` | warn | page body is empty |
| `unused-partial` | info | partial is never included by any page or partial |

### Opt-in external checks

Three rules leave the process (a PlantUML server, the `mmdc` CLI, live HTTP
requests), so they are **off by default** and do nothing until enabled in
config. When enabled but the external dependency is unavailable (server
unreachable, `mmdc` not installed), the rule emits a single info-level
"check skipped" notice instead of a failure per diagram or URL.

| Rule | Default | Checks |
|---|---|---|
| `plantuml-syntax` | error | plantuml diagram fails server-side syntax validation — off unless `[lint.plantuml] check-syntax = true` |
| `mermaid-syntax` | error | mermaid fence fails `mmdc` validation — off unless `[lint.mermaid] check-syntax = true` |
| `external-url` | allow | external URL is unreachable or returns an HTTP error — off unless `[lint.rules]` raises its severity |

## Configuration

Everything lives in an optional `[lint]` section of `docgen.toml`. With no
config at all, the defaults above apply. A realistic example:

```toml
[lint]
# Globs (relative to docs/) of files the linter skips entirely.
ignore = ["drafts/**", "vendor/**"]

[lint.rules]
# Re-level any rule: "error" | "warn" | "info" | "allow" (= disabled).
orphan-page = "warn"          # promote: orphans should be noisy here
duplicate-heading = "allow"   # disable a rule site-wide
external-url = "error"        # enable the opt-in URL check by raising it above allow

[lint.plantuml]
check-syntax = true           # validate diagrams against the [plantuml] server

[lint.mermaid]
check-syntax = true           # validate fences by shelling out to mmdc
mmdc = "mmdc"                 # path to / name of the mermaid CLI binary

[lint.external-urls]
timeout-secs = 10             # per-request timeout (default 10)
exclude = ["https://intranet.example.com/**"]   # URL patterns never probed
```

Notes on the external checks:

- **`plantuml-syntax`** validates against the same PlantUML server (and
  on-disk cache) that `docgen build` uses — see [[plantuml]] for how the
  server URL is resolved.
- **`mermaid-syntax`** requires the
  [mermaid CLI](https://github.com/mermaid-js/mermaid-cli) (`mmdc`) on your
  `PATH`, or point `mmdc` at the binary.
- **`external-url`** probes each distinct `http(s)://` URL once (HEAD, with a
  GET retry for servers that reject HEAD) and skips localhost URLs. It is
  network-bound, so consider enabling it only in a scheduled CI job.

## Per-page suppression

A page can opt out of specific rules for itself via frontmatter — useful for
the one intentionally-orphaned page or a deliberate example of a broken link:

```yaml
---
title: Scratch notes
lint:
  ignore: [orphan-page, broken-wikilink]
---
```

Suppression applies only to findings attributed to that file, and a suppressed
warning is never promoted by `--deny-warnings`.

## CI usage

In CI you typically want warnings to fail the pipeline too:

```sh
docgen lint --deny-warnings
```

### GitHub Actions

`--format github` emits [workflow command](https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions)
annotations (`::error file=docs/page.md,line=12::message [rule-id]`), so
findings show up inline on the PR diff:

```yaml
- name: Lint docs
  run: docgen lint --format github --deny-warnings
```

### GitLab Code Quality

`--format gitlab` emits a [Code Quality](https://docs.gitlab.com/ee/ci/testing/code_quality.html)
report (a JSON array with stable fingerprints), which GitLab renders as a
quality widget on merge requests:

```yaml
lint-docs:
  script:
    - docgen lint --format gitlab > gl-code-quality.json || true
  artifacts:
    reports:
      codequality: gl-code-quality.json
```

(The `|| true` keeps the report-producing job green so the widget always
renders; drop it — or add a second plain `docgen lint` job — if lint errors
should fail the pipeline.)
