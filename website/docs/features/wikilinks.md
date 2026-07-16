---
title: Wikilinks & backlinks
---

# Wikilinks & backlinks

**What it is.** A `[[double-bracket]]` link syntax for connecting pages by name
instead of by file path, plus an automatic **backlinks** section on every page
showing what links *to* it.

**Why you'd want it.** In a growing docs tree, hard-coded relative paths
(`../../guide/intro.md`) break every time you move a file. Wikilinks resolve by
target name, so you can reorganize freely. Backlinks turn your docs into a small
knowledge base: every page tells you what depends on it.

## Syntax

```markdown
[[getting-started]]              → links to the Getting started page
[[getting-started|start here]]   → same link, custom label "start here"
[[features/history]]             → link by path when names would be ambiguous
[[getting-started#Install]]      → links straight to a heading on that page
[[#Syntax]]                      → links to a heading on the current page
```

- `[[target]]` uses the target's name as the link text.
- `[[target|label]]` uses your own label.
- Targets resolve by page name; use a path segment (`features/history`) to
  disambiguate when two pages share a name.
- `[[target#Heading]]` resolves the page and jumps to that heading (the anchor
  is slugified the same way heading ids are); the link text stays
  `target#Heading` unless a label overrides it. `[[#Heading]]` links within
  the current page. Anchored links count as real links — they appear in
  backlinks and the graph like any other wikilink.

## Backlinks

Every page automatically gets a **Backlinks** section listing the pages that
link to it — no configuration, no manual maintenance. This page, for example, is
linked from [[features/index|the features overview]] and [[index|the home page]],
so both appear in its backlinks below.

## Broken links are marked, not fatal

If a `[[target]]` doesn't resolve, docgen does **not** fail the build. Instead it
renders the link as a marked broken span (CSS class `docgen-wikilink--broken`)
so you can spot it visually and style it, while the rest of the site builds
normally. docgen's own CI greps the built output for that marker, so a typo'd
link fails the pipeline before it ships — a pattern you can reuse (see
[[deployment]]).

## See it in context

This paragraph links to [[math-and-mermaid]] and [[search-and-graph]]. Open the
[[search-and-graph|knowledge graph]] and you'll see these pages connected by the
very links written here.
