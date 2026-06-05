//! Capstone gating tests (B-3): prove that NOTHING dev-related (editor, reload
//! client, CodeMirror) ships in a static `docgen build` dist, while the SAME built
//! bytes DO carry the editor + reload markup once served through the dev server.
//! The only difference is the serve-time `inject_dev_html` boundary.

use std::fs;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tokio::sync::broadcast;
use tower::ServiceExt;

use docgen_build::{build_site, BuildMode, BuildOptions};
use docgen_server::{router, AppState};

fn setup_fixture(root: &Path) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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

/// Recursively collect every file path under `dir`.
fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Markers that must NEVER appear in a static production dist.
const DEV_MARKERS: &[&str] = &[
    "__docgen/livereload",
    "__docgen/editor",
    "__codemirror",
    "docgenEditor",
    "EventSource('/__docgen/livereload')",
    "data-docgen-edit",
];

#[test]
fn static_build_has_no_editor_or_reload() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();

    // A pure production build — exactly what `docgen build` runs.
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let mut files = Vec::new();
    walk(out.path(), &mut files);
    assert!(!files.is_empty(), "build emitted nothing");

    for f in &files {
        // No dev-namespaced paths on disk.
        let rel = f.strip_prefix(out.path()).unwrap().to_string_lossy().to_string();
        assert!(
            !rel.contains("__codemirror") && !rel.contains("__docgen"),
            "dev path leaked into static dist: {rel}"
        );
        // No dev markers inside any text file's content.
        if let Ok(text) = fs::read_to_string(f) {
            for m in DEV_MARKERS {
                assert!(
                    !text.contains(m),
                    "dev marker {m:?} leaked into static dist file {rel}"
                );
            }
        }
    }

    // The vendored CodeMirror is specifically absent.
    assert!(!out.path().join("__codemirror/codemirror.js").exists());
    assert!(!out.path().join("__docgen/editor.js").exists());
}

#[tokio::test]
async fn dev_serve_injects_editor_and_reload() {
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

    let resp = router(state)
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(body.to_vec()).unwrap();

    // The serve-time injection added the dev surface to the SAME built bytes.
    for m in [
        "__docgen/livereload.js",
        "__docgen/editor.js",
        "__codemirror/codemirror.js",
        "docgenEditor",
        "data-docgen-edit",
    ] {
        assert!(html.contains(m), "dev serve missing injected marker {m}");
    }
}
