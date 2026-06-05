//! axum handlers + router for the dev server. All dev-only routes are namespaced
//! under `/__docgen/` and `/__codemirror/` so they cannot collide with a doc slug
//! and are trivially greppable as "dev-only surface".

use std::convert::Infallible;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::{
    extract::{DefaultBodyLimit, Query, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::get,
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
        .route("/__codemirror/*file", get(serve_dev_asset))
        .route("/__docgen/editor.js", get(serve_dev_asset))
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
        if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
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
    let stream = BroadcastStream::new(rx).filter_map(|ev| async move {
        match ev {
            Ok(ReloadEvent::Reload) => {
                Some(Ok(Event::default().event("reload").data("now")))
            }
            Err(_) => None, // lagged: skip, the next reload still fires
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
    Ok(Json(SourceResponse {
        path: q.path,
        source,
        disk_hash,
    }))
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
    let rel = uri.path().trim_start_matches('/');
    match resolve_served_file(&state.out_dir, rel) {
        Some(Served::Html(path)) => match tokio::fs::read_to_string(&path).await {
            Ok(body) => (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                crate::inject_dev_html(&body),
            )
                .into_response(),
            Err(_) => not_found(),
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
            Err(_) => not_found(),
        },
        None => not_found(),
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

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn content_type_for(path: &str) -> &'static str {
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
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}
