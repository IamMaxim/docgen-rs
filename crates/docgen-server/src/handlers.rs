//! axum handlers + router for the dev server. All dev-only routes are namespaced
//! under `/__docgen/` and `/__codemirror/` so they cannot collide with a doc slug
//! and are trivially greppable as "dev-only surface".

use std::convert::Infallible;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, Path as AxumPath, Query, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_stream::wrappers::BroadcastStream;

use crate::{rebuild_and_reload, resolve_doc_path, AppState, PathGuardError, ReloadEvent};

/// Max accepted body for `PUT /__docgen/source`. Generous for a markdown source
/// file, but bounded + explicit so the limit survives extractor refactors
/// instead of silently relying on axum's implicit 2 MiB default.
const MAX_SOURCE_BODY: usize = 8 * 1024 * 1024;

// ---- request/response payloads (mirror the original `types.ts`) ----

#[derive(Deserialize)]
pub struct SaveRequest {
    /// docs-relative path, e.g. "guide/intro.md".
    pub path: String,
    pub source: String,
    /// sha256 hex of the source last loaded; optimistic-concurrency guard.
    /// When omitted, the write is an **intentional force-write** (no stale-write
    /// check). The bundled editor always sends it; a direct caller may omit it
    /// to deliberately clobber concurrent on-disk changes.
    #[serde(default)]
    pub disk_hash: Option<String>,
}

#[derive(Serialize)]
pub struct SaveResponse {
    pub path: String,
    pub disk_hash: String,
}

#[derive(Serialize)]
pub struct SourceResponse {
    pub path: String,
    pub source: String,
    pub disk_hash: String,
    /// The file's content at git HEAD, for the editor's merge-against-HEAD
    /// gutter. `""` outside a repo / untracked file.
    pub head_source: String,
}

#[derive(Deserialize)]
pub struct PreviewRequest {
    /// docs-relative path (used only to validate the edit target exists).
    pub path: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct PreviewResponse {
    /// Rendered markdown HTML for the live preview pane.
    pub html: String,
}

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Deserialize)]
pub struct SourceQuery {
    pub path: String,
}

/// sha256 hex of a string (optimistic-concurrency token + load hash).
pub(crate) fn sha256_hex(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let out = h.finalize();
    let mut hex = String::with_capacity(out.len() * 2);
    for b in out {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

/// Map a path-guard rejection to an HTTP status + message.
fn guard_response(e: PathGuardError) -> (StatusCode, Json<ApiError>) {
    let (status, msg) = match e {
        PathGuardError::NotMarkdown => (StatusCode::BAD_REQUEST, "path must be a .md file"),
        PathGuardError::Absolute => (StatusCode::BAD_REQUEST, "path must be relative"),
        PathGuardError::Traversal => (StatusCode::FORBIDDEN, "path must stay under docs"),
        PathGuardError::NotAFile => (StatusCode::BAD_REQUEST, "path must be a regular file"),
        PathGuardError::NotFound => (StatusCode::NOT_FOUND, "path not found"),
    };
    (
        status,
        Json(ApiError {
            error: msg.to_string(),
        }),
    )
}

/// Build the dev router: dev-only routes + the static-serving fallback.
///
/// Every request first passes through [`loopback_guard`], which enforces a
/// Host/Origin allowlist so a DNS-rebinding site cannot reach the mutating
/// write endpoint even though the bind is already loopback-only.
pub fn router(state: AppState) -> Router {
    let port = state.port;
    Router::new()
        .route("/__docgen/livereload", get(livereload_sse))
        .route(
            "/__docgen/source",
            get(get_source)
                .put(put_source)
                .layer(DefaultBodyLimit::max(MAX_SOURCE_BODY)),
        )
        .route(
            "/__docgen/preview",
            post(post_preview).layer(DefaultBodyLimit::max(MAX_SOURCE_BODY)),
        )
        .route("/edit/*slug", get(serve_editor_page))
        .route("/__docgen/editor-cm6.js", get(serve_dev_asset))
        .route("/__docgen/editor.css", get(serve_dev_asset))
        .route("/__docgen/livereload.js", get(serve_dev_asset))
        .fallback(serve_site)
        .layer(middleware::from_fn(move |req, next| {
            loopback_guard(port, req, next)
        }))
        .with_state(state)
}

/// Host/Origin allowlist enforced on every request. The loopback bind already
/// blocks off-host TCP, but it does NOT stop a browser-mediated DNS-rebinding
/// attack: a remote page whose hostname is rebound to `127.0.0.1` would
/// otherwise reach this dev server (and its markdown write endpoint) as
/// "same-origin". The rebound request still carries the attacker's hostname in
/// `Host`, so allowlisting `Host` to the loopback authorities closes it.
///
///  * `Host` must be exactly `127.0.0.1:<port>` or `localhost:<port>` -> else 403.
///  * On mutating verbs (anything but GET/HEAD), if an `Origin` header is
///    present it must be a loopback origin -> else 403. (Same-origin/no-CORS
///    requests omit `Origin`; a cross-site request that sets it is rejected.)
async fn loopback_guard(port: u16, req: Request, next: Next) -> Response {
    let allowed_hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| allowed_hosts.iter().any(|a| a == h))
        .unwrap_or(false);
    if !host_ok {
        return (StatusCode::FORBIDDEN, "forbidden host").into_response();
    }

    let is_mutating = !matches!(req.method(), &Method::GET | &Method::HEAD);
    if is_mutating {
        if let Some(origin) = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|o| o.to_str().ok())
        {
            let allowed_origins = [
                format!("http://127.0.0.1:{port}"),
                format!("http://localhost:{port}"),
            ];
            if !allowed_origins.iter().any(|a| a == origin) {
                return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
            }
        }
    }

    next.run(req).await
}

