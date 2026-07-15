---
title: Components
---

# Components

Components let you drop reusable, styled building blocks into Markdown. docgen
ships **built-in callouts** and lets you define **custom components** as plain
HTML templates.

## Built-in callouts

**What it is.** A `callout` block for admonitions — notes, warnings, tips.

**Why you'd want it.** Highlight the thing readers must not miss without dropping
into raw HTML.

### Syntax

```markdown
:::callout{type=warning title="Heads up"}
This is a **callout**. The body is normal Markdown.
:::
```

### Live example

:::callout{type=warning title="Heads up"}
This callout is rendered by docgen right here on the page. The body is regular
Markdown, so **bold**, `code`, and [[wikilinks]] all work inside it.
:::

:::callout{type=note}
`type` accepts values like `note`, `warning`, and `tip`; `title` is optional.
:::

You can override the built-in callout's appearance by adding your own
`components/callout/template.html` — custom components with the same name win.

## Custom components

**What it is.** Your own component, defined as a `template.html` (plus an
optional `style.css`) in the components directory.

**Why you'd want it.** Encapsulate a bit of styled markup — a badge, a note pill,
a product-specific callout — and invoke it by name across your docs.

### Defining one

Create `components/<name>/template.html`. This site ships a `note` component:

```
components/
└── note/
    ├── template.html   →  <span class="docgen-note">📝 {{ label }}</span>
    └── style.css       →  .docgen-note{ … }
```

`{{ label }}` is filled from the text you pass when invoking it. The `style.css`
is bundled into the site automatically.

The components directory defaults to `components/`; change it in
[[configuration]] with `[components] dir = "…"`.

### Invoking one

```markdown
Inline: :note[a project component]{}
```

### Live example

Here is the `note` component rendered inline: :note[a project component]{}
— defined by this very site's `components/note/` template.
