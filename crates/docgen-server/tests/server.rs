//! In-process router + rebuild tests. No ports, no fs-timing — handlers are
//! exercised via `tower::ServiceExt::oneshot`, and rebuild-on-change is driven by
//! invoking `rebuild_and_reload` directly.

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::sync::broadcast;
use tower::ServiceExt;

use docgen_build::{build_site, BuildMode, BuildOptions};
use docgen_server::{rebuild_and_reload, router, AppState, ReloadEvent};

/// Copy the `site-basic` fixture docs into `root/docs`.
fn setup_fixture(root: &Path) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen-server
    let workspace = manifest.parent().unwrap().parent().unwrap();
    let fixture = workspace.join("fixtures/site-basic");
    fs::create_dir_all(root.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), root.join("docs/index.md")).unwrap();
    fs::copy(
        fixture.join("docs/guide/intro.md"),
        root.join("docs/guide/intro.md"),
    )
    .unwrap();
}

/// Build a fixture site + an `AppState` over a fresh tempdir out_dir, with the
/// dev-only assets emitted (mirrors `rebuild_and_reload`'s post-step).
fn state_with_built_site() -> (tempfile::TempDir, tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Dev,
    })
    .unwrap();
    docgen_assets::emit(&docgen_assets::dev_assets(), out.path()).unwrap();

    let docs_dir = root.path().join("docs").canonicalize().unwrap();
    let (reload_tx, _rx) = broadcast::channel(16);
    let state = AppState::new(
        root.path().to_path_buf(),
        out.path().to_path_buf(),
        docs_dir,
        4321,
        reload_tx,
    );
    (root, out, state)
}

#[tokio::test]
async fn serves_built_index_with_injected_dev_html() {
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "127.0.0.1:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(
        html.contains("<title>Home</title>"),
        "not the index: {html}"
    );
    // Serve-time injection added the reload client.
    assert!(html.contains("/__docgen/livereload.js"));
}

#[tokio::test]
async fn serves_static_asset_without_injection() {
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/docgen.css")
                .header("host", "127.0.0.1:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("css"), "unexpected content-type: {ct}");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let css = String::from_utf8(body.to_vec()).unwrap();
    // Injection is HTML-only — css must not carry the reload client.
    assert!(!css.contains("/__docgen/livereload.js"));
}

#[tokio::test]
async fn unknown_path_404() {
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/does/not/exist")
                .header("host", "127.0.0.1:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    // The 404 is the full styled app-shell page (so the user can navigate off via
    // the sidebar/search), with the dev reload client injected — not bare text.
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ct.contains("html"), "404 should be html, got {ct}");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("docgen-sidebar"), "404 lacks the nav shell");
    assert!(html.contains("404"), "404 body missing the 404 marker");
    assert!(
        html.contains("/__docgen/livereload.js"),
        "404 missing dev injection"
    );
}

#[tokio::test]
async fn serves_dev_livereload_client_asset() {
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/__docgen/livereload.js")
                .header("host", "127.0.0.1:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let js = String::from_utf8(body.to_vec()).unwrap();
    assert!(js.contains("EventSource('/__docgen/livereload')"));
}

#[tokio::test]
async fn rejects_foreign_host_header_dns_rebinding() {
    // A DNS-rebinding attacker reaches the loopback bind but the rebound request
    // still carries the attacker hostname in Host -> must be 403, not served.
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "attacker.com:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rejects_missing_host_header() {
    // A request with no Host at all is not a loopback browser request.
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);
    let resp = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn accepts_localhost_host_header() {
    // `localhost:<port>` is an allowed loopback authority alongside 127.0.0.1.
    let (_root, _out, state) = state_with_built_site();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("host", "localhost:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn rejects_cross_site_origin_on_mutating_put() {
    // A PUT with a valid loopback Host but a cross-site Origin (the cross-origin
    // write a rebinding/preflight-skipping attacker would attempt) -> 403.
    let (root, _out, state) = state_with_built_site();
    let secret_before = fs::read_to_string(root.path().join("docs/index.md")).unwrap();
    let app = router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("host", "127.0.0.1:4321")
                .header("origin", "http://attacker.com")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "path": "index.md", "source": "HACKED" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // The write never landed.
    assert_eq!(
        fs::read_to_string(root.path().join("docs/index.md")).unwrap(),
        secret_before
    );
}

#[tokio::test]
async fn livereload_endpoint_streams_reload_event() {
    use tokio::time::{timeout, Duration};

    // The load-bearing wire: GET /__docgen/livereload must be a 200 SSE stream,
    // and a broadcast Reload must arrive as an `event: reload` / `data: now` frame.
    let (_root, _out, state) = state_with_built_site();
    let tx = state.reload_tx.clone();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/__docgen/livereload")
                .header("host", "127.0.0.1:4321")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.contains("text/event-stream"), "not SSE: {ct}");

    // Drive a reload and read the first frame off the (otherwise endless) stream.
    let mut body = resp.into_body().into_data_stream();
    tx.send(ReloadEvent::Reload).unwrap();
    use futures::StreamExt as _;
    let chunk = timeout(Duration::from_secs(2), body.next())
        .await
        .expect("SSE frame timed out")
        .expect("stream ended")
        .expect("stream error");
    let frame = String::from_utf8(chunk.to_vec()).unwrap();
    assert!(frame.contains("event: reload"), "frame: {frame}");
    assert!(frame.contains("data: now"), "frame: {frame}");
}

#[tokio::test]
async fn rebuild_broadcasts_reload() {
    let (_root, _out, state) = state_with_built_site();
    let mut rx = state.reload_tx.subscribe();
    rebuild_and_reload(&state).unwrap();
    assert_eq!(rx.recv().await.unwrap(), ReloadEvent::Reload);
}

#[tokio::test]
async fn rebuild_regenerates_changed_page() {
    let (root, out, state) = state_with_built_site();

    let before = fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert!(before.contains("<title>Home</title>"));

    // Edit the source on disk, then rebuild directly (no watcher, no port).
    fs::write(
        root.path().join("docs/index.md"),
        "---\ntitle: Renamed Home\n---\n\nfresh body\n",
    )
    .unwrap();
    rebuild_and_reload(&state).unwrap();

    let after = fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert!(
        after.contains("Renamed Home"),
        "rebuild did not pick up the edit: {after}"
    );
}

#[tokio::test]
async fn failed_build_does_not_broadcast() {
    let (root, _out, state) = state_with_built_site();
    let mut rx = state.reload_tx.subscribe();

    // Remove the docs dir so discover fails -> hard build error.
    fs::remove_dir_all(root.path().join("docs")).unwrap();
    let res = rebuild_and_reload(&state);
    assert!(res.is_err(), "expected a hard build failure");
    // No reload was delivered for the failed build.
    assert!(rx.try_recv().is_err());
}
