# docgen-rs — Rust rewrite design

**Date:** 2026-06-05
**Status:** Approved (design), not yet implemented
**Target directory:** `~/work/docgen-rs`
**Supersedes:** the SvelteKit-based `@iammaxim/docgen` (at `~/work/docgen`)

## Goal

Rewrite docgen as a **Cargo-only static documentation-site generator**. Two
explicit motivations:

1. **Kill the Node toolchain.** No npm, no Vite, no SvelteKit, no `node_modules`
   — at build time or in distribution. The build is a single Rust binary.
2. **Faster, simpler builds.** Fewer moving parts than the current
   SvelteKit + mdsvex + Vite pipeline.

Explicitly **not** a goal: eliminating JavaScript from the *output*. The
generated site may ship JS. What must die is the npm/Node *toolchain*.

Feature-parity bar: **full parity** with the current site (reader, sidebar,
search, backlinks, graph, diff timeline, math, mermaid, dev editor), reached via
staged phasing.

## Key constraint and its consequences

Svelte's compiler is JS/Node. "No npm ever" therefore means the current
~4,400 LOC of Svelte components **cannot be compiled** and must be re-expressed.
This is the dominant cost of the rewrite — far larger than the markdown pipeline.

The chosen resolution:

- **Build side → Rust.** Markdown, git diff, search index, syntax highlight,
  templating. This half is where Rust most improves on the status quo.
- **Interactive side → vendored JS islands, no WASM.** Components are authored as
  plain `.js`/`.css`, driven by a vendored micro-framework, and **embedded in a
  Cargo crate** so they ship with the binary. npm never appears. WASM was
  considered and rejected — it added `wasm-bindgen` FFI plumbing for
  Mermaid/KaTeX/CodeMirror with no benefit over plain JS here.

## Decisions (locked)

| Decision | Choice | Rationale |
| --- | --- | --- |
| Frontend runtime | **Vendored JS, no WASM** | Simplest build; native-fidelity JS libs; no FFI tax |
| Island framework | **Alpine.js** (vendored) | No-build reactive sprinkles; well-maintained; restores the reactivity lost with Svelte. (petite-vue rejected: lighter but semi-dormant.) |
| Heavy widgets | Imperative vendored libs (CodeMirror, Mermaid, graph renderer) invoked from Alpine `x-init` | Alpine owns reactive shell; libs do imperative work |
| Markdown | **comrak** (CommonMark + GFM) | Mature, fast |
| Syntax highlight | **syntect** | Better than current JS path |
| Git diff timeline | **git2** + port of existing diff logic | Highest-value, lowest-risk port (algorithms carry over) |
| Templating | **minijinja** (or maud) | Drives page render + custom-component templates |
| KaTeX | **Build-time render** via the `katex` crate (runs a JS engine at cargo-build only) | Full fidelity, zero runtime JS, JS engine never ships or touches npm. Runtime `katex.js` is a fallback toggle. |
| Mermaid | Lazy-loaded vendored `mermaid.js` at runtime | No Rust equivalent |
| Asset embedding | **rust-embed / include_dir!** | Crate "hosts the components" |
| Search | Prebuilt JSON index, client-side fuzzy search | No server; static-deploy friendly |
| Distribution | Prebuilt binaries (GitHub Releases + cargo-binstall, optional brew) | Non-Rust authors need no toolchain |

## Crate layout (Cargo workspace)

- **`docgen`** (bin) — CLI: `init`, `build`, `dev`, `serve`. Replaces SvelteKit +
  Vite + `create-docgen` in one binary.
- **`docgen-core`** (lib) — pure content pipeline: markdown, frontmatter,
  wikilinks/backlinks, callouts, slugs, syntax highlight, search index, graph
  model, git diff timeline. Fully unit-testable in Rust.
- **`docgen-assets`** (lib) — the crate that *hosts the components*. Real
  `.js`/`.css` (Alpine + built-in island components + lazy big libs) embedded
  via `rust-embed`/`include_dir!`, emitted at build.
