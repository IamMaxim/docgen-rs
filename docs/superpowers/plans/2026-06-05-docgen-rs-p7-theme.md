# docgen-rs P7 — Theme / Design Implementation Plan

Status: PLAN (not yet implemented). Branch `overnight/p1-p6`, local commits only.

Goal: turn the functionally-complete-but-unstyled docgen-rs site into a polished,
restrained, professional documentation theme with full light + dark modes, a sticky
topbar, a proper sidebar doc tree, a centered readable content column, an optional
right rail, and styling for every surface the build emits — matching the look of the
original Svelte `docgen` app (`/Users/maxim/work/docgen/packages/docgen/src/lib`).

This plan ports the original Svelte design tokens and per-component styling into
hand-authored CSS for the static, island-based docgen-rs site. No npm/node/bundler.

---

## 0. Ground truth (what exists today)

Templates (minijinja, HTML auto-escape via `.html` names):
- `crates/docgen-render/templates/page.html` — `<nav.docgen-sidebar>` (raw `<ul>`
  tree via `render_nodes` macro) + `<main.docgen-content>` + fixed `.docgen-search-trigger`
  button + island scripts. Links `docgen.css`, conditional `components.css`, `katex.min.css`.
- `crates/docgen-render/templates/graph.html` — same sidebar + `<main.docgen-content.docgen-graph-page>`.
- `crates/docgen-render/templates/history.html` — same sidebar + `<main.docgen-content.docgen-history>`
  with `.docgen-timeline-bucket`, `.docgen-commit`, `.docgen-diff-file`, `.docgen-diff-line--{kind}`.

Template contexts (`crates/docgen-render/src/lib.rs`):
- `PageContext`: `title, slug, body_html, tree, backlinks, has_history, has_mermaid,
  has_math, base, site_title, search_enabled, has_components_css, has_component_island`.
- `GraphContext`: `tree, graph_json, node_count, edge_count, base, site_title, search_enabled`.
- `HistoryContext`: `title, slug, tree, buckets, base, site_title`.
- `Renderer::new(page_template)` registers `page.html` (caller-supplied), plus the
  built-in `history.html` / `graph.html` from `DEFAULT_*_TEMPLATE` (`include_str!`).
- `render_page/render_graph/render_history` pass these vars through `context!`.

TreeNode model (`crates/docgen-core/src/model.rs`):
```
enum TreeNode { Dir { name, children }, Doc { name, slug, title } }   // serde tag="kind"
```
NOTE: `Dir` has **no slug** — only `Doc` is linkable. Active state in the sidebar is
computed in-template by comparing `node.slug == slug` (the current page slug, already
in every context — add it to graph/history if needed for active state; see §4).

CSS / assets (`crates/docgen-assets`):
- `assets/docgen/docgen.css` (~58 lines) — flat light-only colors, NO tokens, NO themes.
- `assets/docgen/bootstrap.js` — defines `window.docgen.island(name, fn)` registry +
  `docgen.loadScript`, runs registered islands on `alpine:init`.
- `assets/docgen/search.js` — builds the modal DOM at runtime: `.docgen-search-modal`,
  `.docgen-search-backdrop`, `.docgen-search-box`, `.docgen-search-input`,
  `.docgen-search-results`, `.docgen-search-result` (`.is-selected`), inner `.title`.
- `assets/docgen/islands/{graph.js, mermaid.js}` — graph SVG / mermaid container.
- `assets/docgen/dev/editor.css` (DEV ONLY) — `.docgen-edit-toggle`, `#docgen-editor`,
  `.docgen-edit-save`. Injected server-side via `inject_dev_html` (docgen-server/src/lib.rs:191).
- `components/callout/style.css` — `.docgen-callout`, `--warning/--danger/--note`,
  `__title`, `__body` (hardcoded dark `#0b1220` bg — must be tokenized).
- `src/lib.rs`: `core_assets()` emits `bootstrap.js, docgen.css, search.js, alpine`.
  `assets_for()` planner; `emit()`. Asset existence/key-rule tests live here.

Emission contract: `docgen.css` is a single embedded file emitted to `dist/docgen.css`
and linked by all three templates. Component CSS is a separate concatenated
`components.css`. There is no SCSS/bundler — author plain CSS in one file.

---

## Design decision: syntect code blocks in dark mode

The build renders fenced code via syntect with the **InspiredGitHub** light theme,
producing `<pre><code>` with inline `style="color:#..."` spans on a light assumption.
Those inline colors win over any class rule and read poorly on a dark surface.

