---
title: Includes & partials
---

# Includes, partials & folder notes

Three related features for structuring a docs tree: **includes** (reuse shared
Markdown), **partials** (helper files that don't get their own page), and
**folder notes** (a landing page for a folder).

## Includes

**What it is.** The `:include` directive transcludes another Markdown file into
the current page, rendered through the full pipeline.

**Why you'd want it.** Keep a piece of content — an install snippet, a warning, a
support matrix — in one file and pull it into many pages. Edit once, update
everywhere.

### Syntax

```markdown
:include{src="./_install-snippet.md"}
```

The `src` path is resolved relative to the including document. The included file
is rendered exactly as if its content were inline, so wikilinks, math, and
components inside it all work.

## Partials

Any `.md` file whose basename starts with an underscore (`_install-snippet.md`)
is an **include-only partial**:

- It is excluded from page discovery — no standalone page, no sidebar entry, no
  search result.
- It remains a valid `:include` target.

This lets you keep reusable fragments right next to the pages that use them
without cluttering navigation.

## Graceful degradation

Includes never break your build. A missing target or an include cycle degrades
to an inert error span on the page — the build still succeeds. You get a visible
signal without a failed pipeline.

## Folder notes

**What it is.** An `index.md` inside a subfolder becomes that folder's landing
page — a **folder note**.

**Why you'd want it.** Instead of a bare folder in the sidebar that expands to a
list, the folder itself is a real page you can write an introduction on, with
its children nested beneath it.

The page you're reading sits under `docs/features/`, and
[[features/index|the Features page]] is that folder's note — open it and notice
how every feature page nests under it in the sidebar.