- **`docgen-render`** (lib) — minijinja templating → static HTML + generated
  glue/bootstrap JS.

## Pipeline / data flow

1. Discover `docs/**/*.md`.
2. `comrak` → AST → custom passes: wikilinks (→ links + backlink edges),
   callouts/directives, autolink headings, slugs, code blocks → `syntect`, math
   extraction (→ build-time KaTeX).
3. Build site model: doc tree, backlinks, graph nodes/edges, search index (JSON).
4. Git diff timeline: `git2` → per-doc commit history, block/line diff — a direct
   port of the existing ~1,814 LOC.
5. Render each page → static HTML with island markers + injected JSON data
   (`<script type="application/json">` / `data-*`).
6. Emit embedded assets; lazy-load Mermaid/CodeMirror only where needed.
7. Output `dist/` — fully static, deploy anywhere (parity with adapter-static).

## Islands (full parity)

| Island | Build side (Rust) | Runtime side (JS) |
| --- | --- | --- |
| SearchModal ⌘K | prebuilt JSON index | Alpine state + client fuzzy search |
| DocTree sidebar | tree HTML | Alpine collapse / active state |
| HomeDocGraph | nodes/edges (+ optional precomputed layout) | imperative SVG/canvas + Alpine controls |
| Diff timeline | precomputed diffs | Alpine navigation, mostly static |
| Dev editor (dev only) | dev-server file-write endpoint | vendored CodeMirror via `x-init` |
| Mermaid | mark diagram blocks | lazy `mermaid.js` |
| KaTeX | build-time render to HTML+CSS | none (runtime fallback optional) |

## Custom-component system

Built-in components (callout, graph, diff, …) and project-supplied components use
the **same mechanism** — the built-ins ship in `docgen-assets` via this convention,
consumers add their own in their project.

### Convention (auto-discovered, zero config)

```
my-docs/
  docgen.toml
  docs/
  components/
    callout/   template.html   style.css
    youtube/   template.html
    rating/    template.html   island.js   style.css
```

- **Directive name = folder name.** `template.html` required; `island.js` +
  `style.css` optional.
- docgen reads them at build, renders templates, emits JS/CSS into `dist/`. All
  plain files authored in the consumer project — no npm.
- A project component with the same name **overrides** a built-in.
- `island.js` files are concatenated into one module loaded before
  `Alpine.start()` (lazy-per-page is an optional optimization).

### Markdown directive syntax (comrak `:::` directives)

- **Block:** `:::name{attrs}` … inner markdown … `:::`
- **Leaf/inline:** `:name[label]{attrs}`

### Template context (minijinja)

Each template receives: `attrs` (the `{...}` map), `content` (rendered HTML of
inner markdown, block form), `label` (the `[...]` text, leaf form), `id` (unique
per-instance id for island wiring).

### Example A — static, no JS (callout)

````md
:::callout{type=warning title="Back up first"}
This operation is **destructive**.
:::
````

`components/callout/template.html`:

```html
<aside class="docgen-callout docgen-callout--{{ attrs.type | default('note') }}">
  {% if attrs.title %}<p class="docgen-callout__title">{{ attrs.title }}</p>{% endif %}
  <div class="docgen-callout__body">{{ content }}</div>
</aside>
```

No `island.js` → pure build-time HTML.

### Example B — static leaf directive (youtube)

```md
:youtube[Intro to docgen]{id=dQw4w9WgXcQ}
```

`components/youtube/template.html`:

```html
<figure class="docgen-yt">
  <iframe loading="lazy" title="{{ label }}"
          src="https://www.youtube-nocookie.com/embed/{{ attrs.id }}" allowfullscreen></iframe>
  <figcaption>{{ label }}</figcaption>
</figure>
```

### Example C — interactive island (Alpine + build-time data → runtime)

