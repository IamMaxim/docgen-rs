# docgen-rs P5 — Dev server + live-reload + in-browser editor

**Date:** 2026-06-05
**Phase:** P5 (dev server + editor; the last interactive piece before P6 init/dist)
**Branch:** `overnight/p1-p6` (local only — never push/PR, never create branches)
**Status:** Plan approved, not yet implemented
**Spec:** `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`
(sections: *Dev story*, *Islands* — "Dev editor (dev only)" row, *Crate layout* — the `dev` CLI verb)

**Original references (parity targets):**
- Editor: `~/work/docgen/packages/docgen/src/lib/editor/` — `source-paths.server.ts`
  (the path-traversal guard we port to Rust), `source.server.ts`, `client.ts`, `types.ts`.
- Vite plugin: `~/work/docgen/packages/docgen/src/lib/vite/preview-dev-server.ts`
  (the file-write dev endpoint we replace with axum).

---

## 0. Scope, decisions, and what P5 inherits

### 0.1 Goal restated

`docgen dev` = `axum` (bind `127.0.0.1` only) + `notify` file watcher + SSE live-reload +
an in-browser CodeMirror editor with a path-guarded file-write endpoint. Two clusters,
landed strictly in order:

- **Cluster A — reusable build + dev server.** Extract the entire
  *discover → render → emit* pipeline (currently inline in `crates/docgen/src/build.rs::build`)
  into a single reusable `build_site(opts) -> Result<BuildOutcome>` function that BOTH
  `docgen build` AND the dev server call (no duplicated pipeline). Add a `dev` subcommand:
  an axum server that serves the built site from an output dir, a debounced `notify` watcher on
  `docs/` that rebuilds on change, an SSE endpoint that pushes a "reload" event after each
  rebuild, and a tiny dev-only reload-client script injected into every served HTML page.

- **Cluster B — CodeMirror editor + path-guarded write endpoint.** Vendor CodeMirror 5
  (single-file UMD, no bundler) into `docgen-assets` as **dev-only** assets; add an editor
  Alpine island (`docgenEditor`, registered via `window.docgen.island`) that loads CodeMirror to
  edit the current doc's markdown source; add a `PUT /__docgen/source` endpoint that writes the
  edited markdown back into `docs/` behind a **strict path-traversal guard** (the Rust port of
  `validateRepoDocPath`), after which the watcher rebuilds and live-reload fires.

Pure-Rust + in-process axum-handler tests assert: the path-traversal guard rejects every
escape vector, the write endpoint persists only in-bounds writes, the SSE channel delivers a
reload signal, and `build_site` produces identical output for `build` and `dev`. Interactive
editing + live reload in a real browser is validated separately by the architect (manual).

### 0.2 FLAGGED DECISION — CodeMirror version: **CodeMirror 5 (UMD single-file). Confirmed.**

Probed on this machine before writing the plan:

```
$ curl -fsSI https://cdn.jsdelivr.net/npm/codemirror@6.0.1/dist/index.js   # → 200, BUT...
  # CodeMirror 6 ships as ESM with BARE imports (`@codemirror/state` etc.) — unusable
  # without a bundler. "Cargo-only, no npm/bundler" forbids resolving those imports.

$ curl -fsSL https://cdn.jsdelivr.net/npm/codemirror@5.65.16/lib/codemirror.js | head
  // CodeMirror, copyright ... (UMD, self-contained)
$ grep -c '^import ' codemirror.js   # → 0   (classic UMD, no bare imports)
```

CodeMirror **6** is ESM-only with bare module specifiers — it REQUIRES a bundler to assemble
`@codemirror/{state,view,commands,lang-markdown,...}` into one file. That violates the
cargo-only / no-bundler constraint. CodeMirror **5** (`5.65.16`) ships a single self-contained
UMD `lib/codemirror.js` (+ `lib/codemirror.css` + standalone `mode/markdown/markdown.js`,
`mode/xml/xml.js`, `addon/mode/overlay.js` which markdown mode depends on) — all loadable as
plain `<script>`/`<link>` with **no import resolution**. This is the only no-bundler-compatible
option and is exactly the "vendored prebuilt, pinned, via curl" pattern P3 established for
Alpine/KaTeX/Mermaid. Decision: **vendor CodeMirror 5.65.16**, dev-only.

CM5 markdown mode's dependency chain (verified from CM5 source): `markdown.js` requires the
`xml` mode and the `overlay` addon to be present (for embedded-HTML highlighting). We vendor
all four files. They load in dependency order via the editor island's `loadScript` chain.

### 0.3 THE DEV-ONLY GATING MECHANISM (most important section — read first)

The editor UI, the file-write endpoint, the SSE endpoint, the reload-client script, and the
vendored CodeMirror assets MUST appear **only** under `docgen dev` and **NEVER** in a static
`docgen build` dist. Three independent, layered gates enforce this — defence in depth:

1. **Endpoints live only on the dev server.** `PUT /__docgen/source`, `GET /__docgen/source`,
   `GET /__docgen/livereload` (SSE), and the routes serving CodeMirror are registered on the
   axum `Router` built by `docgen-server`. `docgen build` never constructs that router and never
   links the write/SSE handlers into anything it emits. A static dist is just files on disk; it
   has no server, so these endpoints cannot exist there.

2. **HTML injection happens at *serve* time, not at *render* time.** The production renderer
   (`docgen-render`) and `page.html` template are **NOT modified** — they stay
   production-pure. The dev server post-processes each served HTML response with a single
   string injection (`inject_dev_html`) that adds, immediately before `</body>`:
   the reload-client `<script>`, the editor-toggle button, and the editor island `<script>`
   tags + CodeMirror CSS `<link>`. Because the injection is applied by the *serving layer* and
   never written to disk by `build_site`, the on-disk dist (what `docgen build` produces and
   what users deploy) contains zero editor/reload markup. This is the cleanest gate: the
   dev-only HTML literally cannot leak into a static build because the static build never runs
   the injection.

