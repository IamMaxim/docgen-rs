---
title: PlantUML diagrams
---

# PlantUML diagrams

**What it is.** The `:::plantuml` block directive renders a
[PlantUML](https://plantuml.com) diagram at **build time** and embeds the
resulting SVG directly into the page. Unlike mermaid (which renders in the
reader's browser), PlantUML has no browser renderer, so docgen renders it against
an external PlantUML server while building — the published site stays fully
static, with no runtime dependency on the server.

**Why you'd want it.** Sequence diagrams, class diagrams, activity diagrams,
state machines, and the rest of PlantUML's rich diagram set — kept as text
alongside your docs, versioned in git, and rendered into crisp inline SVG.

## Syntax

Reference a diagram stored in a separate `.puml` file (relative to the current
document, or docs-root-absolute with a leading `/`):

```markdown
:::plantuml{src="diagrams/architecture.puml"}
:::
```

Or write the source inline in the block body:

```markdown
:::plantuml
@startuml
Alice -> Bob : Authentication request
Bob --> Alice : Authentication response
@enduml
:::
```

If both are given, `src` wins. Diagram `.puml` files are treated as build
inputs, not published assets — they are never copied into the output.

## Wide diagrams

Diagrams render at their natural size. One that is wider than the reading column
**scrolls horizontally inside its own box** rather than being scaled down to fit
— PlantUML draws at a fixed size and asks that its output not be re-scaled, so
squeezing a wide diagram into the column distorts it instead of shrinking it. The
page itself never scrolls sideways; only the diagram does.

## Running a server

Diagrams render against a PlantUML server. The quickest way to get one is the
bundled command, which runs the official image in a container:

```bash
docgen plantuml           # runs plantuml/plantuml-server:jetty on :8080
```

It runs in the foreground; press <kbd>Ctrl-C</kbd> to stop (the container is
auto-removed). Run it in one terminal and `docgen build` / `docgen dev` in
another. The default server URL (`http://localhost:8080`) already points at it.

Override the container runtime with `DOCGEN_CONTAINER_RUNTIME` (e.g. `podman`).

## Configuring the server URL

Point docgen at any PlantUML server, in order of precedence:

1. the `DOCGEN_PLANTUML_SERVER` environment variable,
2. the `[plantuml]` section of `docgen.toml`,
3. the default `http://localhost:8080`.

```toml
[plantuml]
server = "http://plantuml.internal:8080"
```

## Caching

Every rendered diagram is cached on disk under `.docgen/plantuml-cache/`, keyed
by a hash of the diagram source and the server URL. Unchanged diagrams are served
straight from the cache — so incremental dev rebuilds stay fast, and a full
rebuild still succeeds for cached diagrams even when the server is unreachable.
The cache directory is git-ignored automatically.

## Graceful degradation

A diagram never breaks your build. If the server is unreachable, a diagram has a
syntax error, or a `src` file is missing, docgen emits a **detailed, visible
error component** in the page — naming the diagram and the specific failure
(the server URL and transport error, or PlantUML's own message and line number)
— and the build still succeeds. You get an actionable signal instead of a broken
pipeline or a generic "an error occurred".

## Turning it off

PlantUML rendering is on by default but inert unless a `:::plantuml` directive is
present (no directives → no server contact). To disable it entirely so any
`:::plantuml` directive renders a "disabled" notice:

```toml
[features]
plantuml = false
```
