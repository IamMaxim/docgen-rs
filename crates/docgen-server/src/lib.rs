//! `docgen-server` is the **dev-only** server behind `docgen dev`: an axum app
//! (bound to `127.0.0.1` only) that serves the built site, watches `docs/` for
//! changes (debounced), rebuilds via [`docgen_build::build_site`], pushes a
//! live-reload signal over SSE, and exposes a path-guarded markdown write
//! endpoint for the in-browser editor.
//!
//! Nothing in this crate ships in a static `docgen build` dist: the editor UI,
//! the reload client, the write/SSE endpoints, and the vendored CodeMirror
//! assets exist ONLY while this server runs.

mod handlers;
mod watch;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use tokio::sync::broadcast;

/// Window after an editor-initiated write during which a watcher event for the
/// same on-disk change is treated as a duplicate and skipped. Must comfortably
/// cover the 200ms watcher debounce.
const SELF_WRITE_SUPPRESS: Duration = Duration::from_millis(750);

/// Dev-server configuration.
pub struct DevOptions {
    pub project_root: PathBuf,
    /// Loopback port. Default 4321.
    pub port: u16,
    /// Open a browser on start (off in tests/CI). Default false.
    pub open: bool,
}

/// One live-reload signal. Carried over the SSE channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadEvent {
    Reload,
}

/// Shared, cheaply-clonable state behind every handler. `Clone` bumps the
/// `Arc`/broadcast handle.
#[derive(Clone)]
pub struct AppState {
    pub project_root: PathBuf,
    pub out_dir: PathBuf,
    /// Canonicalized `docs/` dir — the write-guard root.
    pub docs_dir: PathBuf,
    /// The loopback port the server is bound to. Used by the Host-header
    /// allowlist to defeat DNS-rebinding (see `handlers::loopback_guard`).
    pub port: u16,
    pub reload_tx: broadcast::Sender<ReloadEvent>,
    /// Shared "last editor write" clock (millis since `epoch`) used to suppress
    /// the duplicate watcher rebuild after an in-browser save. `0` = never.
    self_write_at_ms: Arc<AtomicU64>,
    /// Monotonic reference instant for `self_write_at_ms`.
    epoch: Instant,
}

impl AppState {
    /// Construct an `AppState`. Initializes the self-write suppression clock.
    pub fn new(
        project_root: PathBuf,
        out_dir: PathBuf,
        docs_dir: PathBuf,
        port: u16,
        reload_tx: broadcast::Sender<ReloadEvent>,
    ) -> Self {
        Self {
            project_root,
            out_dir,
            docs_dir,
            port,
            reload_tx,
            self_write_at_ms: Arc::new(AtomicU64::new(0)),
            epoch: Instant::now(),
        }
    }

    /// Record that the editor just wrote a doc on disk. The watcher will skip
    /// the next change it sees within [`SELF_WRITE_SUPPRESS`] as a duplicate.
    pub fn note_self_write(&self) {
        // Store elapsed + 1 so the "never written" sentinel (0) is unambiguous
        // even when the write happens at elapsed == 0.
        let ms = self.epoch.elapsed().as_millis() as u64 + 1;
        self.self_write_at_ms.store(ms, Ordering::SeqCst);
    }

    /// Whether a watcher event right now should be suppressed as the echo of a
    /// recent editor write. Consumes the marker so only one rebuild is skipped.
    pub fn take_self_write_suppression(&self) -> bool {
        let marked = self.self_write_at_ms.swap(0, Ordering::SeqCst);
        if marked == 0 {
            return false;
        }
        let now = self.epoch.elapsed().as_millis() as u64 + 1;
        now.saturating_sub(marked) <= SELF_WRITE_SUPPRESS.as_millis() as u64
    }
}

/// Errors from [`resolve_doc_path`]; each maps to an HTTP status (see `handlers`).
#[derive(Debug, PartialEq, Eq)]
pub enum PathGuardError {
    /// Not a `.md` path. (400)
    NotMarkdown,
    /// Absolute path or leading `/`. (400)
    Absolute,
    /// `..` component, backslash, or a realpath that escapes `docs/`. (403)
    Traversal,
    /// Resolves to something that is not a regular file. (400)
    NotAFile,
    /// In-bounds but the file does not exist. (404)
    NotFound,
}

