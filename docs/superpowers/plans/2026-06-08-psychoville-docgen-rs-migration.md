# Psychoville docs → docgen-rs Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite `~/work/Psychoville/docs` from the Svelte/`.svx` `docgen` to docgen-rs — `.svx`→`.md`, Svelte components → triple-colon directives — by (A) adding a reusable `:include` directive + partial-file exclusion to docgen-rs, then (B) adding Psychoville's `docgen.toml` + `rustref` component and (C) running a one-shot migration script over the tree, in place.

**Architecture:** `:include{src=...}` is a *built-in* directive handled in `directivepass::substitute` (not a project component) because it transcludes and recursively renders a file. Partials are resolved **in-memory**: discovered `.md` files whose basename starts with `_` are split out of the page set into a `Partials` map (docs-relative path → frontmatter-stripped body) and rendered on demand via the existing recursive `render_block_markdown` pipeline, with a cycle guard. The `rustref` component is pure MiniJinja; its Rust-logo icon ships as a single CSS `background-image` data-URI (avoids DOM id collisions from inlining the SVG 660×).

**Tech Stack:** Rust (comrak, minijinja, walkdir), MiniJinja templates, Python 3 (one-shot migration script).

---

## File Structure

**docgen-rs (`~/work/docgen-rs`) — Part A:**
- Modify `crates/docgen-core/src/pipeline.rs` — `Partials` type, `is_partial_rel`, `partition_partials`, `resolve_include_key`, `resolve_include_src`; thread partials through `render_block_markdown` / `render_doc` / `render_docs`.
- Modify `crates/docgen-core/src/directivepass.rs` — `substitute` gains a `resolve_include` arg + `include` special-case; make `error_span` `pub(crate)`.
- Modify `crates/docgen-build/src/lib.rs` — partition partials, pass map to `render_docs`.
- Modify `crates/docgen-server/src/handlers.rs` — thread partials into the editor-preview `render_doc` call.
- Modify `crates/docgen-core/src/assemble.rs` — pass empty partials.
- Modify `crates/docgen-build/tests/components.rs` — add an include integration test.
- Modify `fixtures/site-basic/docs/directives.md` + create `fixtures/site-basic/docs/_partial.inc.md` — fixture coverage.
- Modify `README.md` — document `:include` + partials.

**Psychoville (`~/work/Psychoville`) — Parts B & C:**
- Create `docgen.toml`
- Create `components/rustref/template.html`, `components/rustref/style.css`
- Create `tools/migrate_svx.py`
- Modify (in place) all `docs/**/*.svx` → `docs/**/*.md`

---

## PART A — docgen-rs: `:include` directive + partial exclusion

### Task A1: Pure helpers — partial detection, partition, path resolution

**Files:**
- Modify: `crates/docgen-core/src/pipeline.rs`

- [ ] **Step 1: Write failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `pipeline.rs`:

```rust
#[test]
fn is_partial_rel_detects_underscore_basename() {
    assert!(is_partial_rel("dev/server/_systems.gen.md"));
    assert!(is_partial_rel("_root.md"));
    assert!(!is_partial_rel("dev/server/index.md"));
    assert!(!is_partial_rel("dev/_dir/page.md")); // only the *basename* counts
}

#[test]
fn partition_partials_splits_pages_and_strips_frontmatter() {
    let raws = vec![
        RawDoc { rel_path: "a/index.md".into(), raw: "# Page\n".into() },
        RawDoc {
            rel_path: "a/_inc.md".into(),
            raw: "---\ntitle: x\n---\n## Inc\n".into(),
        },
    ];
    let (pages, partials) = partition_partials(raws);
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].rel_path, "a/index.md");
    assert_eq!(partials.get("a/_inc.md").map(String::as_str), Some("## Inc\n"));
}

#[test]
fn resolve_include_key_normalizes_relative_and_absolute() {
    assert_eq!(resolve_include_key("dev/server", "./_s.gen.md").as_deref(), Some("dev/server/_s.gen.md"));
    assert_eq!(resolve_include_key("dev/server", "../_top.md").as_deref(), Some("dev/_top.md"));
    assert_eq!(resolve_include_key("dev/server", "/root/_x.md").as_deref(), Some("root/_x.md"));
    assert_eq!(resolve_include_key("", "_x.md").as_deref(), Some("_x.md"));
    assert_eq!(resolve_include_key("dev", "../../escape.md"), None); // escapes docs root
}
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `cargo test -p docgen-core is_partial_rel partition_partials resolve_include_key 2>&1 | tail -20`
Expected: FAIL — `cannot find function ...` (not yet defined).

- [ ] **Step 3: Implement the helpers**

Near the top of `pipeline.rs` (after the `use` block), add:

```rust
/// Docs-relative path → frontmatter-stripped body, for `:include` targets.
pub type Partials = std::collections::BTreeMap<String, String>;

