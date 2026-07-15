# PlantUML Build Support — Design

**Status:** Approved (design phase)
**Date:** 2026-07-15

## Goal

Let docs authors embed PlantUML diagrams. Diagrams are referenced via a
`:::plantuml` block directive (a separate `.puml` file, or inline source). At
**build time**, docgen renders each diagram against an external PlantUML server
and embeds the resulting SVG inline in the page, so the published static site
has **zero runtime dependency** on the server. A `docgen plantuml` command runs
an ephemeral server container for local builds.

## Non-goals

- No client-side/browser rendering of PlantUML (there is no browser renderer;
  this is why rendering happens at build time, unlike mermaid).
- No PNG/TXT output formats — SVG only.
- No bundled/embedded PlantUML engine — always an external server.
- No automatic container management inside `docgen build` — the container is
  started explicitly via `docgen plantuml`.

## Architecture

PlantUML rendering is the first build-time capability in docgen-core that needs
a network call. To keep docgen-core network-free (as it is today), the design
mirrors the existing `asset_urls: Option<&dyn AssetUrlResolver>` injection
pattern (core defines a trait; a sibling crate provides the concrete, I/O-heavy
implementation; docgen-build wires them together):

| Crate | Responsibility |
|-------|----------------|
| `docgen-core` (`plantuml.rs`) | `PlantumlRenderer` trait, `PlantumlError` type, error-component HTML builder, directive glue. **No network, no new heavy deps.** |
| `docgen-plantuml` (**new**) | Concrete renderer: PlantUML text encoding, `ureq` HTTP GET, on-disk content-hash cache, structured errors, **and** the `docgen plantuml` container command. |
| `docgen-build` | Loads `.puml` source map, constructs the concrete renderer (server URL + cache dir), passes it into `render_docs`. Feature-gates on `features.plantuml`. |
| `docgen-config` | New `[plantuml]` config section + `features.plantuml`. |
| `docgen-assets` | CSS for `.docgen-plantuml` container + `.docgen-plantuml-error` component (added to the always-shipped hand-authored `docgen.css`; no asset gating needed). |
| `docgen` (CLI) | New `plantuml` subcommand → calls `docgen_plantuml::run_container`. |

## Authoring surface

```
:::plantuml{src="diagrams/arch.puml"}
:::
```

or inline:

```
:::plantuml
@startuml
Alice -> Bob: Hello
@enduml
:::
```

- **Block directive** named `plantuml`, handled as a **built-in** in
  `directivepass::substitute` (exactly like the existing `include` built-in),
  not a registry component.
- **Path resolution** reuses `pipeline::resolve_include_key(base_dir, src)`:
  relative to the referencing doc's directory; a leading `/` is docs-root
  absolute; `..` that escapes the docs root is rejected.
- **`src` vs inline precedence:** if `src` attribute is present, it wins and any
  inline body is ignored. If `src` is absent, the block body is the source. If
  both are absent/empty → an error component ("missing `src` and empty body").
- `.puml` source files are discovered from the docs tree and loaded into a
  `Diagrams` map (docs-relative path → source string), analogous to `Partials`.
  They are **excluded** from the `dist/` asset copy (they are inputs, not
  published assets).

## Rendering flow

1. `directivepass::substitute` recognizes the `plantuml` built-in and calls an
   injected `render_plantuml: &dyn Fn(&DirectiveInstance) -> String` closure
   (constructed in `pipeline.rs`, same place `resolve_include` is built).
2. The closure resolves the source (file via `Diagrams` map, or inline body),
   then calls the `PlantumlRenderer` trait object (threaded through
   `render_doc`/`render_docs` as `plantuml: Option<&dyn PlantumlRenderer>`).
3. On `Ok(svg)`: strip the SVG XML prolog/DOCTYPE, wrap the `<svg>` element in
   `<div class="docgen-plantuml">…</div>` (overflow-auto → wide diagrams scroll,
   like tables). SVG is treated as author-trusted content (same trust model as
   `render.unsafe = true` markdown).
4. On `Err(e)`: emit a styled `.docgen-plantuml-error` block (see Error
   handling). The build still succeeds.
5. When `plantuml: None` (feature off, or no renderer wired): every
   `:::plantuml` directive renders an inert "PlantUML rendering disabled" notice.

### `PlantumlRenderer` trait (docgen-core)

```rust
/// Renders PlantUML source to an inline SVG fragment. Implemented by
/// docgen-plantuml (network + cache); injected into the render pipeline like
/// AssetUrlResolver. Kept in core so directive glue does not depend on the
/// network implementation.
pub trait PlantumlRenderer {
    /// Render `source` (raw PlantUML text) to an SVG document string.
    fn render(&self, source: &str) -> Result<String, PlantumlError>;
}

/// A classified render failure, carrying enough detail for a specific,
/// non-generic error component.
#[derive(Debug, Clone)]
pub enum PlantumlError {
    /// Could not reach the server (connection refused, DNS, TLS, timeout).
    /// `server` is the URL tried; `detail` is the transport error text.
    Unreachable { server: String, detail: String },
    /// Server returned a non-success status. `status` is the HTTP code;
    /// `message`/`line` come from the PlantUML `X-PlantUML-Diagram-Error*`
    /// headers when present (the diagram had a syntax error).
    Server { status: u16, message: String, line: Option<u32> },
}
```

The error-component HTML builder (`plantuml_error_html`) lives in core and takes
a `PlantumlError` plus the diagram identity (the `src` path, or `inline #N`), so
both the "disabled" path and the render-failure path produce consistent markup.

## Server URL & container