/// Resolve a client-supplied doc-relative path (e.g. `"guide/intro.md"`) to a
/// canonical absolute path strictly inside `docs_dir`, or reject. `docs_dir`
/// MUST already be canonicalized by the caller. Layered checks mirror the
/// original `validateRepoDocPath`:
///
/// 1. backslash -> `Traversal`; absolute / leading `/` -> `Absolute`.
/// 2. strip leading `./`; any `..` component -> `Traversal`; empty -> `Traversal`.
/// 3. require a `.md` suffix -> else `NotMarkdown`.
/// 4. lexical: `docs_dir.join(rel)` must stay under `docs_dir`.
/// 5. `canonicalize()`: missing -> `NotFound`; realpath escaping `docs_dir`
///    (symlink escape) -> `Traversal`.
/// 6. the canonical target must be a regular file -> else `NotAFile`.
pub fn resolve_doc_path(docs_dir: &Path, rel: &str) -> Result<PathBuf, PathGuardError> {
    // (1) gross-shape rejections.
    if rel.contains('\\') {
        return Err(PathGuardError::Traversal);
    }
    if rel.starts_with('/') || Path::new(rel).is_absolute() {
        return Err(PathGuardError::Absolute);
    }

    // (2) normalize + component scan.
    let trimmed = rel.strip_prefix("./").unwrap_or(rel);
    if trimmed.is_empty() {
        return Err(PathGuardError::Traversal);
    }
    let mut kept: Vec<&str> = Vec::new();
    for comp in trimmed.split('/') {
        match comp {
            "" | "." => continue, // collapse `//` and `.` segments
            ".." => return Err(PathGuardError::Traversal),
            other => kept.push(other),
        }
    }
    if kept.is_empty() {
        return Err(PathGuardError::Traversal);
    }
    let normalized = kept.join("/");

    // (3) extension whitelist (markdown-only; the TS guard also allowed `.svx`,
    // which the Rust rewrite does not support).
    if !normalized.ends_with(".md") {
        return Err(PathGuardError::NotMarkdown);
    }

    // (4) lexical containment.
    let candidate = docs_dir.join(&normalized);
    if !candidate.starts_with(docs_dir) {
        return Err(PathGuardError::Traversal);
    }

    // (5) realpath check (catches symlink escapes).
    // Any canonicalize failure (missing path, permission, etc.) is reported as
    // NotFound for a dev write endpoint — the client cannot act on finer detail.
    let canonical = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return Err(PathGuardError::NotFound),
    };
    if !canonical.starts_with(docs_dir) {
        return Err(PathGuardError::Traversal);
    }

    // (6) must be a regular file (not a dir, not a symlink-to-dir).
    let meta = match std::fs::symlink_metadata(&canonical) {
        Ok(m) => m,
        Err(_) => return Err(PathGuardError::NotFound),
    };
    if !meta.is_file() {
        return Err(PathGuardError::NotAFile);
    }

    Ok(canonical)
}

/// The dev-only HTML injected before `</body>` of every served page: the editor
/// css + toggle button + editor island element, the vendored CodeMirror UMD
/// scripts (loaded in dependency order: core -> xml mode -> overlay addon ->
/// markdown mode), the editor island JS, and the live-reload client.
///
/// These non-`defer` scripts execute at parse time — before the page's deferred
/// Alpine script fires `alpine:init` — so `editor.js`'s `docgen.island(...)`
/// registration lands before Alpine runs the registry. Injected ONLY by the dev
/// server (`inject_dev_html`); never written to disk by `docgen build`.
const DEV_HTML: &str = r#"
<link rel="stylesheet" href="/__codemirror/codemirror.css" />
<link rel="stylesheet" href="/__docgen/editor.css" />
<button class="docgen-edit-toggle" data-docgen-edit>Edit</button>
<div id="docgen-editor" x-data="docgenEditor" x-cloak></div>
<script src="/__codemirror/codemirror.js"></script>
<script src="/__codemirror/xml.js"></script>
<script src="/__codemirror/overlay.js"></script>
<script src="/__codemirror/markdown.js"></script>
<script src="/__docgen/editor.js"></script>
<script src="/__docgen/livereload.js"></script>
"#;

/// Post-process a served HTML body: inject the reload-client script + editor
/// toggle + editor island scripts/styles immediately before `</body>`. Dev-only;
/// never run by `docgen build`. Pure string fn so it is unit-testable.
pub fn inject_dev_html(html: &str) -> String {
    match html.rfind("</body>") {
        Some(i) => {
            let mut s = String::with_capacity(html.len() + DEV_HTML.len());
            s.push_str(&html[..i]);
            s.push_str(DEV_HTML);
            s.push_str(&html[i..]);
            s
        }
        // Graceful: append if there is no closing body tag.
        None => format!("{html}{DEV_HTML}"),
    }
}

/// The loopback bind address for the dev server. NEVER `0.0.0.0` — the dev
/// server (editor + write endpoint) must not be reachable off-host.
pub fn dev_bind_addr(port: u16) -> std::net::SocketAddr {
    std::net::SocketAddr::from(([127, 0, 0, 1], port))
}

/// Build the axum router (NO listener) for the given state. Split out so handler
/// tests can `oneshot` requests without binding a port.
pub fn router(state: AppState) -> Router {
    handlers::router(state)
}