// ---- SSE live-reload ----

async fn livereload_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.reload_tx.subscribe();
    let stream = BroadcastStream::new(rx)
        // End the stream on shutdown so axum's graceful drain doesn't hang on this
        // keep-alive connection (the root cause of Ctrl+C not stopping `docgen dev`).
        .take_while(|ev| std::future::ready(!matches!(ev, Ok(ReloadEvent::Shutdown))))
        .filter_map(|ev| async move {
            match ev {
                Ok(ReloadEvent::Reload) => Some(Ok(Event::default().event("reload").data("now"))),
                // Shutdown is consumed by take_while above; lagged is skipped.
                Ok(ReloadEvent::Shutdown) | Err(_) => None,
            }
        });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

// ---- editor source endpoints ----

async fn get_source(
    State(state): State<AppState>,
    Query(q): Query<SourceQuery>,
) -> Result<Json<SourceResponse>, (StatusCode, Json<ApiError>)> {
    let abs = resolve_doc_path(&state.docs_dir, &q.path).map_err(guard_response)?;
    let source = tokio::fs::read_to_string(&abs).await.map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "path not found".into(),
            }),
        )
    })?;
    let disk_hash = sha256_hex(&source);
    let head_source = docgen_diff::head_source(&state.docs_dir, &q.path).unwrap_or_default();
    Ok(Json(SourceResponse {
        path: q.path,
        source,
        disk_hash,
        head_source,
    }))
}

/// Render the editor's live preview through the EXACT pipeline `docgen build`
/// runs for a published page — not a bare markdown render. The live `source` is
/// prepared (frontmatter stripped, title derived), rendered against the whole
/// site's slug set (so `[[wikilinks]]` resolve), with directives/components,
/// math, and mermaid all expanded, then wrapped in a content-only document that
/// loads the same CSS + island stack a built page uses. The result is fed into an
/// `<iframe srcdoc>` in the editor, so the preview hydrates identically to the
/// real page. No disk write. (Path validated so only editable docs preview.)
async fn post_preview(
    State(state): State<AppState>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<PreviewResponse>, (StatusCode, Json<ApiError>)> {
    resolve_doc_path(&state.docs_dir, &req.path).map_err(guard_response)?;

    // The render is CPU/disk-bound (reads every doc for the slug set, runs comrak +
    // syntect): run it off the async worker, mirroring the save-rebuild handler.
    let result = tokio::task::spawn_blocking(move || {
        render_preview_document(&state.project_root, &state.docs_dir, &req.path, &req.source)
    })
    .await;

    match result {
        Ok(Ok(html)) => Ok(Json(PreviewResponse { html })),
        Ok(Err(e)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("preview render failed: {e:#}"),
            }),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("preview task panicked: {e}"),
            }),
        )),
    }
}