DECISION: **Give every code block a stable light "code card" surface in BOTH themes.**
- `.doc-content pre` (the syntect block wrapper) keeps a fixed light code surface
  (`--code-surface: #f6f8fa`, border `--code-border-stable`) regardless of `data-theme`,
  so the InspiredGitHub inline span colors always sit on the background they were
  generated for and stay WCAG-legible. This is the lowest-risk, deterministic choice
  and is visually common in dark docs themes (light code cards on dark page).
- Inline code (`:not(pre) > code`) is theme-aware (uses `--code-bg`/`--code-border`
  tokens) because it carries no syntect inline colors — it inherits `--text`.
- Diff hunks (`history.html`) are plain text (no syntect) — fully theme-aware via tokens.
- Document this in a top comment in `docgen.css` and assert the stable-surface rule
  exists (key-rule test, §10). If a future syntect dark theme is wired, flip the wrapper
  to a token; out of scope for P7.

(Alternative considered + rejected: emitting a second syntect dark theme and swapping
`<pre>` variants by `data-theme` — doubles HTML weight and needs render-crate changes;
not worth it for P7.)

---

## Cluster A — Layout restructure + tokens + base typography + theme toggle (+ no-flash)

### A1. Design tokens (CSS custom properties, light + dark)

Author at the TOP of `assets/docgen/docgen.css`. Light is the DEFAULT (`:root`), dark
under `:root[data-theme="dark"]`. (Original Svelte defaults to dark; we default to
**light** because most static docs are light-first and our syntect theme is light — the
no-flash script still honors `prefers-color-scheme`.)

Token set (names are the public contract — see BUILD_STATUS):

Colors / surfaces / borders / text:
```
--bg, --bg-elev, --bg-soft,
--surface, --surface-hi,
--border, --border-strong, --hairline,
--text, --text-dim, --text-mute, --text-error,
--accent, --accent-soft, --accent-line,
--warn, --warn-soft, --info, --info-soft, --talk, --talk-soft,
--code-bg, --code-border,                 /* inline code, theme-aware */
--code-surface, --code-border-stable,     /* syntect block, STABLE across themes */
--diff-added, --diff-added-bg, --diff-removed, --diff-removed-bg,
--diff-modified, --diff-renamed
```
Typography:
```
--font-sans:  system-ui, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
--font-mono:  ui-monospace, 'SFMono-Regular', 'JetBrains Mono', 'Fira Code', monospace;
```
(NOTE: original uses 'Geist'/'Geist Mono' web fonts; docgen-rs ships none — use system
stacks, no @font-face, no network fonts.)

Type scale (base 16px, line-height 1.6 — task spec; original used 14.5/1.55):
```
--fs-base:16px; --lh:1.6;
--fs-h1:30px; --fs-h2:22px; --fs-h3:18px; --fs-h4:15px; --fs-small:13px; --fs-code:13.5px;
```
Spacing scale + radii + shadows + layout:
```
--sp-1..--sp-8 (4,8,12,16,24,32,48,64px),
--r-xs:3px; --r-sm:5px; --r-md:8px; --r-lg:12px;
--shadow-sm, --shadow-md, --shadow-lg,
--left-rail-width:272px; --right-rail-width:240px; --topbar-height:54px;
--content-measure:760px;
```

Light values (port from original `tokens.css` `:root[data-theme='light']`, warmed-paper
palette): `--bg:#faf8f3; --bg-elev:#fff; --surface:#f4f1ea; --surface-hi:#ece8de;
--border:#e2ddd0; --border-strong:#d5cfbf; --hairline:#e8e3d6; --text:#181712;
--text-dim:#5f5d54; --text-mute:#918e83; --accent: oklch(0.55 0.14 60); ...`
Code stable surface (both themes): `--code-surface:#f6f8fa; --code-border-stable:#e1e4e8;`

Dark values (port from original `tokens.css` `:root` dark block): `--bg:#0d0c0a;
--bg-elev:#131210; --surface:#15140f; --surface-hi:#1c1b16; --border:#232220;
--text:#ecebe5; --text-dim:#a8a59a; --text-mute:#6f6d64; --accent: oklch(0.78 0.13 75); ...`
Diff tokens dark: `--diff-added: oklch(0.76 0.12 145); --diff-added-bg: oklch(0.76 0.12 145 / .09);
--diff-removed: var(--text-error); --diff-removed-bg: oklch(0.67 0.16 25 / .1);`

