use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use docgen_build::{build, build_site, BuildMode, BuildOptions};

/// Copy the checked-in `fixtures/site-basic` docs into `root/docs`.
fn setup_fixture(root: &Path) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen-build
    let workspace = manifest.parent().unwrap().parent().unwrap(); // repo root
    let fixture = workspace.join("fixtures/site-basic");

    fs::create_dir_all(root.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), root.join("docs/index.md")).unwrap();
    fs::copy(
        fixture.join("docs/guide/intro.md"),
        root.join("docs/guide/intro.md"),
    )
    .unwrap();
    fs::copy(fixture.join("docs/markup.md"), root.join("docs/markup.md")).unwrap();
}

/// Collect every emitted file path, relative to `out_dir`.
fn emitted_paths(out_dir: &Path) -> BTreeSet<String> {
    fn walk(base: &Path, dir: &Path, acc: &mut BTreeSet<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, acc);
            } else {
                acc.insert(
                    path.strip_prefix(base)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut acc = BTreeSet::new();
    walk(out_dir, out_dir, &mut acc);
    acc
}

#[test]
fn build_site_writes_pages_to_custom_out_dir() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();

    let outcome = build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    // Output landed in the arbitrary out dir, not root/dist.
    assert_eq!(outcome.out_dir, out.path());
    assert!(out.path().join("index/index.html").is_file());
    assert!(out.path().join("guide/intro/index.html").is_file());
    assert!(out.path().join("search-index.json").is_file());
    assert!(out.path().join("bootstrap.js").is_file());
    assert!(out.path().join("graph/index.html").is_file());
    assert!(!root.path().join("dist").exists());
}

#[test]
fn build_compat_wrapper_writes_dist() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());

    let outcome = build(root.path()).unwrap();

    assert_eq!(outcome.out_dir, root.path().join("dist"));
    assert!(root.path().join("dist/index/index.html").is_file());
    assert!(root.path().join("dist/search-index.json").is_file());
}

#[test]
fn dev_and_production_modes_emit_identical_files() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());

    let prod_out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: prod_out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let dev_out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: dev_out.path(),
        mode: BuildMode::Dev,
    })
    .unwrap();

    // build_site itself is dev-asset-free: the emitted file SET is identical in
    // both modes (gate 0.3 at the build level). The dev server adds dev assets
    // AFTER build_site returns, never inside it.
    assert_eq!(emitted_paths(prod_out.path()), emitted_paths(dev_out.path()));
}