/// Synchronously render one doc's live `source` to the content-only preview
/// document, reconstructing the build-time inputs at serve time: the whole-site
/// slug set (from `docs/`), the loaded `docgen.toml`, and the component registry.
/// `doc_rel_path` is the docs-relative path of the doc being edited (e.g.
/// `guide/intro.md`); it drives the previewed doc's slug + title.
fn render_preview_document(
    project_root: &Path,
    docs_dir: &Path,
    doc_rel_path: &str,
    source: &str,
) -> anyhow::Result<String> {
    use docgen_core::discover::discover_docs;
    use docgen_core::model::RawDoc;
    use docgen_core::pipeline::{prepare, render_doc};
    use docgen_core::wikilink::SlugSet;

    // Whole-site slug set so `[[wikilinks]]` in the edited doc resolve against
    // every other doc, exactly as the build does. Built from the on-disk docs;
    // the edited doc's own slug is already among them (same file path).
    let raws = discover_docs(docs_dir)?;
    let slugs: SlugSet = raws.iter().map(|r| prepare(r.clone()).slug).collect();
    // Include-only partials (`_*.md`), so `:include` resolves in live preview too.
    let (_pages, partials) = docgen_core::pipeline::partition_partials(raws.clone());

    // Same config + component registry the build assembles (built-ins overridden
    // by the project `components/` dir). Mirrors `docgen-build::build_site`.
    let config = docgen_config::load(project_root)?;
    let builtins: Vec<docgen_components::Component> = docgen_assets::builtin_components()
        .into_iter()
        .map(|b| {
            docgen_components::Component::from_parts(
                b.name,
                b.template,
                b.island_js.map(str::to_string),
                b.style_css.map(str::to_string),
            )
        })
        .collect();
    let components_dir = project_root.join(&config.components.dir);
    let registry = docgen_components::build_registry(builtins, &components_dir)?;

    // Render the LIVE buffer (not the on-disk file) through the shared per-doc
    // pipeline — this is the "same roof" as the build.
    let prepared = prepare(RawDoc {
        rel_path: doc_rel_path.to_string(),
        raw: source.to_string(),
    });
    let rendered = render_doc(&prepared, &config, &registry, &slugs, &partials);

    // Per-page asset gating, mirroring the build's page render.
    let has_components_css = !registry.styles().is_empty();
    let island_components: std::collections::BTreeSet<String> = registry
        .islands()
        .iter()
        .filter_map(|c| c.island_js.as_ref().map(|_| c.name.clone()))
        .collect();
    let has_component_island = rendered
        .doc
        .components_used
        .iter()
        .any(|c| island_components.contains(c));

    let renderer = docgen_render::Renderer::new(docgen_render::DEFAULT_PAGE_TEMPLATE)?;
    let html = renderer.render_preview(&docgen_render::PreviewContext {
        title: &prepared.title,
        body_html: &rendered.doc.body_html,
        base: &config.base,
        has_mermaid: rendered.doc.has_mermaid,
        has_math: rendered.doc.has_math,
        has_components_css,
        has_component_island,
    })?;
    Ok(html)
}

/// Serve the dev-only full-page split editor at `/edit/<slug>`. The page is a
/// thin shell: a mount element carrying the doc path/title + the vendored CM6
/// editor bundle and its stylesheet. All data flows through the `source` /
/// `preview` endpoints. 404s when the slug doesn't resolve to an editable doc.
async fn serve_editor_page(
    State(state): State<AppState>,
    AxumPath(slug): AxumPath<String>,
) -> Response {
    let slug = slug.trim_matches('/');
    let doc_path = format!("{slug}.md");
    // Validate the target exists + stays under docs (same guard as the API).
    let abs = match resolve_doc_path(&state.docs_dir, &doc_path) {
        Ok(p) => p,
        Err(_) => return not_found(&state).await,
    };
    // Title = the doc's first `# ` heading, else the slug's last segment.
    let title = tokio::fs::read_to_string(&abs)
        .await
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
        })
        .unwrap_or_else(|| slug.rsplit('/').next().unwrap_or(slug).to_string());

    let html = editor_page_html(slug, &doc_path, &title);
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response()
}

/// The site topbar for the editor page: brand + diff link + working theme toggle,
/// matching the published `page.html` topbar so the editor lives under the same
/// chrome. The theme toggle is the real `docgenThemeToggle` Alpine island (loaded
/// below); the `.doc-editor` shell already reserves `calc(100vh - --topbar-height)`
/// for exactly this header. Kept as a const so `format!` doesn't choke on the
/// Alpine `{ ... }` class bindings.
const EDITOR_TOPBAR: &str = r#"<header class="docgen-topbar">
  <a class="docgen-topbar__brand" href="/">
    <span class="docgen-brand-mark" aria-hidden="true"></span>
    <span class="docgen-brand-name">Docs</span>
  </a>
  <div class="docgen-topbar__main">
    <div class="docgen-topbar__actions">
      <div class="docgen-btn-strip" role="group" aria-label="Layout">
        <a class="docgen-ctl--diff icon-only" href="/diff" aria-label="Show documentation diff" title="Show documentation diff">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 4v16M18 4v16"/><path d="M9 8h6M9 16h6M12 5v6M12 13v6"/></svg>
        </a>
      </div>
      <div class="docgen-theme-toggle" x-data="docgenThemeToggle" role="tablist" aria-label="Theme">
        <button type="button" class="docgen-theme-toggle__btn" :class="{ 'is-active': theme==='dark' }" :aria-pressed="theme==='dark'" @click="set('dark')" aria-label="Dark">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>
        </button>
        <button type="button" class="docgen-theme-toggle__btn" :class="{ 'is-active': theme==='light' }" :aria-pressed="theme==='light'" @click="set('light')" aria-label="Light">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>
        </button>
      </div>
    </div>
  </div>