3. **CodeMirror assets are gated out of the static emit set.** `docgen-assets` gains a
   `dev_assets()` slice (CodeMirror UMD + css + modes + the editor island JS + the reload-client
   JS) that is **never** included by `assets_for(&EmitOptions)` — the function `docgen build`
   uses. The dev server emits `dev_assets()` into its output dir as an extra step that
   `build_site` does NOT perform. A test (`assets_for_never_includes_dev_assets`) locks this:
   for every `EmitOptions` combination, no `dev_assets()` path appears in `assets_for`.

To make gate (1) and (3) auditable in one place, `build_site` takes an explicit
`BuildMode { Production, Dev }` enum (default `Production`). `build_site` itself emits ONLY
production assets regardless of mode — the mode is recorded for logging/parity and to let the
dev server assert it built in `Dev` context — and the dev-only emission + injection are done by
`docgen-server` AFTER `build_site` returns. Production `docgen build` calls
`build_site(BuildMode::Production)` and stops. There is no code path in which `docgen build`
emits dev assets or injects dev HTML.

### 0.4 Security posture (non-negotiable)

- **Bind `127.0.0.1` only.** The axum listener binds `SocketAddr::from(([127, 0, 0, 1], port))`.
  Never `0.0.0.0`. A test asserts the constructed bind address is loopback.
- **Write endpoint path-traversal guard.** `PUT /__docgen/source` accepts a `path` that must
  resolve to a regular file strictly inside the canonicalized `docs/` dir. The guard
  (`resolve_doc_path`, the Rust port of `validateRepoDocPath`) rejects: absolute paths, any
  `..` component, a leading `/`, backslashes, a `.md` requirement, a path whose
  `canonicalize()` (realpath) escapes `docs/` (symlink escape), and a target that is not a
  regular file. Rejections return `403`/`400` and write nothing. A dedicated rejection test
  enumerates every vector.
- **Dev server is localhost-only and dev-only.** Gate 0.3 guarantees nothing dev-related ships.

### 0.5 What P5 inherits (do not re-derive)

- `build_site`'s body is the **existing** `build.rs::build` logic, refactored — do NOT rewrite
  the pipeline. The current function already does discover → prepare → render_docs → tree →
  history pages → doc pages → graph → search index → `docgen_assets::emit`. We lift it verbatim
  into a parameterized function.
- The island convention from P3/P4: `window.docgen.island(name, fn)` in `bootstrap.js`, islands
  self-register, Alpine starts once on `alpine:init`. The editor island plugs into this exactly
  like `docgenGraph`/`docgenMermaid`.
- `docgen_assets::{Asset, AssetKind, emit}` and the `EmitOptions` planner — reused unchanged for
  production; extended with a non-default `dev_assets()` slice.
- The VENDOR.md convention for pinned curl-fetched third-party files.

---

## 1. New / changed crates and deps

### 1.1 New crate `docgen-server` (lib) — the dev server

A new workspace member `crates/docgen-server`. Keeping it a lib (not inline in the `docgen` bin)
makes the axum handlers in-process-testable via `tower::ServiceExt::oneshot` without binding a
port, matching the plan's TDD bias.

`crates/docgen-server/Cargo.toml`:

```toml
[package]
name = "docgen-server"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
anyhow = "1.0.102"
axum = "0.7"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "fs", "net", "signal", "time"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["fs"] }
notify = "6"
notify-debouncer-mini = "0.4"
sha2 = "0.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1.0.150"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
docgen-core = { path = "../docgen-core" }
docgen-render = { path = "../docgen-render" }
docgen-diff = { path = "../docgen-diff" }
docgen-assets = { path = "../docgen-assets" }
docgen-build = { path = "../docgen-build" }

[dev-dependencies]
http-body-util = "0.1"
tempfile = "3"
```

> Probe to run first (the implementer does this before writing code, per TDD setup):
> `cargo add` each of axum/tokio/tower/tower-http/notify/notify-debouncer-mini/sha2 in a scratch
> crate to confirm versions resolve on this machine; pin the resolved minor versions into the
> manifest above. `tower-http` `fs` feature pulls `ServeDir`. If `notify-debouncer-mini` 0.4
> does not resolve against `notify` 6, fall back to a hand-rolled 200 ms debounce over a raw
> `notify` watcher (Cluster A task A-5 covers both shapes).

### 1.2 New crate `docgen-build` (lib) — the reusable build

The reusable `build_site` must be callable from BOTH the `docgen` bin and `docgen-server`.
Today it lives in the bin's `build.rs` (+ `history.rs`), which the server cannot depend on (a bin
is not a library). Extract it into a small lib crate `crates/docgen-build` that both depend on.
This is the clean home for the "single build the whole site" function.

`crates/docgen-build/Cargo.toml`:

```toml
[package]
name = "docgen-build"
edition.workspace = true
license.workspace = true
version.workspace = true

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.41", default-features = false, features = ["clock"] }
docgen-core = { path = "../docgen-core" }
docgen-diff = { path = "../docgen-diff" }
docgen-assets = { path = "../docgen-assets" }
docgen-render = { path = "../docgen-render" }

[dev-dependencies]
tempfile = "3"
```

### 1.3 Workspace + bin changes

- `Cargo.toml` (workspace): add `"crates/docgen-build"` and `"crates/docgen-server"` to
  `members`.
- `crates/docgen/Cargo.toml`: add `docgen-build = { path = "../docgen-build" }` and
  `docgen-server = { path = "../docgen-server" }`; the bin keeps `clap`. The bin's old
  `build.rs`/`history.rs` move into `docgen-build`; the bin's `Command::Build` arm calls
  `docgen_build::build_site(...)`, and a new `Command::Dev` arm calls
  `docgen_server::serve(...)`.

---

## 2. Public API (precise types)

### 2.1 `docgen-build` (crate `docgen_build`)

