---
title: Search & knowledge graph
---

# Search & knowledge graph

Two features that help readers find their way around a larger docs set. Both are
computed at build time and enabled by default (toggle them in
[[configuration]] under `[features]`).

## Full-text search

**What it is.** A <kbd>Ctrl</kbd>/<kbd>⌘</kbd>+<kbd>K</kbd> search modal backed
by a prebuilt `search-index.json`.

**Why you'd want it.** Readers expect to jump straight to the page they need.
docgen indexes your prose at build time, so search is instant and works on a
purely static host — there is **no search server and no JavaScript build step.**

**Try it now.** Press <kbd>Ctrl</kbd>/<kbd>⌘</kbd>+<kbd>K</kbd> and search for
"mermaid" or "backlinks". The modal is vendored and dependency-free.

The index is emitted as `search-index.json` alongside your pages. Even the
display text of a broken [[wikilinks|wikilink]] is indexed, so searching finds
prose that mentions a not-yet-created page.

## Knowledge graph

**What it is.** An interactive `/graph/` page rendering every document as a node
and every [[wikilinks|wikilink]] as an edge.

**Why you'd want it.** The graph makes the *shape* of your documentation visible:
clusters of related pages, hubs everything links to, and orphans nothing points
at. It's a fast way to spot gaps in a knowledge base.

**Try it now.** Open the **Graph** link in the navigation. You'll see this page
wired to [[wikilinks]], [[math-and-mermaid]], and the rest of the docs by the
links written throughout the site.

## Turning them off

Both are on by default. To drop a feature's output and its client-side
JavaScript entirely:

```toml
[features]
search = false
graph = false
```