/// A doc is an include-only *partial* (never its own page) when its filename
/// starts with `_`. Only the basename matters — a `_dir/` directory does not
/// hide the pages inside it.
pub fn is_partial_rel(rel_path: &str) -> bool {
    rel_path
        .rsplit('/')
        .next()
        .map(|name| name.starts_with('_'))
        .unwrap_or(false)
}

/// Split discovered raw docs into rendered pages and the include-only partial
/// map (keyed by docs-relative path, frontmatter stripped).
pub fn partition_partials(raws: Vec<RawDoc>) -> (Vec<RawDoc>, Partials) {
    let mut pages = Vec::new();
    let mut partials = Partials::new();
    for raw in raws {
        if is_partial_rel(&raw.rel_path) {
            let body = parse_frontmatter(&raw.raw).body;
            partials.insert(raw.rel_path, body);
        } else {
            pages.push(raw);
        }
    }
    (pages, partials)
}

/// Resolve a relative include `src` against the docs-relative directory
/// `base_dir` into a normalized docs-relative key (no `./`, `..` collapsed). A
/// leading `/` is treated as docs-root-absolute. Returns `None` if the path
/// escapes above the docs root.
pub fn resolve_include_key(base_dir: &str, src: &str) -> Option<String> {
    let src = src.trim();
    let combined = if let Some(rest) = src.strip_prefix('/') {
        rest.to_string()
    } else if base_dir.is_empty() {
        src.to_string()
    } else {
        format!("{base_dir}/{src}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}
```

- [ ] **Step 4: Run tests, verify they pass**

Run: `cargo test -p docgen-core is_partial_rel partition_partials resolve_include_key 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/docgen-core/src/pipeline.rs
git commit -m "feat(core): partial-file helpers + include path resolution"
```

---

### Task A2: Wire the `:include` directive end-to-end

This task changes `substitute`'s signature, so all call sites update together to keep the tree compiling. Driven by a build-level integration test.

**Files:**
- Modify: `crates/docgen-core/src/directivepass.rs`
- Modify: `crates/docgen-core/src/pipeline.rs`
- Modify: `crates/docgen-core/src/assemble.rs`
- Modify: `crates/docgen-build/src/lib.rs`
- Modify: `crates/docgen-server/src/handlers.rs`
- Test: `crates/docgen-build/tests/components.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `crates/docgen-build/tests/components.rs` (reuse the file's existing build harness — match its existing helper for building a temp project; the snippet below assumes a `build_project(files: &[(&str, &str)]) -> PathBuf` helper like the other tests in that file. If the helper differs, adapt the call but keep the assertions):

```rust
#[test]
fn include_directive_transcludes_partial_and_excludes_it_as_page() {
    let dist = build_project(&[
        ("docs/guide/index.md", "# Guide\n\n:include{src=\"./_facts.gen.md\"}\n"),
        ("docs/guide/_facts.gen.md", "## Facts\n\n- alpha\n- beta\n"),
    ]);
    // The partial's content is spliced into the host page...
    let host = std::fs::read_to_string(dist.join("guide/index/index.html")).unwrap();
    assert!(host.contains("Facts"), "partial heading missing: {host}");
    assert!(host.contains("alpha"), "partial list missing");
    // ...and the partial never becomes its own page.
    assert!(!dist.join("guide/_facts.gen/index.html").exists(), "partial leaked as a page");
}

#[test]
fn include_missing_src_degrades_to_error_span() {
    let dist = build_project(&[
        ("docs/index.md", "# Home\n\n:include{src=\"./_nope.md\"}\n"),
    ]);
    let html = std::fs::read_to_string(dist.join("index/index.html")).unwrap();
    assert!(html.contains("docgen-directive-error"), "expected inert error span: {html}");
    // Build did not panic / fail — reaching this line means it succeeded.
}
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cargo test -p docgen-build include_directive_transcludes 2>&1 | tail -25`
Expected: FAIL — partial currently renders as its own page and `:include` is an unknown directive (error span where content is expected).

- [ ] **Step 3: `directivepass.rs` — add `resolve_include` arg + include branch**

Make `error_span` reusable: change its signature line from `fn error_span(` to `pub(crate) fn error_span(`.

Change the `substitute` signature to add a final parameter:

```rust
pub fn substitute(
    html: &str,
    instances: &[DirectiveInstance],
    registry: &docgen_components::Registry,
    render_inner: &dyn Fn(&str) -> String,
    resolve_include: &dyn Fn(&str) -> String,
) -> (String, std::collections::BTreeSet<String>) {
```

Inside the `for (idx, inst) in instances.iter().enumerate()` loop, **before** the `let rendered = match registry.get(&inst.name) { ... }`, insert the include special-case:

```rust
        // `:include{src=...}` is a built-in, file-transcluding directive — not a
        // registry component. It renders the resolved partial's markdown here.
        if inst.name == "include" {
            let src = inst.attrs.get("src").map(String::as_str).unwrap_or("");
            let rendered = if src.is_empty() {
                error_span("include", "missing `src`")
            } else {
                resolve_include(src)
            };
            out = out.replace(&sentinel(idx), &rendered);
            continue;
        }
```

Update the two existing `substitute(...)` calls inside this file's `#[cfg(test)] mod substitute_tests` to pass a no-op resolver as the new last arg: `&|_src: &str| String::new()`.

- [ ] **Step 4: `pipeline.rs` — thread partials + build the resolver**

Replace `render_block_markdown` (currently lines ~93–117) with the partials-aware version:

```rust
pub fn render_block_markdown(
    md: &str,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    slugs: &SlugSet,
    partials: &Partials,
    base_dir: &str,
    stack: &[String],
) -> String {
    let (rewritten, instances) = crate::directivepass::extract(md);
    let options = comrak_options();
    let arena = Arena::new();
    let root = parse_document(&arena, &rewritten, &options);
    let _pass = transform_wikilinks(root, &arena, slugs, &config.base);
    if config.features.math {
        crate::mathpass::transform_math(root);
    }
    if config.features.mermaid {
        crate::mermaidpass::transform_mermaid(root);
    }
    let inner_html = format_ast(root, &options);
    let render_inner =
        |m: &str| render_block_markdown(m, config, registry, slugs, partials, base_dir, stack);
    let resolve_include =
        |src: &str| resolve_include_src(src, base_dir, partials, stack, config, registry, slugs);
    let (out, _used) = crate::directivepass::substitute(
        &inner_html,
        &instances,
        registry,
        &render_inner,
        &resolve_include,
    );
    out
}

/// Resolve `:include{src}` against `base_dir`, render the partial's body through
/// the recursive pipeline. Missing target or an include cycle degrades to an
/// inert error span (never panics). `stack` holds the include keys currently on
/// the rendering path, for cycle detection.
fn resolve_include_src(
    src: &str,
    base_dir: &str,
    partials: &Partials,
    stack: &[String],
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    slugs: &SlugSet,
) -> String {
    let key = match resolve_include_key(base_dir, src) {
        Some(k) => k,
        None => return crate::directivepass::error_span("include", "src escapes docs root"),
    };
    if stack.iter().any(|s| s == &key) {
        return crate::directivepass::error_span("include", "include cycle");
    }
    let Some(body) = partials.get(&key) else {
        return crate::directivepass::error_span("include", "missing `src`");
    };
    let mut next = stack.to_vec();
    next.push(key.clone());
    let child_dir = key.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    render_block_markdown(body, config, registry, slugs, partials, child_dir, &next)
}
```

In `render_doc`, change the signature to add `partials: &Partials` (after `slugs`), and replace the directive post-pass block (currently lines ~184–189) with:

```rust
    // Directive post-pass: substitute each sentinel with the component's
    // rendered HTML; block inner content + `:include` partials are rendered by
    // the full recursive pipeline. `used` drives per-page island/style gating.
    let base_dir = p.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let stack: Vec<String> = Vec::new();
    let render_inner =
        |m: &str| render_block_markdown(m, config, registry, slugs, partials, base_dir, &stack);
    let resolve_include =
        |src: &str| resolve_include_src(src, base_dir, partials, &stack, config, registry, slugs);
    let (body_html, used) = crate::directivepass::substitute(
        &formatted,
        &instances,
        registry,
        &render_inner,
        &resolve_include,
    );
```

Change `render_docs`'s signature to take `partials: &Partials` (after `prepared`):

```rust
pub fn render_docs(
    prepared: Vec<PreparedDoc>,
    partials: &Partials,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
) -> SiteBuild {
```

Find the `render_doc(p, config, registry, &slugs)` call inside `render_docs` (~line 227) and change it to `render_doc(p, config, registry, &slugs, partials)`.

Update the in-file `#[cfg(test)]` callers: every `render_docs(prepared, &cfg, &reg)` becomes `render_docs(prepared, &Partials::new(), &cfg, &reg)`, and every direct `render_doc(p, &cfg, &reg, &slugs)` becomes `render_doc(p, &cfg, &reg, &slugs, &Partials::new())`. (There are several around lines 227–390 — update each.)

- [ ] **Step 5: `assemble.rs` — pass empty partials**

In `assemble()`, change `render_docs(vec![prepared], &docgen_config::SiteConfig::default(), &docgen_components::Registry::empty())` to insert `&crate::pipeline::Partials::new(),` as the second argument.

- [ ] **Step 6: `docgen-build/src/lib.rs` — partition partials**

Replace (lines ~122–126):

```rust
    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Two-pass: prepare all docs, then render with full slug knowledge.
    let prepared: Vec<_> = raws.into_iter().map(prepare).collect();
```

with:

```rust
    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Split include-only partials (`_*.md`) out of the page set; they render
    // only where a `:include` transcludes them, never as standalone pages.
    let (pages, partials) = docgen_core::pipeline::partition_partials(raws);
    // Two-pass: prepare all pages, then render with full slug knowledge.
    let prepared: Vec<_> = pages.into_iter().map(prepare).collect();
```

Change the render call (line ~146) `let site = render_docs(prepared, &config, &registry);` to:

```rust
    let site = render_docs(prepared, &partials, &config, &registry);
```

- [ ] **Step 7: `docgen-server/src/handlers.rs` — thread partials into preview**

At the `render_doc(&prepared, &config, &registry, &slugs)` call (~line 315), build a partials map from the same discovered docs used to compute `slugs` in that handler and pass it. If the handler already has the discovered raw docs in scope as `raws`/`docs`, add just above the call:

```rust
    let (_pages, partials) = docgen_core::pipeline::partition_partials(raws.clone());
```

and change the call to `render_doc(&prepared, &config, &registry, &slugs, &partials)`. If the surrounding code shape differs, the minimal correct fallback is `&docgen_core::pipeline::Partials::new()` (includes simply won't resolve in live preview) — prefer the real map when the discovered docs are in scope.

- [ ] **Step 8: Build + run the whole core/build test suite**

Run: `cargo test -p docgen-core -p docgen-build 2>&1 | tail -30`
Expected: PASS, including the two new `include_directive_*` tests.

- [ ] **Step 9: Commit**

```bash
git add crates/docgen-core crates/docgen-build crates/docgen-server
git commit -m "feat(core): :include directive transcludes _partials, excluded from pages"
```

---

### Task A3: Fixture + docs

**Files:**
- Create: `fixtures/site-basic/docs/_facts.inc.md`
- Modify: `fixtures/site-basic/docs/directives.md`
- Modify: `README.md`

- [ ] **Step 1: Add the partial fixture**

Create `fixtures/site-basic/docs/_facts.inc.md`:

```markdown
## Included facts

This paragraph lives in a `_`-prefixed **partial** — it never gets its own page,
but `:include` transcludes it here.
```

- [ ] **Step 2: Reference it from `directives.md`**

Append to `fixtures/site-basic/docs/directives.md`:

```markdown

## Includes

The `:include` directive transcludes another file's markdown, resolved relative
to this doc. Files whose name starts with `_` are include-only partials and are
never rendered as their own page.

:include{src="./_facts.inc.md"}
```

- [ ] **Step 3: Document in README**

In `README.md`, under the directives/components description, add a short paragraph:

```markdown
### Includes & partials

`:include{src="./_part.md"}` transcludes another markdown file (resolved relative
to the including doc) and renders it through the full pipeline. Any `.md` file
whose basename starts with `_` is an *include-only partial*: it is excluded from
page discovery (no standalone page, sidebar entry, or search result) but remains
a valid `:include` target. Missing targets and include cycles degrade to an
inert error span; the build never fails on a bad include.
```

- [ ] **Step 4: Build the fixture, verify the include renders and the partial has no page**

Run:
```bash
cargo run -p docgen -- build fixtures/site-basic >/dev/null 2>&1 && \
  grep -q "Included facts" fixtures/site-basic/dist/directives/index.html && echo "INCLUDE_OK" && \
  test ! -e fixtures/site-basic/dist/_facts.inc/index.html && echo "NO_PARTIAL_PAGE_OK"
```
Expected: prints `INCLUDE_OK` and `NO_PARTIAL_PAGE_OK`.

- [ ] **Step 5: Commit**

```bash
git add fixtures/site-basic/docs/_facts.inc.md fixtures/site-basic/docs/directives.md README.md
git commit -m "docs(include): fixture + README for :include and _partials"
```

---

## PART B — Psychoville: config + `rustref` component

> All Part B/C work happens in `~/work/Psychoville`. Use a feature branch there:
> `cd ~/work/Psychoville && git checkout -b migrate/docgen-rs` (before Task B1).

### Task B1: `docgen.toml`

**Files:**
- Create: `~/work/Psychoville/docgen.toml`

- [ ] **Step 1: Write the config**

Create `~/work/Psychoville/docgen.toml`:

```toml
title = "Psychoville Docs"
base = ""

[features]
graph = true
math = true
mermaid = true
search = true

[components]
dir = "components"
```

- [ ] **Step 2: Commit**

```bash
cd ~/work/Psychoville
git add docgen.toml
git commit -m "build(docs): docgen-rs project config"
```

---

### Task B2: `rustref` component

**Files:**
- Create: `~/work/Psychoville/components/rustref/template.html`
- Create: `~/work/Psychoville/components/rustref/style.css`

- [ ] **Step 1: Write the template**

Create `~/work/Psychoville/components/rustref/template.html` (MiniJinja; `label` is the `::`-path, `attrs.href` the URL; icon comes from CSS so no inline SVG):

```html
<a class="rust-ref rust-ref-link" href="{{ attrs.href | default('/docs/dev/rust-linking') }}"{% if '/rustdoc/' in (attrs.href | default('')) %} rel="external"{% endif %} title="Rust path (links to API documentation)"><span class="rust-ref__icon" aria-hidden="true"></span><span class="rust-ref__path">{{ label }}</span></a>
```

- [ ] **Step 2: Generate `style.css` (ported styles + icon data-URI)**

The icon is the real `docgen/static/icons/rust.svg`, embedded once as a CSS
`background-image` data-URI (no per-instance DOM ids → no collisions). Generate
the file with this command (run from `~/work/Psychoville`):

```bash
cd ~/work/Psychoville
python3 - <<'PY'
import urllib.parse, pathlib
svg = pathlib.Path("docgen/static/icons/rust.svg").read_text()
svg = " ".join(svg.split())  # collapse whitespace to one line
uri = "data:image/svg+xml," + urllib.parse.quote(svg, safe="")
css = '''.rust-ref {
	display: inline-flex;
	align-items: center;
	gap: 5px;
	max-width: 100%;
	min-width: 0;
	padding: 1px 7px 1px 5px;
	border: 1px solid var(--code-border);
	border-radius: 4px;
	background: var(--code-bg);
	color: var(--text);
	font-family: var(--font-mono);
	font-size: 0.85em;
	text-decoration: none;
	white-space: normal;
	vertical-align: middle;
	transition: border-color 0.15s, color 0.15s;
}
.rust-ref:hover {
	border-color: var(--accent-line);
	color: var(--accent);
}
.rust-ref__path {
	min-width: 0;
	overflow-wrap: anywhere;
	word-break: normal;
}
.rust-ref__icon {
	display: inline-block;
	width: 14px;
	height: 14px;
	flex-shrink: 0;
	background: center / contain no-repeat url("%s");
}
''' % uri
out = pathlib.Path("components/rustref/style.css")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(css)
print("wrote", out, len(css), "bytes")
PY
```
Expected: prints `wrote components/rustref/style.css <n> bytes`.

- [ ] **Step 3: Smoke-test the component renders (temporary fixture build)**

Run from `~/work/Psychoville`:
```bash
mkdir -p docs && cat > docs/_rustref_probe.md <<'EOF'

EOF
printf '# Probe\n\n:rustref[server::http::websocket]{href="/rustdoc/server/http/websocket/index.html"}\n' > docs/__rustref_probe.md
cargo run --manifest-path ~/work/docgen-rs/Cargo.toml -p docgen -- build ~/work/Psychoville >/dev/null 2>&1
grep -q 'class="rust-ref' ~/work/Psychoville/dist/__rustref_probe/index.html && echo "RUSTREF_RENDERS"
grep -q '/rustdoc/server/http/websocket/index.html' ~/work/Psychoville/dist/__rustref_probe/index.html && echo "HREF_OK"
rm -f ~/work/Psychoville/docs/__rustref_probe.md ~/work/Psychoville/docs/_rustref_probe.md
```
Expected: prints `RUSTREF_RENDERS` and `HREF_OK`. (The probe doc is removed after; it is not part of the real docs.)

- [ ] **Step 4: Commit**

```bash
cd ~/work/Psychoville
git add components/rustref
git commit -m "feat(docs): rustref custom component for docgen-rs"
```

---

## PART C — Psychoville: content migration

### Task C1: Migration script with self-tests

**Files:**
- Create: `~/work/Psychoville/tools/migrate_svx.py`

- [ ] **Step 1: Write the script (pure transform + self-tests + driver)**

Create `~/work/Psychoville/tools/migrate_svx.py`:

```python
#!/usr/bin/env python3
"""One-shot migration: Psychoville `.svx` (Svelte/mdsvex) -> docgen-rs `.md`.

Transforms each file body in place and renames `.svx` -> `.md`:
  * strip the leading <script>...</script> import block
  * <RustRef ref="X" href={rustdoc('P')} /> -> :rustref[X]{href="/rustdoc/P"}
  * <RustRef ref="X" href="/lit" />        -> :rustref[X]{href="/lit"}
  * <RustRef ref="X" />                     -> :rustref[X]{}
  * <Name /> (a `*.gen.svx` import)         -> :include{src="./_name.gen.md"}
  * ](/docs/PATH) internal links           -> ](/PATH)

Run `python3 tools/migrate_svx.py --selftest` to run unit checks,
`python3 tools/migrate_svx.py docs` to migrate the tree.
"""
import re
import sys
import pathlib

SCRIPT_RE = re.compile(r"<script\b[^>]*>.*?</script>\s*", re.DOTALL)
IMPORT_RE = re.compile(r"""import\s+(\w+)\s+from\s+['"]([^'"]+)['"]""")
RUSTREF_RE = re.compile(r"<RustRef\b(.*?)/>", re.DOTALL)
REF_ATTR_RE = re.compile(r"""ref\s*=\s*["']([^"']*)["']""")
HREF_RUSTDOC_RE = re.compile(r"""href\s*=\s*\{\s*rustdoc\(\s*['"]([^'"]+)['"]\s*\)\s*\}""")
HREF_LITERAL_RE = re.compile(r"""href\s*=\s*["']([^"']+)["']""")
SCAFFOLD_RE = re.compile(r"<!--\s*docs-gen-scaffold:[^>]*-->\s*")
DOCS_LINK_RE = re.compile(r"\]\(/docs(/[^)]*)?\)")

def parse_imports(text):
    """Return {ComponentName: importPath} from the (first) <script> block."""
    m = re.search(r"<script\b[^>]*>(.*?)</script>", text, re.DOTALL)
    if not m:
        return {}
    return {name: path for name, path in IMPORT_RE.findall(m.group(1))}

def convert_rustref(match):
    blob = match.group(1)
    ref_m = REF_ATTR_RE.search(blob)
    ref = ref_m.group(1) if ref_m else ""
    rd = HREF_RUSTDOC_RE.search(blob)
    if rd:
        return f':rustref[{ref}]{{href="/rustdoc/{rd.group(1)}"}}'
    lit = HREF_LITERAL_RE.search(blob)
    if lit:
        return f':rustref[{ref}]{{href="{lit.group(1)}"}}'
    return f':rustref[{ref}]{{}}'

def transform(text):
    """Pure body transform. Returns (new_text, warnings)."""
    warnings = []
    imports = parse_imports(text)
    text = SCRIPT_RE.sub("", text, count=1)
    text = SCAFFOLD_RE.sub("", text)
    text = RUSTREF_RE.sub(convert_rustref, text)
    # Transclusion components: every import whose path ends in `.gen.svx`.
    for name, path in imports.items():
        if name == "RustRef":
            continue
        if path.endswith(".gen.svx"):
            md_path = path[:-len(".svx")] + ".md"
            tag_re = re.compile(r"<%s\s*/>" % re.escape(name))
            text = tag_re.sub(f':include{{src="{md_path}"}}', text)
        else:
            # Unknown/unused component import (e.g. Badge/SrcEmbed) — only warn
            # if actually used in the body.
            if re.search(r"<%s[\s/>]" % re.escape(name), text):
                warnings.append(f"unhandled component <{name}> from {path}")
    text = DOCS_LINK_RE.sub(lambda m: "](" + (m.group(1) or "/") + ")", text)
    # Straggler check: any remaining capitalized JSX-ish tag.
    for m in re.finditer(r"<([A-Z][A-Za-z0-9]*)\b", text):
        warnings.append(f"leftover tag <{m.group(1)}>")
    return text, warnings

def selftest():
    t = '''---
title: x
---
<!-- docs-gen-scaffold:v1 -->
<script>
  import Systems from './_systems.gen.svx';
  import RustRef from '$lib/doc-components/RustRef.svelte';
</script>

# Title

See <RustRef ref="server::http" href={rustdoc('server/http/index.html')} />.
And <RustRef ref="protocol" />.
And [testing](/docs/testing).

<Systems />
'''
    out, warns = transform(t)
    assert "<script" not in out, out
    assert ":rustref[server::http]{href=\"/rustdoc/server/http/index.html\"}" in out, out
    assert ":rustref[protocol]{}" in out, out
    assert ':include{src="./_systems.gen.md"}' in out, out
    assert "](/testing)" in out, out
    assert "docs-gen-scaffold" not in out, out
    assert warns == [], warns
    # Multiline RustRef
    t2 = '<RustRef\n  ref="a::b"\n  href={rustdoc(\'a/b/index.html\')}\n/>'
    out2, _ = transform(t2)
    assert out2 == ':rustref[a::b]{href="/rustdoc/a/b/index.html"}', out2
    print("selftest OK")

def migrate(root):
    root = pathlib.Path(root)
    total, warned = 0, 0
    for svx in sorted(root.rglob("*.svx")):
        text = svx.read_text()
        new_text, warns = transform(text)
        md = svx.with_suffix(".md")
        md.write_text(new_text)
        svx.unlink()
        total += 1
        for w in warns:
            warned += 1
            print(f"WARN {svx}: {w}")
    print(f"migrated {total} files ({warned} warnings)")

if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--selftest":
        selftest()
    elif len(sys.argv) >= 2:
        migrate(sys.argv[1])
    else:
        print("usage: migrate_svx.py [--selftest | <docs-dir>]")
        sys.exit(1)
```

- [ ] **Step 2: Run the self-test**

Run: `cd ~/work/Psychoville && python3 tools/migrate_svx.py --selftest`
Expected: prints `selftest OK`.

- [ ] **Step 3: Commit**

```bash
cd ~/work/Psychoville
git add tools/migrate_svx.py
git commit -m "tools: svx->md migration script with self-tests"
```

---

### Task C2: Dry-run on representative files

**Files:** none (verification only)

- [ ] **Step 1: Snapshot three representative files before migrating**

Run (from `~/work/Psychoville`):
```bash
python3 - <<'PY'
import sys; sys.path.insert(0, "tools")
from migrate_svx import transform
import pathlib
samples = [
    "docs/flows.svx",                         # RustRef + script
    "docs/dev/server.svx",                    # the four includes + RustRef
    "docs/dev/npc-ai/social.svx",             # mermaid + RustRef
]
for s in samples:
    p = pathlib.Path(s)
    if not p.exists():
        print("MISSING", s); continue
    out, warns = transform(p.read_text())
    print("="*70, "\n", s, "warnings:", warns)
    # show the first 12 non-empty body lines after frontmatter
    body = out.split("---", 2)[-1].strip().splitlines()
    print("\n".join(body[:12]))
PY
```
Expected: no `<script>` / `<RustRef` / `<Systems>` left; `:rustref[...]` and `:include{src="./_*.gen.md"}` present; `warnings: []` for each. **If any file shows leftover tags or warnings, fix `transform()` in Task C1 before continuing.**

- [ ] **Step 2: Verify the include target naming matches actual partial files**

Run:
```bash
cd ~/work/Psychoville
ls docs/dev/*.gen.svx 2>/dev/null | head
grep -rl "gen.svx" docs/dev/server.svx
```
Expected: the partials referenced (`_systems.gen.svx`, etc.) exist as siblings; after migration they will be renamed to `.gen.md` (Task C3 migrates all `.svx`, including `.gen.svx`). Confirm the `import ... from './_X.gen.svx'` paths in `server.svx` point at real sibling files.

---

### Task C3: Run the migration

**Files:** Modifies all `~/work/Psychoville/docs/**/*.svx` → `.md` (in place)

- [ ] **Step 1: Migrate the whole tree**

Run: `cd ~/work/Psychoville && python3 tools/migrate_svx.py docs`
Expected: `migrated 339 files (0 warnings)`. **If warnings appear, stop and inspect** — fix `transform()` (Task C1), `git checkout -- docs && git clean -fd docs` to restore, and re-run.

- [ ] **Step 2: Verify no `.svx` and no Svelte residue remain**

Run:
```bash
cd ~/work/Psychoville
echo "svx left: $(find docs -name '*.svx' | wc -l)"
echo "script tags: $(grep -rl '<script' docs --include='*.md' | wc -l)"
echo "RustRef tags: $(grep -rl '<RustRef' docs --include='*.md' | wc -l)"
echo "bare includes: $(grep -rl '<Systems\|<Deps\|<Symbols\|<Files' docs --include='*.md' | wc -l)"
```
Expected: all four counts are `0`.

- [ ] **Step 3: Commit the migrated content**

```bash
cd ~/work/Psychoville
git add -A docs
git commit -m "docs: migrate .svx -> .md (docgen-rs directives, no Svelte)"
```

---

### Task C4: Build the site & verify

**Files:** none (verification; fixes loop back to earlier tasks)

- [ ] **Step 1: Build Psychoville with docgen-rs**

Run:
```bash
cargo run --manifest-path ~/work/docgen-rs/Cargo.toml -p docgen -- build ~/work/Psychoville 2>&1 | tail -20
```
Expected: build completes without error (exit 0), writes `~/work/Psychoville/dist/`.

- [ ] **Step 2: Verify partials excluded, includes + rustref + mermaid rendered**

Run:
```bash
cd ~/work/Psychoville
echo "partial pages (want 0): $(find dist -path '*/_*.gen/index.html' | wc -l)"
echo "directive errors (want 0): $(grep -rl 'docgen-directive-error' dist --include='index.html' | wc -l)"
# server crate page pulls in all four generated includes:
grep -q 'Systems' dist/dev/server/index.html && echo "INCLUDE_RENDERED"
grep -rq 'class="rust-ref' dist && echo "RUSTREF_RENDERED"
grep -rlq 'mermaid' dist && echo "MERMAID_PRESENT"
```
Expected: partial-page count `0`, directive-error count `0`, and the three OK markers print. **If `docgen-directive-error` count > 0, list the offenders** (`grep -rl 'docgen-directive-error' dist`) and trace each back to a bad `:include`/`:rustref` — fix the migration script or component, restore & re-run from Task C3.

- [ ] **Step 3: Visual spot-check (optional)**

Run: `cargo run --manifest-path ~/work/docgen-rs/Cargo.toml -p docgen -- dev ~/work/Psychoville`
Open `http://localhost:4321`, check: a RustRef-dense page (e.g. `/flows`), the `/dev/server` page (includes), and a mermaid page (`/dev/npc-ai/social`). Confirm rust-ref chips show the icon, includes show their lists, diagrams render. Ctrl-C when done.

- [ ] **Step 4: Final verification summary**

Confirm and report:
- docgen-rs `cargo test -p docgen-core -p docgen-build` green (from Part A).
- `find ~/work/Psychoville/docs -name '*.svx' | wc -l` → `0`.
- Psychoville build exit 0, `0` directive errors, `0` leaked partial pages.

---

## Self-Review notes

- **Spec coverage:** `:include` + `_`-exclusion (Task A1/A2/A3) ✓; project config (B1) ✓; `rustref` component incl. icon (B2) ✓; `.svx`→`.md`, script strip, RustRef, the four includes, `/docs/` links (C1–C3) ✓; verification incl. mermaid/partials/errors (C4) ✓. Out-of-scope items (old Svelte app, `tools/docs-gen`, Badge/SrcEmbed, `/rustdoc/` targets) intentionally untouched.
- **Type consistency:** `Partials`, `is_partial_rel`, `partition_partials`, `resolve_include_key`, `resolve_include_src`, and the `substitute(..., resolve_include)` signature are used identically across Tasks A1/A2. `render_doc`/`render_docs`/`render_block_markdown` arg order fixed (partials threaded consistently).
- **Deferred-in-spec item resolved:** docgen-rs URL scheme is `/<slug>/` (slug = rel_path minus `.md`); the old `/docs/<path>` links map to `/<path>` (Task C1 `DOCS_LINK_RE`).
- **Known fragility:** the `handlers.rs` partials threading (A2 Step 7) depends on that handler's local variable names; the step gives an explicit empty-map fallback so live-preview still compiles if the discovered-docs variable isn't in scope.