- **Default server:** `http://localhost:8080` — the `plantuml/plantuml-server:jetty`
  default; SVG endpoint is `{server}/svg/{encoded}` at the root context.
- **Resolution precedence** (first match wins):
  1. `DOCGEN_PLANTUML_SERVER` env var
  2. `docgen.toml` `[plantuml] server`
  3. default `http://localhost:8080`
- **`docgen plantuml`** runs the container in the **foreground**:
  `docker run --rm -p 8080:8080 plantuml/plantuml-server:jetty`. It prints the
  server URL on start; Ctrl-C stops and (via `--rm`) auto-removes the container.
  - Container runtime is `docker` by default, overridable via
    `DOCGEN_CONTAINER_RUNTIME` (e.g. `podman`).
  - If the runtime binary is missing, exit with a clear, actionable error.
  - Image tag and host port are constants for v1 (not configurable).

## PlantUML text encoding (docgen-plantuml)

The server's `GET /svg/{encoded}` expects the PlantUML-encoded source:

1. UTF-8 encode the source.
2. Raw DEFLATE compress (no zlib/gzip header) — `flate2::write::DeflateEncoder`
   with default compression.
3. Base64-encode the deflated bytes using PlantUML's custom alphabet
   `0-9A-Za-z-_` (NOT standard base64) via the documented 3-byte→4-char mapping.

A unit test asserts a known source encodes to the known reference string.

## Caching

- Path: `{project_root}/.docgen/plantuml-cache/<key>.svg`, gitignored.
- Key: `sha256(server_url + "\n" + source)` (hex). Server URL is in the key so
  switching servers doesn't serve stale renders.
- Hit → return cached SVG, no server contact. Miss → render, write cache, return.
- Effect: unchanged diagrams survive a full rebuild with the server down; the
  dev-server edit loop does not re-hit the server for untouched diagrams.
- Cache writes are best-effort: a cache-write failure is logged and ignored (the
  render result is still used).
- **Self-ignoring:** on first use the renderer writes a `.gitignore` containing
  `*` into `.docgen/` (the way cargo/npm ignore their own caches), so the cache
  is never committed and no change to `docgen init` or the user's `.gitignore` is
  required. This works for existing projects too.

## Config / feature flag

- `[features] plantuml` — **defaults `true`** (parity with `mermaid`/`math`).
  Inert with zero server contact unless a `:::plantuml` directive is present.
- `[plantuml]` section:
  ```toml
  [plantuml]
  server = "http://localhost:8080"   # optional; env var overrides
  ```
  Absent `[plantuml]` → default server. Field is optional.

## HTTP client

`ureq` with rustls (matches `docgen-s3`'s `sync-rustls-tls` choice; the build is
synchronous, so no async runtime is pulled in). A fixed connect+read timeout
(10s) guards against a hung server. Timeout is not configurable in v1.

## Error component detail (never generic)

| Failure | Component shows |
|---------|-----------------|
| Server unreachable / timeout | diagram identity, the server URL tried, transport error text ("connection refused", "timed out") |
| PlantUML syntax error (HTTP 400) | diagram identity, PlantUML error message + line from `X-PlantUML-Diagram-Error` / `-Error-Line` headers |
| Other non-2xx | diagram identity, HTTP status code, body snippet |
| Missing `src` file / path escapes root | reuses include-style messages, plus diagram identity |
| `src` absent AND body empty | "missing `src` and empty body" |
| Feature disabled | "PlantUML rendering disabled (`features.plantuml = false`)" |

All render as a styled `.docgen-plantuml-error` block; the build never crashes.

## Testing

Unit (no real network — a mock `PlantumlRenderer`):

- Encoding round-trip: known source → known PlantUML-encoded string.
- Path resolution + `src`/inline precedence (src wins; inline used when no src;
  both-empty error).
- `.puml` map lookup: relative, docs-root-absolute, `..`-escape rejected.
- Error-component rendering for each `PlantumlError` variant + missing-file +
  disabled path (asserts the specific detail text appears, not a generic string).
- Cache hit/miss behavior (a fake renderer counting calls; second identical
  render is served from cache).
- Feature-off: `:::plantuml` → inert disabled notice, no renderer call.

Integration (thin smoke test, may require a running server → gated/ignored by
default): real `docgen-plantuml` renderer against a live server for one diagram.

The `docgen plantuml` container command's docker invocation is covered by
asserting the constructed command line (argv), not by launching docker in CI.

## File structure

- `crates/docgen-core/src/plantuml.rs` — trait, error type, error-HTML builder,
  directive glue helpers. Wire into `directivepass.rs` (built-in recognition) +
  `pipeline.rs` (closure construction, new `plantuml`/`diagrams` params) +
  `lib.rs` (module + re-exports).
- `crates/docgen-plantuml/` — new crate: `encode.rs`, `render.rs` (ureq + cache),
  `container.rs` (`run_container`), `lib.rs`, `Cargo.toml`.
- `crates/docgen-config/src/lib.rs` — `Features.plantuml`, `PlantumlConfig`,
  `SiteConfig.plantuml`, server-URL resolution helper.
- `crates/docgen-build/src/lib.rs` — discover/load `.puml` map, exclude `.puml`
  from asset copy, construct renderer, thread into `render_docs`.
- `crates/docgen/src/main.rs` — `plantuml` subcommand.
- `crates/docgen-assets/assets/docgen/docgen.css` — `.docgen-plantuml` +
  `.docgen-plantuml-error` styles.
- Workspace `Cargo.toml` — register `docgen-plantuml`.
- `website/` — docs page demonstrating the feature (dogfood).

## Open questions

None. All resolved during brainstorming.