/// Rebuild the site into `state.out_dir` (Dev mode + dev-asset emission), then
/// broadcast a reload. Called on every debounced fs change AND after a successful
/// editor write. Returns `Err` only on a hard build failure; the caller logs and
/// keeps serving the last good build. This guarantee holds because
/// [`docgen_build::build_site`] is atomic — it stages into a temp dir and only
/// swaps `out_dir` on full success, so a failed rebuild leaves the served dir
/// (the previous good build) untouched rather than torn down.
pub fn rebuild_and_reload(state: &AppState) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let outcome = docgen_build::build_site(&docgen_build::BuildOptions {
        project_root: &state.project_root,
        out_dir: &state.out_dir,
        mode: docgen_build::BuildMode::Dev,
    })?;
    // The dev-only extra step build_site never performs: emit CodeMirror + the
    // editor island + the reload client into the served dir.
    docgen_assets::emit(&docgen_assets::dev_assets(), &state.out_dir)?;

    // Ignore "no subscribers" — a reload with nobody listening is fine.
    let _ = state.reload_tx.send(ReloadEvent::Reload);
    tracing::info!(
        pages = outcome.page_count,
        elapsed_ms = start.elapsed().as_millis(),
        "rebuilt + reloaded"
    );
    Ok(())
}

/// Run the dev server: initial build, spawn the debounced watcher, bind
/// `127.0.0.1`, serve until Ctrl-C. Blocking entry point the `docgen dev` CLI
/// arm calls. Owns its own tokio runtime so the `docgen` bin's `main` stays a
/// plain `fn main() -> Result<()>`.
pub fn serve(opts: DevOptions) -> anyhow::Result<()> {
    // Idempotent: a second `serve` in-process (tests) won't panic.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_async(opts))
}

async fn serve_async(opts: DevOptions) -> anyhow::Result<()> {
    let project_root = opts.project_root.clone();
    let docs_dir = project_root.join("docs");
    let docs_canon = docs_dir
        .canonicalize()
        .unwrap_or_else(|_| docs_dir.clone());

    // A process-owned output dir; kept alive for the whole run, auto-cleaned.
    let out_tmp = tempfile::tempdir()?;
    let out_dir = out_tmp.path().to_path_buf();

    let (reload_tx, _rx) = broadcast::channel(16);
    let state = AppState::new(
        project_root,
        out_dir,
        docs_canon.clone(),
        opts.port,
        reload_tx,
    );

    // Initial build (Dev mode + dev assets).
    rebuild_and_reload(&state)?;

    // Spawn the debounced fs watcher; it rebuilds + reloads on every change.
    let _watcher = watch::spawn_watcher(state.clone(), &docs_canon)?;

    let addr = dev_bind_addr(opts.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("docgen dev server: http://{addr}");
    if opts.open {
        let _ = open_browser(&format!("http://{addr}"));
    }

    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// Best-effort browser open (dev convenience; failures are non-fatal).
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    #[cfg(windows)]
    let cmd = "explorer";
    std::process::Command::new(cmd).arg(url).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_dev_html_inserts_before_body() {
        let out = inject_dev_html("<html><body><p>hi</p></body></html>");
        // Reload client + editor island + CodeMirror are all injected.
        for marker in [
            "__docgen/livereload.js",
            "docgenEditor",
            "__codemirror/codemirror.js",
            "__docgen/editor.js",
            "data-docgen-edit",
        ] {
            assert!(out.contains(marker), "missing injected marker {marker}");
        }
        // Every injected marker precedes the closing body tag.
        let body = out.rfind("</body>").unwrap();
        for marker in [
            "__docgen/livereload.js",
            "docgenEditor",
            "__codemirror/codemirror.js",
            "__docgen/editor.js",
        ] {
            assert!(out.find(marker).unwrap() < body, "{marker} not before </body>");
        }
        // CodeMirror loads in dependency order: core -> xml -> overlay -> markdown.
        let core = out.find("__codemirror/codemirror.js").unwrap();
        let xml = out.find("__codemirror/xml.js").unwrap();
        let overlay = out.find("__codemirror/overlay.js").unwrap();
        let markdown = out.find("__codemirror/markdown.js").unwrap();
        assert!(core < xml && xml < overlay && overlay < markdown);
    }

    #[test]
    fn inject_dev_html_no_body_appends() {
        let out = inject_dev_html("<p>no body tag here</p>");
        assert!(out.contains("__docgen/livereload.js"));
        assert!(out.contains("docgenEditor"));
    }

    #[test]
    fn self_write_suppression_is_one_shot() {
        let (tx, _rx) = broadcast::channel(4);
        let state = AppState::new(
            PathBuf::from("/x"),
            PathBuf::from("/x/out"),
            PathBuf::from("/x/docs"),
            4321,
            tx,
        );
        // No write yet -> nothing to suppress.
        assert!(!state.take_self_write_suppression());
        // After a self-write, exactly one watcher event is suppressed.
        state.note_self_write();
        assert!(state.take_self_write_suppression());
        assert!(!state.take_self_write_suppression());
    }

    #[test]
    fn bind_addr_is_loopback() {
        assert!(dev_bind_addr(4321).ip().is_loopback());
        assert_eq!(dev_bind_addr(4321).port(), 4321);
    }
}