```rust
/// Whether this build is for static distribution or for the dev server.
/// `build_site` emits ONLY production assets in BOTH modes; the dev server adds
/// dev-only assets/HTML itself, AFTER build_site returns. The mode is recorded
/// for logging + so the dev server can assert its build context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode {
    #[default]
    Production,
    Dev,
}

/// Inputs to a full site build.
pub struct BuildOptions<'a> {
    /// Project root containing `docs/` (output goes to `out_dir`).
    pub project_root: &'a std::path::Path,
    /// Where the static site is written. `docgen build` passes `project_root/dist`;
    /// the dev server passes a temp dir it owns.
    pub out_dir: &'a std::path::Path,
    pub mode: BuildMode,
}

/// Result of a build (counts for logging; extend later if needed).
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub page_count: usize,
    pub any_mermaid: bool,
    pub out_dir: std::path::PathBuf,
}

/// Discover → render → emit the whole site into `opts.out_dir`. This is the single
/// pipeline both `docgen build` and `docgen dev` call. Wipes + recreates `out_dir`.
pub fn build_site(opts: &BuildOptions) -> anyhow::Result<BuildOutcome>;

/// Back-compat thin wrapper used by `docgen build`: builds `root/docs` into `root/dist`
/// in Production mode. Equivalent to the old `build::build(root)`.
pub fn build(project_root: &std::path::Path) -> anyhow::Result<BuildOutcome>;
```