Base reset (port from original `tokens.css`):
```
*{box-sizing:border-box}
html{scroll-padding-top:calc(var(--topbar-height) + 18px)}
body{margin:0;background:var(--bg);color:var(--text);font-family:var(--font-sans);
     font-size:var(--fs-base);line-height:var(--lh);-webkit-font-smoothing:antialiased}
button,input{font-family:inherit} button{cursor:pointer}
:focus-visible{outline:2px solid var(--accent);outline-offset:2px;border-radius:2px}
```
Contrast: verify AA (≥4.5 body text, ≥3 large/UI) for `--text`/`--text-dim` on `--bg`
in BOTH themes (the ported palettes already pass; re-check `--text-mute` on `--surface`).

### A2. Page layout shell — `page.html`

Restructure to a CSS-grid app shell. New semantic classes (public contract):
- `<html lang="en" data-theme="light">` (default attr; no-flash script overrides pre-paint)
- `<body class="docgen-app">`
- `<header class="docgen-topbar">` (sticky) containing:
  - `<a class="docgen-topbar__brand" href="{{base}}/">` with `<span class="docgen-brand-mark">`
    + `<span class="docgen-brand-name">{{ site_title or "Docs" }}</span>`
  - `<div class="docgen-topbar__actions">`:
    - `<button class="docgen-search-trigger" data-docgen-search>` (MOVE the existing
      search trigger here from its fixed position; keep `data-docgen-search` + `<kbd>`)
    - the ThemeToggle markup (§A4)
    - `<button class="docgen-sidebar-toggle" aria-label="Toggle navigation" aria-expanded="false">`
      (hamburger, shown < 768px, drives the drawer via a tiny inline/Alpine handler — see §A4/§9)
- `<div class="docgen-layout">` (grid: `--left-rail-width minmax(0,1fr) auto`)
  - `<nav class="docgen-sidebar" id="docgen-sidebar">` — KEEP class, restyle. Inside:
    - `<a class="docgen-sidebar__graph" href="{{base}}/graph">Graph</a>` (was `.docgen-nav-graph`)
    - the `render_nodes` macro rewritten to emit the tree (see §A3)
  - `<main class="docgen-content"><article class="docgen-doc-content">` (the readable
    measure column; `--content-measure`). Keep `{{ body | safe }}`. Wrap the rendered
    markdown in `.docgen-doc-content` so prose rules are scoped (mirrors `.doc-shell`).
    - History link stays: `.docgen-history-link`.
    - Backlinks `<section class="docgen-backlinks">` stays (restyled, §B6).
  - `<aside class="docgen-rail">` — NEW optional right rail (renders only when there is
    something to show; for P7 it holds the backlinks-as-"Referenced by" + build info is
    out of scope since no build timestamp in context — so the rail shows a TOC placeholder
    populated client-side OR is omitted). DECISION for P7: render `<aside class="docgen-rail">`
    ALWAYS in `page.html` and populate "On this page" (TOC) client-side from `h2/h3[id]`
    via a tiny `toc.js` island (optional, see §B9). If TOC island is descoped, the rail
    holds the backlinks block; backlinks then move out of `.docgen-content`. To keep the
    diff minimal and tests stable, KEEP backlinks inside `.docgen-content` and make the
    right rail TOC-only + collapsible; if empty it collapses to zero width via `:empty`.

Grid CSS:
```
.docgen-layout{display:grid;grid-template-columns:var(--left-rail-width) minmax(0,1fr) var(--right-rail-width);
  max-width:1440px;margin:0 auto;align-items:start}
.docgen-content{min-width:0;padding:40px 48px 96px}
.docgen-doc-content{max-width:var(--content-measure);margin:0 auto}
```

Keep the script block at the end EXACTLY as-is (bootstrap, mermaid, components,
alpine defer) plus the new theme-toggle island script + (optional) toc island.

### A3. Sidebar tree macro — port DocTree look (page/graph/history)