</header>"#;

/// Build the editor page shell. Dev-only; never written by `docgen build`.
fn editor_page_html(slug: &str, doc_path: &str, title: &str) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let view_path = format!("/{slug}");
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>Editing: {title} — docgen</title>
<script>(function(){{try{{var s=localStorage.getItem('doc-theme');var t=s||(matchMedia('(prefers-color-scheme: light)').matches?'light':'dark');document.documentElement.setAttribute('data-theme',t);}}catch(e){{}}}})();</script>
<link rel="stylesheet" href="/docgen.css" />
<link rel="stylesheet" href="/code.css" />
<link rel="stylesheet" href="/__docgen/editor.css" />
</head>
<body class="docgen-app">
{topbar}
<div id="docgen-editor-app" data-doc-path="{doc_path}" data-view-path="{view_path}" data-title="{title}" data-base=""></div>
<script>window.DOCGEN_BASE = "";</script>
<script src="/bootstrap.js"></script>
<script src="/islands/theme-toggle.js"></script>
<script src="/__docgen/editor-cm6.js"></script>
<script src="/vendor/alpine/alpine.min.js" defer></script>
</body>
</html>
"#,
        title = esc(title),
        doc_path = esc(doc_path),
        view_path = esc(&view_path),
        topbar = EDITOR_TOPBAR,
    )
}

async fn put_source(
    State(state): State<AppState>,
    Json(req): Json<SaveRequest>,
) -> Result<Json<SaveResponse>, (StatusCode, Json<ApiError>)> {
    let abs = resolve_doc_path(&state.docs_dir, &req.path).map_err(guard_response)?;

    // Optimistic concurrency: reject if the file changed on disk since load.
    // An absent hash is an intentional force-write (see `SaveRequest::disk_hash`).
    if let Some(ref expected) = req.disk_hash {
        let current = tokio::fs::read_to_string(&abs).await.unwrap_or_default();
        if &sha256_hex(&current) != expected {
            return Err((
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: "source changed on disk".into(),
                }),
            ));
        }
    }

    tokio::fs::write(&abs, &req.source).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("write failed: {e}"),
            }),
        )
    })?;

    // Mark this as an editor-initiated write so the fs watcher skips the
    // duplicate rebuild it would otherwise fire for the same on-disk change.
    state.note_self_write();

    // A successful save triggers a server rebuild + SSE reload (best-effort).
    // build_site is CPU/disk/git-bound and fully synchronous, so run it off the
    // async worker via spawn_blocking rather than stalling the runtime.
    let st = state.clone();
    let rebuild = tokio::task::spawn_blocking(move || rebuild_and_reload(&st)).await;
    match rebuild {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::error!("rebuild after save failed: {e:#}"),
        Err(e) => tracing::error!("rebuild task panicked: {e}"),
    }

    let disk_hash = sha256_hex(&req.source);
    Ok(Json(SaveResponse {
        path: req.path,
        disk_hash,
    }))
}

// ---- dev-only static assets (CodeMirror + editor + reload client) ----

async fn serve_dev_asset(uri: axum::http::Uri) -> Response {
    let req_path = uri.path().trim_start_matches('/');
    for a in docgen_assets::dev_assets() {
        if a.path == req_path {
            let ct = content_type_for(a.path);
            return ([(header::CONTENT_TYPE, ct)], a.bytes).into_response();
        }
    }
    (StatusCode::NOT_FOUND, "not found").into_response()
}

// ---- static site serving with dev-html injection ----