```md
:::rating{id=page-helpful max=5 label="Was this page helpful?"}
:::
```

`components/rating/template.html`:

```html
<div class="docgen-rating" x-data="docgenRating()" x-init="init($el.dataset)"
     data-id="{{ attrs.id }}" data-max="{{ attrs.max | default(5) }}">
  <p>{{ attrs.label }}</p>
  <template x-for="n in max" :key="n">
    <button @click="vote(n)" :class="{ on: score >= n }" x-text="n"></button>
  </template>
</div>
```

`components/rating/island.js`:

```js
Alpine.data('docgenRating', () => ({
  max: 5, id: '', score: null,
  init(ds) { this.id = ds.id; this.max = +ds.max; this.score = +localStorage.getItem('rating:'+ds.id) || null; },
  vote(n) { this.score = n; localStorage.setItem('rating:'+this.id, n); }
}));
```

**Web-component flavor** works identically — `island.js` calls
`customElements.define('docgen-foo', …)` and the template uses `<docgen-foo>`.
Choose per component. Scoped styles via Shadow DOM (web component) or a
`docgen-c-<name>` class convention (Alpine).

## Dev story

`docgen dev` = `axum` + `notify` file watcher + SSE live-reload. Markdown change →
fast partial rebuild → reload. In dev, island JS is served **from disk** (not
embedded) for instant iteration; embedding happens only for release builds. The
editor's file-write endpoint lives in this dev server (cleaner than the current
Vite plugin).

**Known DX regression:** the Rust/asset dev loop is slower than Vite HMR. Mitigated
by partial rebuilds and disk-served island JS, but expect a slower inner loop than
SvelteKit.

## Distribution

- Prebuilt binaries via GitHub Releases + `cargo-binstall` (+ optional brew tap).
  Most users will not `cargo install` from source.
- `docgen init` replaces `create-docgen` scaffolding.
- Config moves `svelte.config.js` → `docgen.toml` (theme, feature toggles, base
  path, math/mermaid on/off, component dirs).

## Migration risks / gotchas

1. **`.svx` / inline-Svelte content (the one parity asterisk).** mdsvex let authors
   embed Svelte components in markdown. With no Svelte runtime this is unsupported.
   Such content must be rewritten as directives + custom components (the system
   above). Some projects rely on this; each component is rewritten once as a
   `components/<name>/` folder. **Not P1-critical** — the directive/component system
   covers it, and the project owner will do the content rewrite.
2. **Testing gap for islands.** The current `node --test` JS tests disappear with
   npm. Rust-side logic stays fully tested; interactive components rely on manual /
   browser verification (a browser test runner would drag Node back in). Decided
   consciously: Rust unit tests + manual UI verification.
3. **Vendored big libs are heavy.** Mermaid / CodeMirror must load lazily — only on
   pages/islands that need them — or every page pays.
4. **KaTeX build-time dependency.** The `katex` crate runs a JS engine during cargo
   build. It never ships and never touches npm, but it is a build-time cost/edge to
   be aware of.
5. **SSG ecosystem maturity.** Rust SSG/templating is solid, but expect fewer
   ready-made answers than the SvelteKit ecosystem.

## Suggested phasing (lands full parity)

- **P0** core SSG: markdown + tree + templating + static output.
- **P1** search + syntax highlight + wikilinks/backlinks.
- **P2** git diff timeline (port of existing logic) — high value.
- **P3** math (build-time KaTeX) + mermaid.
- **P4** graph view.
- **P5** dev server + editor.
- **P6** `init`/scaffold + binary distribution + custom-component docs.

## Effort read

Build core (P0–P2) is the easy, high-payoff part: a couple of focused weeks, and
the diff/git code gets notably nicer in Rust. The long pole is the interactive UI
(P3–P5) re-expressed as Alpine/vendored-JS islands plus the custom-component system
— multi-week, dominated by the graph and the dev editor.