Rewrite the shared `render_nodes` macro (it's duplicated in all 3 templates) to emit a
nested, indented, active-aware tree matching `DocTree.svelte`:
```
{% macro render_nodes(nodes, depth) %}
<ul class="docgen-tree" data-depth="{{ depth }}">
  {% for node in nodes %}
    {% if node.kind == "dir" %}
      <li class="docgen-tree__group">
        <details class="docgen-tree__details" open>
          <summary class="docgen-tree__summary"><span class="docgen-tree__chev" aria-hidden="true"></span>{{ node.name }}</summary>
          {{ render_nodes(node.children, depth + 1) }}
        </details>
      </li>
    {% else %}
      <li class="docgen-tree__item{% if node.slug == slug %} is-active{% endif %}">
        <a class="docgen-tree__link" href="{{ base | safe }}/{{ node.slug | safe }}"{% if node.slug == slug %} aria-current="page"{% endif %}>{{ node.title }}</a>
      </li>
    {% endif %}
  {% endfor %}
</ul>
{% endmacro %}
{{ render_nodes(tree, 0) }}
```
- Collapsibles: native `<details>/<summary>` (open by default; no JS needed; keyboard
  accessible). Chevron via CSS rotate on `[open]`.
- Active state: `node.slug == slug` → `.is-active` + `aria-current`. `slug` is already in
  `PageContext`/`HistoryContext`. **For graph.html add `slug => ""`** (no active doc) so
  the comparison is valid (see §A6) — or guard with `{% if slug is defined %}`.
- Indent via `data-depth` / nested `ul` padding + a left hairline guide (port
  `.indent-guides`).

CSS targets (port DocTree.svelte `<style>`): `.docgen-tree`, `.docgen-tree__group`,
`.docgen-tree__summary` (hover `--surface`, chevron `.docgen-tree__chev` rotates),
`.docgen-tree__item`, `.docgen-tree__link` (`--text-dim`, hover `--text`),
`.docgen-tree__item.is-active` (`--surface-hi` bg + 2px `--accent` left bar, link `--text`).

### A4. ThemeToggle island (Alpine) + no-flash pre-paint

NEW asset `assets/docgen/islands/theme-toggle.js` registered as island
**`docgenThemeToggle`** via `window.docgen.island`. Contract (mirrors bootstrap/graph):
```
window.docgen.island('docgenThemeToggle', function (Alpine) {
  Alpine.data('docgenThemeToggle', function () {
    return {
      theme: document.documentElement.getAttribute('data-theme') || 'light',
      set(t){
        this.theme = t;
        document.documentElement.setAttribute('data-theme', t);
        try { localStorage.setItem('docgen-theme', t); } catch (e) {}
      },
      toggle(){ this.set(this.theme === 'dark' ? 'light' : 'dark'); }
    };
  });
});
```
Markup in the topbar (port ThemeToggle.svelte segmented control):
```
<div class="docgen-theme-toggle" x-data="docgenThemeToggle" role="group" aria-label="Theme">
  <button type="button" class="docgen-theme-toggle__btn" :class="{ 'is-active': theme==='light' }"
          @click="set('light')" aria-label="Light theme">☀</button>
  <button type="button" class="docgen-theme-toggle__btn" :class="{ 'is-active': theme==='dark' }"
          @click="set('dark')" aria-label="Dark theme">☾</button>
</div>
```
(Use inline SVG `sun`/`moon` paths, not emoji, for crisp rendering — port from
`Icon.svelte`. Keep them as literal `<svg>` in the template.)

No-flash pre-paint script — inline `<script>` in `<head>` of ALL THREE templates,
BEFORE the `docgen.css` link so the attribute is set before first paint:
```
<script>
(function(){try{
  var s=localStorage.getItem('docgen-theme');
  var t=s||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');
  document.documentElement.setAttribute('data-theme',t);
}catch(e){}})();
</script>
```
This runs synchronously in `<head>`, sets `data-theme` pre-paint → no flash. The Alpine
toggle reads the already-set attribute on init (so its `theme` state matches).

Emit wiring: add `theme-toggle.js` to `core_assets()` (it ships on every page, like
`bootstrap.js`):
```
embed("docgen/islands/theme-toggle.js", "islands/theme-toggle.js", AssetKind::Js),
```
and reference it in all three templates:
`<script src="{{ base | safe }}/islands/theme-toggle.js"></script>` (before alpine defer,
after bootstrap — same slot as graph/mermaid islands).

Sidebar drawer toggle (< 768px): the `.docgen-sidebar-toggle` button. Smallest robust
approach without a new island — a 2nd tiny Alpine `x-data` on the `.docgen-layout` or a
one-line inline handler toggling a `data-sidebar-open` attribute on `<body>` + CSS
transform. DECISION: add `x-data="{ navOpen:false }"` to `<body class="docgen-app">`,
button `@click="navOpen=!navOpen"` `:aria-expanded="navOpen"`, sidebar gets
`:class="{ 'is-open': navOpen }"` and a backdrop `<div class="docgen-sidebar-backdrop"
x-show="navOpen" @click="navOpen=false">`. No new JS file; Alpine already ships.

### A5. graph.html / history.html — share the shell

Apply the SAME topbar + layout grid + sidebar macro to `graph.html` and `history.html`
so all three pages look identical in chrome. Add the no-flash head script + theme-toggle
script + `docgen.css` link order to both. `history.html` currently links NO
`docgen.css`?? — it DOES (line 7). Keep. Add theme-toggle island `<script>` to both
(neither currently loads bootstrap; history loads no JS at all). For history/graph the
theme toggle still needs `bootstrap.js` + `alpine` — graph already has them; **add
`bootstrap.js`, `theme-toggle.js`, and `alpine` to history.html** (it has none today).

### A6. Render-crate context tweak

`graph.html`'s sidebar macro references `slug` for active state but `GraphContext` has no
`slug`. Add `slug => ""` to the `render_graph` `context!` (lib.rs:178) — zero behavior
change, makes the macro uniform. (History already passes `slug`.) No struct field needed
(template-only default), but if cleaner, the macro can use `{% if slug is defined and node.slug == slug %}`.

### Cluster A tests (TDD — write first)

Render crate (`crates/docgen-render/src/lib.rs` tests):
- `page_has_app_shell`: rendered page contains `class="docgen-app"`, `docgen-topbar`,
  `docgen-layout`, `docgen-sidebar`, `docgen-content`.
- `page_has_no_flash_script_in_head`: contains `localStorage.getItem('docgen-theme')` and
  the inline script appears BEFORE the `docgen.css` `<link>` (assert `find` index ordering).
- `page_has_theme_toggle_island`: contains `x-data="docgenThemeToggle"` and
  `islands/theme-toggle.js`.
- `sidebar_marks_active_doc`: with `tree=[Doc{slug:"a"}]` and `slug:"a"`, output contains
  `is-active` + `aria-current="page"` on that link; with `slug:"b"` it does not.
- `sidebar_renders_nested_dir_as_details`: Dir node → `<details` + `<summary`.
- `graph_and_history_share_shell`: both `render_graph`/`render_history` outputs contain
  `docgen-topbar` + `data-theme` + `islands/theme-toggle.js`.
- Keep ALL existing render tests green (title suffix, backlinks, component gating, etc.).

Assets crate (`crates/docgen-assets/src/lib.rs` tests):
- `core_assets_include_theme_toggle_island`: `core_assets()` paths contain
  `islands/theme-toggle.js`, nonempty, `AssetKind::Js`.
- `theme_toggle_island_registers_without_esm`: file contains `docgen.island`,
  `docgenThemeToggle`, `localStorage`, `data-theme`, `prefers-color-scheme` is in the
  pre-paint (in template, not this file) — assert the island has `setAttribute('data-theme'`
  and no `import `.
- `shared_css_defines_theme_tokens`: `docgen.css` contains `:root[data-theme="dark"]`,
  `--accent`, `--bg`, `--text`, `--surface`.
- Update existing `shared_css_retains_p1_classes` / `_has_*` tests if class names changed
  (they won't: `.docgen-search-modal`, `.docgen-diff-line--added/removed`, `.katex-display`,
  `.docgen-mermaid`, `.docgen-graph*` all RETAINED — keep those selectors in the new CSS).

### Cluster A cargo gate
```
cargo test -p docgen-render -p docgen-assets
cargo test
cargo clippy --all-targets
```
Commit: `git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -m "P7-A: app shell, design tokens, light/dark themes, theme-toggle island + no-flash"`

---

## Cluster B — Component styling + responsive + polish

All CSS goes into `assets/docgen/docgen.css` (after the tokens + layout from A), EXCEPT
the callout (its own `components/callout/style.css`) and dev editor (`dev/editor.css`).
Port the matching original Svelte `<style>` blocks, swapping selectors to docgen-rs
classes and ALL hardcoded colors to tokens.

### B1. Prose / doc-content (port `doc-shell.css`)
Scope under `.docgen-doc-content`:
- Headings h1–h4 scale + margins; `h1` shown (do NOT hide first h1 — docgen-rs renders
  the title in body); `h2::before` accent dot; letter-spacing.
- `p` 0 0 14px; `ul/ol` padding-left 20px, `li::marker` `--text-mute`.
- Links: `a{color:--accent;border-bottom:1px solid --accent-line}` hover solidifies;
  `.docgen-wikilink--broken{color:--text-error;text-decoration:underline dotted}` (RETAIN
  the existing broken-link class — it's in current css line 2).
- `hr` hairline; task-list checkboxes (port the masked-SVG checkbox).

### B2. Inline code + syntect code blocks (the dark-mode decision, §"Design decision")
- `.docgen-doc-content :not(pre) > code` → theme-aware `--code-bg`/`--code-border`,
  `font-family:--font-mono; font-size:.86em`.
- `.docgen-doc-content pre` → STABLE light card: `background:var(--code-surface);
  border:1px solid var(--code-border-stable);border-radius:--r-md;padding:14px 16px;
  overflow-x:auto`. `pre code{background:transparent;font:--font-mono/--fs-code}`.
- Top-of-file comment documenting why `pre` is theme-stable (syntect InspiredGitHub).

### B3. Callouts (`components/callout/style.css`)
Re-tokenize the built-in callout (currently hardcoded dark `#0b1220`):
```
.docgen-callout{border-left:3px solid var(--cl, var(--accent));background:var(--accent-soft);
  border:1px solid var(--border);border-left-width:3px;border-radius:0 var(--r-md) var(--r-md) 0;
  padding:14px 16px;margin:18px 0}
.docgen-callout--note{--cl:var(--info);background:var(--info-soft)}
.docgen-callout--warning{--cl:var(--warn);background:var(--warn-soft)}
.docgen-callout--danger{--cl:var(--text-error);background:var(--diff-removed-bg)}
.docgen-callout__title{font-weight:600;font-family:var(--font-mono);text-transform:uppercase;
  font-size:13px;margin:0 0 .25rem;color:var(--cl)}
.docgen-callout__body>:first-child{margin-top:0}.docgen-callout__body>:last-child{margin-bottom:0}
```
(Also port the original `blockquote.doc-callout` look in case raw blockquote callouts
appear — apply to `.docgen-doc-content blockquote` as a generic styled blockquote.)
NOTE: this edits a separate file emitted as `components.css`; its tokens resolve because
`docgen.css` (with `:root` tokens) loads first on every page that has components.

### B4. Tables + blockquotes (port `doc-shell.css` table rules)
`.docgen-doc-content table` bordered + rounded, `th` `--surface` header, separated cells.
Generic `blockquote` (non-callout): left `--accent-line` bar, `--text-dim`, italic off.

### B5. Search modal (port `SearchModal.svelte` to docgen-rs `.docgen-search-*` classes)
Restyle the runtime-built modal in `search.js` markup:
- `.docgen-search-modal` overlay (fixed inset 0, grid place-items start center, padding-top 12vh).
- `.docgen-search-backdrop` `background: color-mix(in srgb, var(--bg) 70%, transparent);
  backdrop-filter: blur(6px)`.
- `.docgen-search-box` → the modal card: `width:min(720px,92vw);max-height:74vh;
  border:1px solid --border-strong;border-radius:--r-lg;background:--bg-elev;box-shadow:--shadow-lg`.
- `.docgen-search-input` borderless inside a padded header row.
- `.docgen-search-results` scroll; `.docgen-search-result` grid row, `.is-selected`
  `background:--surface-hi;color:--text`; inner `.title` weight.
- `.docgen-search-trigger` (now in topbar): port `.search-trigger` (bordered pill,
  `--surface`, `--text-dim`, `<kbd>` chip). Style `kbd` globally: small bordered chip.
RETAIN class names already produced by `search.js` (`.docgen-search-modal/-backdrop/-box/
-input/-results/-result/.is-selected/.title`) — do NOT rename, or JS breaks.

### B6. Backlinks (port RightRail `.backlinks`)
`.docgen-backlinks` top hairline + heading; list items as bordered cards
(`border:1px solid --border;border-radius:--r-sm`) hover `--accent-line`. Keep current
markup (`<section.docgen-backlinks><h2>…<ul><li><a>`).

### B7. Diff timeline / history (port `diff.css` + tokenize current diff rules)
Retarget current `history.html` classes to tokens:
- `.docgen-timeline-bucket h2` → section header, `--border` bottom rule.
- `.docgen-commit` card (`--surface` / `--border` / `--r-md`); `__hash` mono `--text-mute`;
  `__subject` weight 600; `__author`/`__stat` `--text-mute`; `__stat` greens/reds via diff tokens.
- `.docgen-diff-file__path` mono; status pills port `.pill--added/modified/deleted/renamed`
  → map current `.docgen-diff-file--{added,removed,deleted,renamed}` to diff tokens.
- `.docgen-diff-hunk` `--code-surface` stable card (plain text, but keep consistent w/ code).
- `.docgen-diff-line--added` `background:--diff-added-bg;color:--diff-added`;
  `--removed` `--diff-removed-bg/--diff-removed`; `--context` `--text-dim`.
RETAIN `.docgen-diff-line--added/--removed` selectors (asserted in existing tests).

### B8. Graph page frame (retarget current `.docgen-graph*`)
Tokenize the existing graph rules: `.docgen-graph` `--surface`/`--border`/`--r-md`;
`__svg` cursor grab; `__links line` `stroke:--hairline`, `.active` `--accent`;
`__nodes circle` `fill:--accent`/stroke `--bg-elev`, `.active`/`.dimmed`.
`.docgen-graph-page h1` uses prose heading. RETAIN `.docgen-graph`,
`.docgen-graph__nodes circle`, `.docgen-graph__links line` (asserted in tests).

### B9. Math + Mermaid (retarget existing)
- `.katex`/`.katex-display{color:var(--text)}`; `.katex-display` overflow-x auto (keep).
  `.docgen-math-error{color:var(--text-error)}`.
- `.docgen-mermaid` container; port the `.doc-mermaid` gradient surface + `diagram·mermaid`
  `::after` label (`--text-mute`, mono). `.docgen-mermaid__out svg{max-width:100%}`.
  `.docgen-mermaid__error{color:--text-error}`. RETAIN `.docgen-mermaid` (asserted).
- (Optional) `toc.js` island for the right-rail "On this page" — only if time permits;
  if built, register `docgenToc` via `window.docgen.island`, populate from
  `.docgen-doc-content h2[id],h3[id]`, IntersectionObserver active state (port RightRail).
  Add to `core_assets()` + templates if built; otherwise `.docgen-rail:empty` collapses.

### B10. Custom components surface + dev Edit button/editor
- Generic wrapper: ensure any component root reads tokens; add a fallback
  `.docgen-component{}` nicety if present. (Built-in is only `callout`, handled B3.)
- Dev editor (`dev/editor.css`, DEV ONLY — still must look right in both themes since it
  overlays themed pages): tokenize `.docgen-edit-toggle` (bordered pill, `--bg-elev`/
  `--border`/`--text`, `--shadow-md`), `#docgen-editor` (`--bg-elev`/`--border-strong`/
  `--r-lg`/`--shadow-lg`), `.docgen-edit-save` (`--accent` bg, contrast text). Keep it in
  `dev/editor.css` (never in production `docgen.css`). `[x-cloak]` rule stays.

### B11. Responsive breakpoints
- `>1100px`: full 3-col grid.
- `768–1100px`: drop right rail (`--right-rail-width:0` / `.docgen-rail{display:none}`),
  grid → `var(--left-rail-width) minmax(0,1fr)`; content padding reduces.
- `<768px`: sidebar becomes a fixed off-canvas drawer:
  ```
  .docgen-sidebar{position:fixed;inset:var(--topbar-height) auto 0 0;width:min(86vw,320px);
    transform:translateX(-100%);transition:transform .2s;z-index:40;background:var(--bg-elev);
    border-right:1px solid var(--border);overflow-y:auto}
  .docgen-sidebar.is-open{transform:none}
  .docgen-sidebar-backdrop{position:fixed;inset:0;z-index:35;background:rgba(0,0,0,.4)}
  .docgen-layout{grid-template-columns:1fr}
  .docgen-content{padding:24px 18px 64px}
  ```
  Show `.docgen-sidebar-toggle` only `<768px` (`display:none` above). Topbar search trigger
  shrinks (`min-width:0;flex:1`) like the original `@media (max-width:860px)`.
- Tap targets: tree links / toggle buttons ≥ 36px touch height under 768px.
- Content never overflows: `min-width:0` on grid children; `pre`/`table`/`.katex-display`
  scroll-x; long inline code `overflow-wrap:anywhere`.

### B12. Polish
- Topbar `backdrop-filter:blur(8px)` + `background:color-mix(in srgb,var(--bg) 92%,transparent)`.
- Subtle transitions on hover/focus (≤120ms). Custom scrollbars (port `scrollbar.css`).
- `::selection{background:var(--accent-soft)}`. Visible `:focus-visible` rings everywhere.

### Cluster B tests (TDD)
Mostly key-rule presence + retained-selector assertions (visual quality judged by
architect via screenshots):
- `shared_css_has_layout_and_topbar`: `docgen.css` contains `.docgen-topbar`,
  `.docgen-layout`, `.docgen-sidebar`, `.docgen-tree`, `.docgen-content`.
- `shared_css_styles_search_modal_and_results`: contains `.docgen-search-box`,
  `.docgen-search-result`, `.is-selected`.
- `shared_css_code_block_surface_is_theme_stable`: contains `--code-surface` and a
  `.docgen-doc-content pre` rule using it (the syntect decision).
- `shared_css_responsive_has_mobile_drawer`: contains `@media` + `max-width: 768px` +
  `.docgen-sidebar` `.is-open`.
- `shared_css_retains_legacy_selectors`: `.docgen-search-modal`, `.docgen-diff-line--added`,
  `.docgen-diff-line--removed`, `.katex-display`, `.docgen-mermaid`, `.docgen-graph`,
  `.docgen-graph__nodes circle`, `.docgen-graph__links line`, `.docgen-wikilink--broken`
  (so JS-built markup + prior tests + existing pages keep working).
- `callout_css_uses_tokens_not_hardcoded_dark`: `components/callout/style.css` contains
  `var(--` and does NOT contain `#0b1220`.
- (If toc island built) `core_assets_include_toc_island` + `toc_island_registers`.
- Keep ALL prior asset tests green (search gating, mermaid/graph/katex slices, dev assets).

### Cluster B cargo gate
```
cargo test
cargo clippy --all-targets
# manual visual: docgen build a sample site, open page/graph/history in both themes,
# toggle, resize to <768px; screenshot for architect review.
```
Commit: `git -c commit.gpgsign=false -c user.name="docgen-rs overnight" -c user.email="g.maxim.stepanoff@gmail.com" commit -m "P7-B: component styling, callouts, diff/graph/math/mermaid, search modal, responsive drawer, dev editor"`

---

## File-change inventory

Edit:
- `crates/docgen-render/templates/page.html` — full shell restructure (topbar, layout,
  sidebar macro, right rail, no-flash head script, theme-toggle markup + script).
- `crates/docgen-render/templates/graph.html` — same shell + sidebar macro + scripts.
- `crates/docgen-render/templates/history.html` — same shell + sidebar macro + add
  bootstrap/theme-toggle/alpine scripts.
- `crates/docgen-render/src/lib.rs` — `render_graph` adds `slug => ""`; new template tests.
- `crates/docgen-assets/assets/docgen/docgen.css` — REWRITE: tokens + themes + layout +
  every component (the bulk of P7).
- `crates/docgen-assets/components/callout/style.css` — tokenize.
- `crates/docgen-assets/assets/docgen/dev/editor.css` — tokenize.
- `crates/docgen-assets/src/lib.rs` — add `theme-toggle.js` (+ optional `toc.js`) to
  `core_assets()`; new asset tests; update retained-class tests.

New:
- `crates/docgen-assets/assets/docgen/islands/theme-toggle.js` — `docgenThemeToggle` island.
- (Optional) `crates/docgen-assets/assets/docgen/islands/toc.js` — `docgenToc` island.

No changes to: search.js markup CLASS NAMES (restyle by CSS only), bootstrap.js island
contract, graph.js/mermaid.js island contracts, EmitOptions, server inject_dev_html.

---

## BUILD_STATUS — new public API surface

New template classes / semantic hooks (page/graph/history):
`docgen-app`, `docgen-topbar`, `docgen-topbar__brand`, `docgen-brand-mark`,
`docgen-brand-name`, `docgen-topbar__actions`, `docgen-sidebar-toggle`,
`docgen-sidebar-backdrop`, `docgen-layout`, `docgen-sidebar` (retained, restyled),
`docgen-sidebar__graph`, `docgen-tree`, `docgen-tree__group`, `docgen-tree__details`,
`docgen-tree__summary`, `docgen-tree__chev`, `docgen-tree__item` (`.is-active`),
`docgen-tree__link`, `docgen-content` (retained), `docgen-doc-content`, `docgen-rail`,
`docgen-theme-toggle`, `docgen-theme-toggle__btn` (`.is-active`).

New template context defaults: `render_graph` passes `slug => ""` (active-state uniformity).

New island names (registered via `window.docgen.island`):
`docgenThemeToggle` (asset `islands/theme-toggle.js`, in `core_assets`); optional
`docgenToc` (asset `islands/toc.js`). Body-level Alpine inline state `navOpen` for the
mobile sidebar drawer (no new island).

localStorage key: `docgen-theme` (values `"light"`/`"dark"`). `data-theme` on `<html>`.

Design-token names (CSS custom properties, light `:root` + dark `:root[data-theme="dark"]`):
`--bg --bg-elev --bg-soft --surface --surface-hi --border --border-strong --hairline
--text --text-dim --text-mute --text-error --accent --accent-soft --accent-line
--warn --warn-soft --info --info-soft --talk --talk-soft --code-bg --code-border
--code-surface --code-border-stable --diff-added --diff-added-bg --diff-removed
--diff-removed-bg --diff-modified --diff-renamed --font-sans --font-mono
--fs-base --lh --fs-h1 --fs-h2 --fs-h3 --fs-h4 --fs-small --fs-code
--sp-1..--sp-8 --r-xs --r-sm --r-md --r-lg --shadow-sm --shadow-md --shadow-lg
--left-rail-width --right-rail-width --topbar-height --content-measure`.

Syntect-in-dark decision: `.docgen-doc-content pre` (syntect blocks) use a THEME-STABLE
light surface (`--code-surface`/`--code-border-stable`) in both themes; inline code +
diff lines remain token/theme-aware.
