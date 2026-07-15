---
title: Features
---

# Features

docgen does a lot out of the box. This section has a page per feature — each one
explains what it is, why you'd want it, the exact syntax, and shows a live
example rendered by docgen itself.

> This page is a **folder note**: `docs/features/index.md` is the landing page
> for the `features/` folder, and its children appear nested under it in the
> sidebar. Any folder can have one — see [[includes]].

## The features

- **[[wikilinks]]** — connect pages with `[[target]]` links and get automatic
  backlinks and broken-link marking.
- **[[search-and-graph|Search & knowledge graph]]** — a static full-text search
  modal and an interactive graph of how your docs link together.
- **[[math-and-mermaid|Math & diagrams]]** — LaTeX math and mermaid diagrams,
  both rendered at build time.
- **[[plantuml|PlantUML diagrams]]** — sequence, class, and activity diagrams
  rendered to inline SVG at build time against a PlantUML server.
- **[[bases|Obsidian Bases]]** — `.base` files and `base` code blocks become
  filtered, sorted database views over your notes, rendered to static HTML.
- **[[includes|Includes & partials]]** — transclude shared Markdown with
  `:include`, and keep helper files out of the sidebar with a `_` prefix.
- **[[components]]** — built-in callouts and your own HTML/CSS components.
- **[[history|Git-history timeline]]** — a per-document page showing how it
  evolved, with line- and block-level diffs.
- **[[s3-offload|S3 asset offload]]** — optionally store large binaries in an
  S3-compatible bucket and serve them from a CDN.

Prefer a linear read? Start at [[wikilinks]] and follow the links.