`build_site`'s body is the current `build.rs::build` body, with `docs_dir`/`dist_dir`
parameterized to `opts.project_root.join("docs")` and `opts.out_dir`, and the final `println!`
replaced by returning `BuildOutcome`. `report_to_buckets` (today in the bin's `history.rs`)
moves into `docgen-build` as a private module. **No pipeline logic changes** — pure extraction.

### 2.2 `docgen-server` (crate `docgen_server`)

```rust
/// Dev-server configuration.
pub struct DevOptions {
    pub project_root: std::path::PathBuf,
    /// Loopback port. Default 4321.
    pub port: u16,
    /// Open a browser on start (off in tests/CI). Default false.
    pub open: bool,
}

/// Run the dev server: initial build, spawn the debounced watcher, bind 127.0.0.1,
/// serve until Ctrl-C. Blocking entry point the `docgen dev` CLI arm calls.
pub fn serve(opts: DevOptions) -> anyhow::Result<()>;

/// Shared, cheaply-clonable state behind every handler. `Clone` = bump Arc/broadcast handle.
#[derive(Clone)]
pub struct AppState {
    pub project_root: std::path::PathBuf,
    pub out_dir: std::path::PathBuf,
    pub docs_dir: std::path::PathBuf,            // canonicalized
    pub reload_tx: tokio::sync::broadcast::Sender<ReloadEvent>,
}

/// One live-reload signal. Carried over the SSE channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadEvent { Reload }

/// Build the axum router (NO listener) for the given state. Split out so handler tests
/// can `oneshot` requests without binding a port.
pub fn router(state: AppState) -> axum::Router;

/// Rebuild the site into `state.out_dir` (Dev mode + dev-asset emission), then broadcast
/// a reload. Called on every debounced fs change AND after a successful editor write.
/// Returns Err only on a hard build failure; the caller logs and keeps serving.
pub fn rebuild_and_reload(state: &AppState) -> anyhow::Result<()>;

/// Post-process a served HTML body: inject the reload-client script + editor toggle +
/// editor island scripts/styles immediately before `</body>`. Dev-only; never run by
/// `docgen build`. Pure string fn so it is unit-testable.
pub fn inject_dev_html(html: &str) -> String;

// ---- path guard (the security core; ported from validateRepoDocPath) ----

#[derive(Debug, PartialEq, Eq)]
pub enum PathGuardError {
    NotMarkdown,     // 400
    Absolute,        // 400
    Traversal,       // 403  (.. component, backslash, or realpath escape)
    NotAFile,        // 400
    NotFound,        // 404
}

/// Resolve a client-supplied doc-relative path (e.g. "guide/intro.md") to a canonical
/// absolute path strictly inside `docs_dir`, or reject. `docs_dir` MUST already be
/// canonicalized by the caller. Mirrors `validateRepoDocPath`'s layered checks.
pub fn resolve_doc_path(
    docs_dir: &std::path::Path,
    rel: &str,
) -> Result<std::path::PathBuf, PathGuardError>;
```

Request/response payloads for the write endpoint (mirrors `types.ts`):

```rust
#[derive(serde::Deserialize)]
pub struct SaveRequest {
    /// docs-relative path, e.g. "guide/intro.md".
    pub path: String,
    pub source: String,
    /// sha256 hex of the source last loaded; optimistic-concurrency guard. Optional in dev.
    #[serde(default)]
    pub disk_hash: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SaveResponse { pub path: String, pub disk_hash: String }

#[derive(serde::Serialize)]
pub struct SourceResponse { pub path: String, pub source: String, pub disk_hash: String }

#[derive(serde::Serialize)]
pub struct ApiError { pub error: String }
```

### 2.3 Routes (registered only on the dev `router`)

| Method | Path | Handler | Purpose |
| --- | --- | --- | --- |
| `GET` | `/__docgen/livereload` | `livereload_sse` | SSE stream; emits `event: reload` on rebuild |
| `GET` | `/__docgen/source?path=…` | `get_source` | return current markdown + `disk_hash` for the editor |
| `PUT` | `/__docgen/source` | `put_source` | path-guarded write → rebuild → reload |
| `GET` | `/*path` | `ServeDir` + `inject_dev_html` fallthrough | serve the built site, injecting dev HTML into `text/html` |

The `__docgen` prefix namespaces every dev-only route, so it cannot collide with a doc slug and
is trivially greppable as "dev-only surface."

---

## 3. Cluster A — reusable build + dev server + SSE live-reload

Land tasks strictly in order. Each task: write the test(s), watch them fail, implement, watch
them pass, then `cargo test` + `cargo clippy --all-targets` green, then commit.

### A-1 — Extract `docgen-build` crate (pure refactor, zero behavior change)

**Files:** new `crates/docgen-build/{Cargo.toml,src/lib.rs,src/history.rs}`; move the bodies of
`crates/docgen/src/build.rs` + `crates/docgen/src/history.rs` into it; update workspace
`members`; edit `crates/docgen/src/main.rs` to `use docgen_build` and `crates/docgen/Cargo.toml`
to depend on it; delete the bin's `mod build;`/`mod history;`.

**`crates/docgen-build/src/lib.rs`** — the extracted pipeline, parameterized:

```rust
mod history;
use history::report_to_buckets;
// ... (all the existing imports from build.rs) ...

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode { #[default] Production, Dev }

pub struct BuildOptions<'a> {
    pub project_root: &'a std::path::Path,
    pub out_dir: &'a std::path::Path,
    pub mode: BuildMode,
}

#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub page_count: usize,
    pub any_mermaid: bool,
    pub out_dir: std::path::PathBuf,
}

pub fn build(project_root: &std::path::Path) -> anyhow::Result<BuildOutcome> {
    build_site(&BuildOptions {
        project_root,
        out_dir: &project_root.join("dist"),
        mode: BuildMode::Production,
    })
}

pub fn build_site(opts: &BuildOptions) -> anyhow::Result<BuildOutcome> {
    let docs_dir = opts.project_root.join("docs");
    let dist_dir = opts.out_dir;     // <- was project_root/dist
    // ... EXACT body of today's build.rs::build, with `dist_dir` now `opts.out_dir` ...
    // ... final lines: capture counts, return BuildOutcome instead of println! ...
    Ok(BuildOutcome { page_count: site.docs.len(), any_mermaid: site.any_mermaid,
                      out_dir: dist_dir.to_path_buf() })
}
```

The `_ = opts.mode;` is referenced (logging) so the field is not dead. `build_site` emits only
`assets_for(&EmitOptions{..})` — i.e. production assets — in both modes (per 0.3).

**Tests** (`crates/docgen-build/tests/build_site.rs`, ported/extended from the bin's
`build_cli.rs`):

- `build_site_writes_pages_to_custom_out_dir`: build the `fixtures/site-basic` docs into a
  `tempfile::tempdir()` `out_dir` ≠ `project_root/dist`; assert `out_dir/index/index.html`,
  `out_dir/search-index.json`, `out_dir/bootstrap.js`, `out_dir/graph/index.html` exist. Proves
  the pipeline honors an arbitrary out dir (what the dev server needs).
- `build_compat_wrapper_writes_dist`: `build(root)` still writes `root/dist/...` — back-compat.
- `dev_and_production_modes_emit_identical_files`: build the same fixture twice into two out
  dirs with `BuildMode::Production` then `BuildMode::Dev`; assert the set of emitted relative
  paths is identical (proves `build_site` itself is dev-asset-free — gate 0.3, layer at the
  build level).

The existing `crates/docgen/tests/build_cli.rs` keeps passing unchanged (it shells out to the
`docgen` bin, whose `build` arm now delegates to `docgen_build::build`). Keep it as the
integration guard.

**Bin wiring** — `crates/docgen/src/main.rs`:

```rust
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Subcommand)]
enum Command {
    /// Build the static site from `docs/` into `dist/`.
    Build { #[arg(default_value = ".")] root: PathBuf },
    /// Run the dev server with live reload + in-browser editor (localhost only).
    Dev {
        #[arg(default_value = ".")] root: PathBuf,
        #[arg(long, default_value_t = 4321)] port: u16,
        #[arg(long, default_value_t = false)] open: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Build { root } => { docgen_build::build(&root)?; Ok(()) }
        Command::Dev { root, port, open } =>
            docgen_server::serve(docgen_server::DevOptions { project_root: root, port, open }),
    }
}
```

**Commit:** `P5(A): extract reusable build_site into docgen-build crate`

### A-2 — `resolve_doc_path` guard + `inject_dev_html` (pure fns, no server yet)

**File:** new `crates/docgen-server/src/lib.rs` (start it here; grows over A-3..A-7).
Implement `resolve_doc_path` + `PathGuardError` and `inject_dev_html` first because they are
pure and fully testable with zero async.

`resolve_doc_path` algorithm (Rust port of `validateRepoDocPath`, layered):

1. Reject if `rel` contains a backslash → `Traversal`. Reject if `rel` starts with `/` or is an
   absolute `Path` → `Absolute`.
2. Normalize: strip leading `./`. Split on `/`; if ANY component is `..` (or `.` repeated to no
   purpose) → `Traversal`. Reject empty.
3. Require the path to end in `.md` (case-sensitive) → else `NotMarkdown`.
4. `candidate = docs_dir.join(rel)`. `lexical`-check: `candidate` must start with `docs_dir`.
5. Canonicalize `docs_dir` (caller already did) and `candidate.canonicalize()`:
   - If canonicalize errors with `NotFound` → `NotFound`.
   - The canonical target must start with the canonical `docs_dir`; else (symlink escape) →
     `Traversal`.
6. `std::fs::symlink_metadata(canonical)` must be a regular file (not dir, not symlink-to-dir);
   else `NotAFile`.
7. Return the canonical absolute path.

> Note vs. the original: the TS guard allowed `.svx`; we are markdown-only (per spec — `.svx`
> is unsupported in the Rust rewrite), so the extension whitelist is `{.md}` only.

`inject_dev_html(html)`:

```rust
const DEV_HTML: &str = r#"
<link rel="stylesheet" href="/__codemirror/codemirror.css" />
<button class="docgen-edit-toggle" data-docgen-edit>Edit</button>
<div id="docgen-editor" x-data="docgenEditor" x-cloak></div>
<script src="/__codemirror/codemirror.js"></script>
<script src="/__codemirror/xml.js"></script>
<script src="/__codemirror/overlay.js"></script>
<script src="/__codemirror/markdown.js"></script>
<script src="/__docgen/editor.js"></script>
<script src="/__docgen/livereload.js"></script>
"#;

pub fn inject_dev_html(html: &str) -> String {
    match html.rfind("</body>") {
        Some(i) => { let mut s = String::with_capacity(html.len() + DEV_HTML.len());
                     s.push_str(&html[..i]); s.push_str(DEV_HTML); s.push_str(&html[i..]); s }
        None => format!("{html}{DEV_HTML}"),  // graceful: append if no </body>
    }
}
```

**Tests** (`crates/docgen-server/tests/path_guard.rs` + a `inject` unit test in `lib.rs`):

- **THE path-traversal rejection test** `rejects_all_traversal_vectors`: build a `tempdir`
  `docs/` containing `ok.md` and a subdir `guide/intro.md`; also create `secret.md` OUTSIDE
  `docs/` and a symlink `docs/escape.md -> ../secret.md`. Assert:
  - `resolve_doc_path(docs, "guide/intro.md")` → `Ok`, canonical, inside docs.
  - `"../secret.md"` → `Err(Traversal)`.
  - `"..%2f.."`-style raw `../../etc/passwd` → `Err(Traversal)`.
  - `"/etc/passwd"` → `Err(Absolute)`.
  - absolute tempdir path string → `Err(Absolute)`.
  - `"guide\\..\\..\\x.md"` (backslash) → `Err(Traversal)`.
  - `"ok.txt"` → `Err(NotMarkdown)`.
  - `"nope.md"` (in-bounds but absent) → `Err(NotFound)`.
  - `"guide.md"` where `guide` is a dir → `Err(NotAFile)` (or NotFound, depending on existence;
    use an existing dir name to force NotAFile).
  - **symlink escape** `"escape.md"` → `Err(Traversal)` (realpath leaves docs). (Skip the
    symlink assertion gracefully on platforms without symlink support — darwin/linux have it.)
- `inject_dev_html_inserts_before_body`: asserts the injected markers (`__docgen/livereload.js`,
  `docgenEditor`, `__codemirror/codemirror.js`) appear, and appear BEFORE `</body>`.
- `inject_dev_html_no_body_appends`: input without `</body>` still gets the markers appended.

**Commit:** `P5(A): path-traversal guard + dev-html injection (pure, tested)`

### A-3 — `AppState`, `router`, and the `ServeDir` + injection fallthrough

**File:** `crates/docgen-server/src/lib.rs` (+ `src/handlers.rs`).

Build `router(state)` with `tower_http::services::ServeDir` rooted at `state.out_dir` for static
files, the three `/__docgen/*` routes, and the `/__codemirror/*` + `/__docgen/{editor,livereload}.js`
asset routes (served from embedded `docgen_assets::dev_assets()` bytes — see B-1). HTML responses
pass through a small middleware/handler that runs `inject_dev_html` on any `text/html` body.

Implementation note: rather than fight `ServeDir`'s body type for HTML rewriting, register an
explicit handler for `/` and `/*path` that (a) resolves the request path to
`out_dir/<path>/index.html` (clean URLs, matching the build's `dist/<slug>/index.html` layout),
(b) for `.html`/dir requests reads the file and returns `inject_dev_html(body)` with
`content-type: text/html`, (c) for everything else streams the raw bytes with a guessed
content-type. This keeps injection deterministic and testable. (A `ServeDir` fallback handles
fonts/css/js exactly; injection only touches HTML.)

**Tests** (`crates/docgen-server/tests/server.rs`, all in-process via `oneshot`):

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
```

- `serves_built_index_with_injected_dev_html`: set up a tempdir, `build_site` the fixture into
  `out_dir`, build `router(state)`, `oneshot` `GET /` (or `/index/`) → 200, body contains the
  doc HTML AND the injected `__docgen/livereload.js` marker (proves serve-time injection works).
- `serves_static_asset_without_injection`: `GET /docgen.css` → 200, `content-type` css-ish, body
  does NOT contain `__docgen/livereload.js` (injection is HTML-only).
- `unknown_path_404`: `GET /does/not/exist` → 404.

**Commit:** `P5(A): axum router serving the built site with dev-html injection`

### A-4 — SSE live-reload endpoint + `rebuild_and_reload`

**File:** `crates/docgen-server/src/handlers.rs`.

`livereload_sse`: subscribe to `state.reload_tx`, return an `axum::response::sse::Sse` stream
that maps each `ReloadEvent::Reload` to `Event::default().event("reload").data("now")`, with a
keep-alive comment every ~15 s.

`rebuild_and_reload(state)`: call `docgen_build::build_site(&BuildOptions{ project_root, out_dir,
mode: Dev })`, then emit `docgen_assets::dev_assets()` into `out_dir` (the dev-only extra step),
then `let _ = state.reload_tx.send(ReloadEvent::Reload);` (ignore "no subscribers"). Log via
`tracing` (page count, elapsed). On build error: `tracing::error!`, return `Err`, do NOT send a
reload (caller keeps serving the last good build).

**Tests** (`crates/docgen-server/tests/server.rs`):

- `rebuild_broadcasts_reload`: create `AppState` with a `broadcast::channel`; `subscribe()`;
  call `rebuild_and_reload(&state)` (after an initial build so `out_dir` is valid); assert the
  subscriber receives `ReloadEvent::Reload` (use `try_recv`/`recv` under a `tokio::test`).
- `rebuild_regenerates_changed_page` (the **rebuild-on-change** test, invoked directly — no fs
  watcher, no port): build the fixture; read `out_dir/index/index.html`; overwrite
  `docs/index.md` with new title text; call `rebuild_and_reload(&state)`; re-read
  `out_dir/index/index.html` and assert the new title is present. Proves a source edit →
  rebuild → fresh output, deterministically and synchronously.
- `failed_build_does_not_broadcast`: point state at a docs dir, then make `build_site` fail
  (e.g. remove the docs dir) and assert `rebuild_and_reload` returns `Err` and no reload is
  delivered. (If `build_site` is tolerant of an empty docs dir, force failure another way, e.g.
  an unreadable out_dir; otherwise assert the `Ok` path doesn't double-send.)

**Commit:** `P5(A): SSE live-reload endpoint + rebuild_and_reload`

### A-5 — debounced `notify` watcher + `serve` entry point

**File:** `crates/docgen-server/src/watch.rs` + `serve` in `lib.rs`.

`serve(opts)`:

1. `tracing_subscriber` init (idempotent; `try_init`).
2. Canonicalize `docs_dir`; create an owned `out_dir` (a `tempfile::TempDir` kept alive for the
   process, OR `project_root/.docgen-dev`). Initial `build_site(Dev)` + `dev_assets()` emit.
3. `let (reload_tx, _) = broadcast::channel(16);` build `AppState`.
4. Spawn the watcher: `notify-debouncer-mini` (200 ms) on `docs_dir` recursive; on any debounced
   event call `rebuild_and_reload(&state)` (log + swallow errors). Run the watcher on a blocking
   thread that forwards events into the tokio runtime via the state (the watcher closure clones
   `AppState`).
5. Bind `SocketAddr::from(([127, 0, 0, 1], opts.port))`; `axum::serve(listener, router(state))`;
   log `http://127.0.0.1:<port>`; optionally open browser when `opts.open`.
6. Graceful shutdown on Ctrl-C (`tokio::signal::ctrl_c`).

`serve` builds its own tokio runtime (`tokio::runtime::Builder::new_multi_thread`) so the
`docgen` bin's `main` stays a plain `fn main() -> Result<()>` (no `#[tokio::main]` on the bin —
matches the existing minimal bin).

**Tests:**

- `bind_addr_is_loopback`: a tiny `fn dev_bind_addr(port) -> SocketAddr` returns
  `127.0.0.1:port`; assert `.ip().is_loopback()`. (Security: never `0.0.0.0`.)
- The watcher itself is NOT port/fs-timing tested (flaky); its payload — `rebuild_and_reload` —
  is covered by A-4's direct-invocation test. Note in the plan: **live watch + reload is
  validated in-browser by the architect** (edit a `.md`, see the page reload).

**Commit:** `P5(A): notify watcher + localhost dev server entry point`

### A-6 — reload-client script (dev-only asset)

**File:** `crates/docgen-assets/assets/docgen/dev/livereload.js` (new; embedded, dev-only slice).

```js
// Dev-only live reload. Connects to the dev server's SSE channel; on a `reload`
// event, reloads the page. Never emitted by `docgen build`.
(function () {
  try {
    var es = new EventSource('/__docgen/livereload');
    es.addEventListener('reload', function () { location.reload(); });
    es.onerror = function () { /* server restarting; EventSource auto-retries */ };
  } catch (e) { console.warn('[docgen] livereload unavailable', e); }
})();
```

**Test** (in `docgen-assets`): `dev_livereload_connects_to_sse_endpoint` — the embedded bytes
contain `EventSource('/__docgen/livereload')` and `location.reload`.

**Commit:** `P5(A): dev-only live-reload client script`

---

## 4. Cluster B — CodeMirror editor + path-guarded write endpoint + dev-only gating

### B-1 — Vendor CodeMirror 5 + define `dev_assets()` (gated out of `assets_for`)

**Curl commands (pinned, run from repo root; record in VENDOR.md):**

```bash
CM=crates/docgen-assets/assets/vendor/codemirror
mkdir -p "$CM"
V=5.65.16
curl -fsSL "https://cdn.jsdelivr.net/npm/codemirror@${V}/lib/codemirror.js"            -o "$CM/codemirror.js"
curl -fsSL "https://cdn.jsdelivr.net/npm/codemirror@${V}/lib/codemirror.css"           -o "$CM/codemirror.css"
curl -fsSL "https://cdn.jsdelivr.net/npm/codemirror@${V}/mode/markdown/markdown.js"    -o "$CM/markdown.js"
curl -fsSL "https://cdn.jsdelivr.net/npm/codemirror@${V}/mode/xml/xml.js"              -o "$CM/xml.js"
curl -fsSL "https://cdn.jsdelivr.net/npm/codemirror@${V}/addon/mode/overlay.js"        -o "$CM/overlay.js"
```

> Probed: each URL returns `200 application/javascript` and `codemirror.js` is UMD with zero
> bare `import` statements (no bundler needed). markdown mode depends on `xml` mode + `overlay`
> addon — all five vendored.

**Files:**
- `crates/docgen-assets/assets/docgen/dev/editor.js` (new — the editor island, B-2).
- `crates/docgen-assets/assets/docgen/dev/livereload.js` (from A-6).
- `crates/docgen-assets/src/lib.rs`: add `dev_assets()`.

```rust
/// DEV-ONLY assets: vendored CodeMirror (UMD) + css + modes, the editor island,
/// and the live-reload client. Served by `docgen dev` ONLY. NEVER returned by
/// `assets_for` and NEVER emitted by `docgen build`. Dist paths are namespaced
/// under `__codemirror/` and `__docgen/` so they cannot collide with doc slugs.
pub fn dev_assets() -> Vec<Asset> {
    vec![
        embed("vendor/codemirror/codemirror.js",  "__codemirror/codemirror.js",  AssetKind::Js),
        embed("vendor/codemirror/codemirror.css", "__codemirror/codemirror.css", AssetKind::Css),
        embed("vendor/codemirror/markdown.js",    "__codemirror/markdown.js",    AssetKind::Js),
        embed("vendor/codemirror/xml.js",         "__codemirror/xml.js",         AssetKind::Js),
        embed("vendor/codemirror/overlay.js",     "__codemirror/overlay.js",     AssetKind::Js),
        embed("docgen/dev/editor.js",     "__docgen/editor.js",     AssetKind::Js),
        embed("docgen/dev/livereload.js", "__docgen/livereload.js", AssetKind::Js),
    ]
}
```

**Tests** (`docgen-assets`):
- `dev_assets_has_codemirror_and_editor_and_reload`: paths include
  `__codemirror/codemirror.js`, `__codemirror/codemirror.css`, `__codemirror/markdown.js`,
  `__docgen/editor.js`, `__docgen/livereload.js`; all bytes non-empty.
- `codemirror_is_umd_not_esm`: `__codemirror/codemirror.js` bytes do NOT start a line with
  `import ` (no bare ESM) — locks the no-bundler invariant.
- **`assets_for_never_includes_dev_assets`** (gate 0.3, layer 3 — the critical lock): for the
  default and the all-flags-on `EmitOptions`, assert NONE of `dev_assets()`'s dist paths appear
  in `assets_for(&opts)`. Iterate a matrix of all 8 flag combinations.

**Commit:** `P5(B): vendor CodeMirror 5 (UMD) as dev-only assets, gated out of static emit`

### B-2 — editor island (`docgenEditor`) + `get_source`/`put_source` handlers

**File:** `crates/docgen-assets/assets/docgen/dev/editor.js` — the Alpine island. Follows the
P3/P4 island convention exactly (`window.docgen.island('docgenEditor', fn)`):

```js
window.docgen.island('docgenEditor', function (Alpine) {
  Alpine.data('docgenEditor', function () {
    return {
      open: false, cm: null, path: '', diskHash: '', status: '',
      init() {
        var self = this;
        document.querySelectorAll('[data-docgen-edit]').forEach(function (b) {
          b.addEventListener('click', function () { self.toggle(); });
        });
      },
      docPath() {
        // map the current page URL (/guide/intro) to its source (guide/intro.md).
        var p = location.pathname.replace(/^\/+|\/+$/g, '');
        if (p === '' ) p = 'index';
        return p + '.md';
      },
      async toggle() {
        this.open = !this.open;
        this.$el.style.display = this.open ? 'block' : 'none';
        if (this.open && !this.cm) await this.mount();
      },
      async mount() {
        this.path = this.docPath();
        var r = await fetch('/__docgen/source?path=' + encodeURIComponent(this.path));
        var data = await r.json();
        this.diskHash = data.disk_hash || '';
        this.cm = window.CodeMirror(this.$el, {
          value: data.source || '', mode: 'markdown', lineNumbers: true, lineWrapping: true
        });
        var saveBtn = document.createElement('button');
        saveBtn.textContent = 'Save'; saveBtn.className = 'docgen-edit-save';
        var self = this; saveBtn.addEventListener('click', function () { self.save(); });
        this.$el.appendChild(saveBtn);
      },
      async save() {
        var res = await fetch('/__docgen/source', {
          method: 'PUT', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path: this.path, source: this.cm.getValue(), disk_hash: this.diskHash })
        });
        if (res.ok) { var d = await res.json(); this.diskHash = d.disk_hash; this.status = 'saved'; }
        else { this.status = 'error'; }
        // a successful save triggers a server rebuild + SSE reload automatically.
      }
    };
  });
});
```

**Handlers** (`crates/docgen-server/src/handlers.rs`):

`get_source(State, Query{path}) -> Result<Json<SourceResponse>, (StatusCode, Json<ApiError>)>`:
`resolve_doc_path(&state.docs_dir, &path)?` → read file → `disk_hash = sha256_hex(&source)` →
`Json(SourceResponse{...})`. Map `PathGuardError` → status (see B-3 mapping).

`put_source(State, Json<SaveRequest>) -> Result<Json<SaveResponse>, (StatusCode, Json<ApiError>)>`:
1. `let abs = resolve_doc_path(&state.docs_dir, &req.path)?;` (rejects every escape).
2. If `req.disk_hash` is `Some(h)`: read current file, `sha256_hex` it; if `≠ h` → `409 Conflict`
   (`Source changed on disk`) — optimistic concurrency, ported from `saveSourceFile`.
3. `std::fs::write(&abs, &req.source)?`.
4. `rebuild_and_reload(&state)` (best-effort; log on error). This is what makes a save
   auto-reload the browser.
5. `Json(SaveResponse{ path: req.path, disk_hash: sha256_hex(&req.source) })`.

Helper `fn sha256_hex(s: &str) -> String` via `sha2::Sha256`.

**Tests** (`crates/docgen-server/tests/editor.rs`, in-process `oneshot`):

- `get_source_returns_markdown_and_hash`: build state over a tempdir docs with `index.md`;
  `GET /__docgen/source?path=index.md` → 200, JSON `source` matches file, `disk_hash` is the
  sha256.
- **`put_source_persists_in_bounds_write_and_rebuilds`**: `PUT /__docgen/source` with
  `{path:"index.md", source:"# Edited\n\nhi"}` → 200; assert `docs/index.md` on disk now
  contains `# Edited`; assert `out_dir/index/index.html` (after the in-handler rebuild) contains
  `Edited` (proves write → rebuild round-trip); assert a reload was broadcast (subscribe first).
- **`put_source_rejects_traversal`** (endpoint-level security test): `PUT` with
  `{path:"../secret.md", source:"x"}` → `403`; assert no file was created/modified outside docs.
  Also `{path:"/etc/passwd"}` → `400`/`403`, and `{path:"x.txt"}` → `400`.
- `put_source_conflict_on_stale_hash`: `PUT` with a wrong `disk_hash` → `409`, file unchanged.

**Editor css** lives in shared `docgen.css`? No — keep dev-only. Add a tiny
`.docgen-edit-toggle`, `.docgen-editor`, `[x-cloak]{display:none}` block to a NEW dev-only
`crates/docgen-assets/assets/docgen/dev/editor.css`, embedded into `dev_assets()` at
`__docgen/editor.css`, and add its `<link>` to `DEV_HTML`. (Add the path to B-1's `dev_assets()`
list + tests.) This keeps editor styling out of the production `docgen.css`.

**Commit:** `P5(B): editor island + path-guarded get/put source endpoints`

### B-3 — error mapping + dev-only end-to-end gate test

**File:** `crates/docgen-server/src/handlers.rs` — `impl From<PathGuardError>` (or a helper) to
`(StatusCode, Json<ApiError>)`:

| `PathGuardError` | HTTP | message |
| --- | --- | --- |
| `NotMarkdown` | 400 | `path must be a .md file` |
| `Absolute` | 400 | `path must be relative` |
| `Traversal` | 403 | `path must stay under docs` |
| `NotAFile` | 400 | `path must be a regular file` |
| `NotFound` | 404 | `path not found` |

**The dev-only gate integration test** (`crates/docgen-server/tests/dev_only_gating.rs`) — the
capstone proving nothing dev-related ships in a static build:

- `static_build_has_no_editor_or_reload`: run `docgen_build::build_site(Production)` into a
  tempdir; walk every emitted file and assert NONE contains `__docgen/livereload`,
  `docgenEditor`, `__codemirror`, or `EventSource('/__docgen/livereload')`; assert no file path
  contains `__codemirror/` or `__docgen/`; assert `dist/__codemirror/codemirror.js` does not
  exist. (Layer-2 + layer-3 gate, observed end-to-end on disk.)
- `dev_serve_injects_editor_and_reload`: build state, `router`, `oneshot GET /` → body DOES
  contain `__docgen/livereload.js` + `docgenEditor` + `__codemirror/codemirror.js`. (Same
  bytes, opposite expectation — the difference is purely the serve-time injection, proving the
  gate is the injection boundary.)

**Commit:** `P5(B): error mapping + dev-only gating integration tests`

### B-4 — VENDOR.md + docs update

Append the CodeMirror 5 rows to `VENDOR.md` (package/version/source URL/license = MIT) and a
short "Dev-only assets" note: CodeMirror + editor island + live-reload client are emitted ONLY
by `docgen dev`, never by `docgen build` (cite `dev_assets()` + `assets_for` exclusion +
serve-time `inject_dev_html`). Mention the CM5-over-CM6 no-bundler rationale.

**Commit:** `P5(B): document vendored CodeMirror + dev-only asset policy`

---

## 5. Test inventory (what must be green before P5 is "done")

In-process / pure (preferred — no ports, no fs-timing):

1. `resolve_doc_path` — `rejects_all_traversal_vectors` (absolute, `..`, backslash, non-`.md`,
   missing, dir, **symlink escape**) + the in-bounds Ok case. **[path-traversal rejection test]**
2. `inject_dev_html` — inserts before `</body>`; appends when absent.
3. `build_site` — custom out_dir; back-compat dist; **dev≡prod emitted-file set**.
4. `router` — serves injected index; serves raw asset without injection; 404.
5. `rebuild_and_reload` — broadcasts reload; **rebuild regenerates a changed page**
   (direct invocation — the rebuild-on-change test); failed build doesn't broadcast.
6. `dev_bind_addr` — loopback only.
7. `dev_assets` — present + UMD-not-ESM + **`assets_for` never includes dev assets** (8-combo
   matrix).
8. `get_source` / `put_source` — round-trip + **`put_source` rejects traversal** + 409 on stale
   hash + write→rebuild round-trip.
9. Dev-only gating — `static_build_has_no_editor_or_reload`;
   `dev_serve_injects_editor_and_reload`.

Manual / in-browser (architect-validated, documented as such):

- Run `docgen dev` against `fixtures/site-basic`; open `http://127.0.0.1:4321`.
- Edit a `.md` on disk → page auto-reloads (SSE).
- Click "Edit" → CodeMirror mounts with the doc source → edit → Save → file written + page
  reloads with the change.
- Confirm `docgen build` dist has no `__docgen`/`__codemirror` paths.

Run before each commit: `cargo test` (workspace) + `cargo clippy --all-targets`. Both green.

---

## 6. Commit sequence (local only — `overnight/p1-p6`)

```
P5(A): extract reusable build_site into docgen-build crate
P5(A): path-traversal guard + dev-html injection (pure, tested)
P5(A): axum router serving the built site with dev-html injection
P5(A): SSE live-reload endpoint + rebuild_and_reload
P5(A): notify watcher + localhost dev server entry point
P5(A): dev-only live-reload client script
P5(B): vendor CodeMirror 5 (UMD) as dev-only assets, gated out of static emit
P5(B): editor island + path-guarded get/put source endpoints
P5(B): error mapping + dev-only gating integration tests
P5(B): document vendored CodeMirror + dev-only asset policy
```

Commit command (every unit):

```bash
git -c commit.gpgsign=false -c user.name="docgen-rs overnight" \
    -c user.email="g.maxim.stepanoff@gmail.com" commit -m "<msg>"
```

---

## 7. Risks / notes

- **`ServeDir` + HTML rewrite friction.** Mitigated by the explicit serve handler (A-3) that
  reads HTML, injects, and returns; `ServeDir` only handles non-HTML static files.
- **`notify` event storms / editor-save loops.** The editor write triggers
  `rebuild_and_reload` directly AND fires a `notify` event for the same file. Debounce (200 ms)
  collapses the duplicate; a redundant rebuild is harmless (idempotent). Acceptable.
- **Out dir choice.** Prefer a `tempfile::TempDir` owned by `serve` (auto-cleaned) over
  `project_root/.docgen-dev` to avoid polluting the project; if a temp dir complicates
  `ServeDir`, fall back to `project_root/.docgen-dev` (gitignored).
- **`notify-debouncer-mini` version drift.** If 0.4 doesn't resolve against `notify` 6, hand-roll
  a 200 ms debounce (collect events, sleep, drain) — A-5 already allows this shape.
- **CM5 is unmaintained-ish but stable.** Chosen purely because it is the only no-bundler option;
  it is feature-sufficient for a dev markdown editor. If CM6 is ever wanted, it needs a
  vendored pre-bundled IIFE build (out of scope; would be a separate vendoring effort).
- **Production purity is structurally guaranteed**, not merely conventional: `build_site` never
  emits dev assets, `assets_for` never returns them, and injection happens only in the serve
  path. Three independent layers, each with a locking test.
```