async fn serve_site(State(state): State<AppState>, uri: axum::http::Uri) -> Response {
    // `uri.path()` is percent-encoded (a browser sends `/café` as `/caf%C3%A9`);
    // decode it so non-ASCII slugs match the Unicode dirs the build wrote on disk.
    let decoded = percent_encoding::percent_decode_str(uri.path()).decode_utf8_lossy();
    // The built HTML prefixes URLs with `base`; strip it before resolving against
    // `out_dir` (where assets/pages live without the base prefix).
    let path = crate::strip_base(&decoded, &state.base);
    let rel = path.trim_start_matches('/');
    match resolve_served_file(&state.out_dir, rel) {
        Some(Served::Html(path)) => match tokio::fs::read_to_string(&path).await {
            Ok(body) => (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                crate::inject_dev_html(&body),
            )
                .into_response(),
            Err(_) => not_found(&state).await,
        },
        Some(Served::Raw(path)) => match tokio::fs::read(&path).await {
            Ok(bytes) => (
                [(
                    header::CONTENT_TYPE,
                    content_type_for(path.to_str().unwrap_or("")),
                )],
                bytes,
            )
                .into_response(),
            Err(_) => not_found(&state).await,
        },
        None => not_found(&state).await,
    }
}

enum Served {
    Html(PathBuf),
    Raw(PathBuf),
}

/// Resolve a request path to a file in `out_dir`, honoring clean URLs
/// (`/guide/intro` -> `out_dir/guide/intro/index.html`). Returns `None` for a
/// miss. Never escapes `out_dir`.
fn resolve_served_file(out_dir: &Path, rel: &str) -> Option<Served> {
    // Reject traversal in served paths defensively.
    if rel.split('/').any(|c| c == "..") {
        return None;
    }
    let trimmed = rel.trim_matches('/');

    // Direct file hit (assets like docgen.css, bootstrap.js, search-index.json).
    if !trimmed.is_empty() {
        let direct = out_dir.join(trimmed);
        if direct.is_file() {
            return Some(if is_html(&direct) {
                Served::Html(direct)
            } else {
                Served::Raw(direct)
            });
        }
    }

    // Clean-URL directory index: `/` or `/guide/intro` -> `<dir>/index.html`.
    let index = if trimmed.is_empty() {
        out_dir.join("index.html")
    } else {
        out_dir.join(trimmed).join("index.html")
    };
    if index.is_file() {
        return Some(Served::Html(index));
    }

    // Bare `/` with no top-level index.html: the build emits `index/index.html`
    // for the `index` slug, so map `/` there.
    if trimmed.is_empty() {
        let slug_index = out_dir.join("index").join("index.html");
        if slug_index.is_file() {
            return Some(Served::Html(slug_index));
        }
    }
    None
}

fn is_html(path: &Path) -> bool {
    path.extension().map(|e| e == "html").unwrap_or(false)
}

/// 404 response. Serves the build-emitted `404.html` (full app shell + sidebar,
/// dev HTML injected) with a 404 status so a miss lands somewhere navigable;
/// falls back to bare text if the page isn't on disk (e.g. a build that failed
/// before emitting it).
async fn not_found(state: &AppState) -> Response {
    let page = state.out_dir.join("404.html");
    match tokio::fs::read_to_string(&page).await {
        Ok(body) => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            crate::inject_dev_html(&body),
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

fn content_type_for(path: &str) -> &'static str {
    // Match on a lowercased extension so `IMAGE.PNG` / `Photo.JPG` are typed
    // correctly, not just their all-lowercase forms.
    let path = &path.to_ascii_lowercase();
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".gif") {
        "image/gif"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else if path.ends_with(".avif") {
        "image/avif"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".pdf") {
        "application/pdf"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::content_type_for;

    #[test]
    fn images_get_correct_mime_types() {
        assert_eq!(content_type_for("a/b/image.png"), "image/png");
        assert_eq!(content_type_for("photo.jpg"), "image/jpeg");
        assert_eq!(content_type_for("photo.jpeg"), "image/jpeg");
        assert_eq!(content_type_for("anim.gif"), "image/gif");
        assert_eq!(content_type_for("pic.webp"), "image/webp");
        assert_eq!(content_type_for("pic.avif"), "image/avif");
        assert_eq!(content_type_for("favicon.ico"), "image/x-icon");
        assert_eq!(content_type_for("doc.pdf"), "application/pdf");
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        assert_eq!(content_type_for("IMAGE.PNG"), "image/png");
        assert_eq!(content_type_for("Photo.JPG"), "image/jpeg");
    }

    #[test]
    fn unknown_extension_falls_back_to_octet_stream() {
        assert_eq!(content_type_for("data.bin"), "application/octet-stream");
    }
}
