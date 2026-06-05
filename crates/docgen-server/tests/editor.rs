//! In-process editor endpoint tests (B-2): `GET`/`PUT /__docgen/source`. All run
//! via `tower::ServiceExt::oneshot` — no ports, no fs-timing. Covers the round
//! trip, the write -> rebuild -> reload chain, the path-traversal rejection at the
//! endpoint level, and optimistic-concurrency conflict on a stale hash.

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tower::ServiceExt;

use docgen_build::{build_site, BuildMode, BuildOptions};
use docgen_server::{router, AppState, ReloadEvent};

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
    let state = AppState {
        project_root: root.path().to_path_buf(),
        out_dir: out.path().to_path_buf(),
        docs_dir,
        reload_tx,
    };
    (root, out, state)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn get_source_returns_markdown_and_hash() {
    let (root, _out, state) = state_with_built_site();
    let on_disk = fs::read_to_string(root.path().join("docs/index.md")).unwrap();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/__docgen/source?path=index.md")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v = body_json(resp).await;
    assert_eq!(v["source"].as_str().unwrap(), on_disk);
    // disk_hash is the sha256 of the source (64 hex chars).
    assert_eq!(v["disk_hash"].as_str().unwrap().len(), 64);
}

#[tokio::test]
async fn put_source_persists_in_bounds_write_and_rebuilds() {
    let (root, out, state) = state_with_built_site();
    let mut rx = state.reload_tx.subscribe();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": "index.md", "source": "---\ntitle: Edited Title\n---\n\nhi\n" })
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // The write landed on disk inside docs/.
    let on_disk = fs::read_to_string(root.path().join("docs/index.md")).unwrap();
    assert!(on_disk.contains("Edited Title"));

    // The in-handler rebuild regenerated the page.
    let html = fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert!(html.contains("Edited Title"), "rebuild missed the edit: {html}");

    // A reload was broadcast.
    assert_eq!(rx.recv().await.unwrap(), ReloadEvent::Reload);
}

#[tokio::test]
async fn put_source_rejects_traversal() {
    let (root, _out, state) = state_with_built_site();
    // A target OUTSIDE docs/ that the attack would overwrite if the guard failed.
    let secret = root.path().join("secret.md");
    fs::write(&secret, "ORIGINAL").unwrap();
    let app = router(state);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": "../secret.md", "source": "HACKED" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    // Nothing outside docs/ was modified.
    assert_eq!(fs::read_to_string(&secret).unwrap(), "ORIGINAL");

    // Absolute path -> 400.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": "/etc/passwd", "source": "x" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Non-markdown -> 400.
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": "index.txt", "source": "x" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn put_source_conflict_on_stale_hash() {
    let (root, _out, state) = state_with_built_site();
    let app = router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/__docgen/source")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "path": "index.md",
                        "source": "new",
                        "disk_hash": "0000000000000000000000000000000000000000000000000000000000000000"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    // The stale write did not land.
    let on_disk = fs::read_to_string(root.path().join("docs/index.md")).unwrap();
    assert_ne!(on_disk, "new");
}
