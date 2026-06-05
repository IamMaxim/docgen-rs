//! axum handlers + router for the dev server. All dev-only routes are namespaced
//! under `/__docgen/` and `/__codemirror/` so they cannot collide with a doc slug
//! and are trivially greppable as "dev-only surface".

use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
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

// ---- request/response payloads (mirror the original `types.ts`) ----

#[derive(Deserialize)]
pub struct SaveRequest {
    /// docs-relative path, e.g. "guide/intro.md".
    pub path: String,
    pub source: String,
    /// sha256 hex of the source last loaded; optimistic-concurrency guard.
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
        hex.push_str(&format!("{b:02x}"));
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
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/__docgen/livereload", get(livereload_sse))
        .route(
            "/__docgen/source",
            get(get_source).put(put_source),
        )
        .route("/__codemirror/*file", get(serve_dev_asset))
        .route("/__docgen/editor.js", get(serve_dev_asset))
        .route("/__docgen/editor.css", get(serve_dev_asset))
        .route("/__docgen/livereload.js", get(serve_dev_asset))
        .fallback(serve_site)
        .with_state(state)
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
    let source = std::fs::read_to_string(&abs).map_err(|_| {
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
    if let Some(ref expected) = req.disk_hash {
        let current = std::fs::read_to_string(&abs).unwrap_or_default();
        if &sha256_hex(&current) != expected {
            return Err((
                StatusCode::CONFLICT,
                Json(ApiError {
                    error: "source changed on disk".into(),
                }),
            ));
        }
    }

    std::fs::write(&abs, &req.source).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: format!("write failed: {e}"),
            }),
        )
    })?;

    // A successful save triggers a server rebuild + SSE reload (best-effort).
    if let Err(e) = rebuild_and_reload(&state) {
        tracing::error!("rebuild after save failed: {e:#}");
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
        Some(Served::Html(path)) => match std::fs::read_to_string(&path) {
            Ok(body) => Response::builder()
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(crate::inject_dev_html(&body)))
                .unwrap(),
            Err(_) => not_found(),
        },
        Some(Served::Raw(path)) => match std::fs::read(&path) {
            Ok(bytes) => Response::builder()
                .header(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(content_type_for(
                        path.to_str().unwrap_or(""),
                    )),
                )
                .body(Body::from(bytes))
                .unwrap(),
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
