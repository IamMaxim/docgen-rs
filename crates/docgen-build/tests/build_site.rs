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
fn base_subpath_prefixes_assets_and_wikilinks_end_to_end() {
    // A site deployed under `/docs` must emit every asset and link under that
    // prefix; <base> alone does not rewrite root-absolute URLs, so we assert the
    // rendered HTML actually points under /docs (and never at the bare root).
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docgen.toml"),
        "title = \"My Docs\"\nbase = \"/docs\"\n",
    )
    .unwrap();
    fs::write(root.path().join("docs/index.md"), "# Home\n\nSee [[guide]] now.\n").unwrap();
    fs::write(root.path().join("docs/guide.md"), "# Guide\n\nBody.\n").unwrap();
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let html = fs::read_to_string(out.path().join("index/index.html")).unwrap();
    // No <base> tag — links are prefixed directly so they resolve.
    assert!(!html.contains("<base"), "should not emit a <base> tag");
    // Asset under base.
    assert!(html.contains(r#"href="/docs/docgen.css""#), "asset href: {html}");
    assert!(html.contains(r#"src="/docs/bootstrap.js""#));
    // Resolved wikilink under base.
    assert!(
        html.contains(r#"href="/docs/guide""#),
        "wikilink should resolve under /docs: {html}"
    );
    // Nothing left at the bare root.
    assert!(!html.contains(r#"href="/docgen.css""#));
    assert!(!html.contains(r#"href="/guide""#));
    // The client (search.js etc.) learns the base via a JS global.
    assert!(html.contains(r#"window.DOCGEN_BASE = "/docs";"#), "DOCGEN_BASE: {html}");
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
fn failed_build_preserves_last_good_out_dir() {
    // A first successful build, then a build that fails (docs removed) must leave
    // the previous good `out_dir` fully intact — the dev server keeps serving it.
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Dev,
    })
    .unwrap();
    let good = emitted_paths(out.path());
    assert!(out.path().join("index/index.html").is_file());

    // Make the next build fail before/while staging (discover fails on no docs).
    fs::remove_dir_all(root.path().join("docs")).unwrap();
    let res = build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Dev,
    });
    assert!(res.is_err(), "expected the second build to fail");

    // The last good build is untouched: same files, index still served.
    assert!(
        out.path().join("index/index.html").is_file(),
        "out_dir was torn down by a failed build"
    );
    assert_eq!(good, emitted_paths(out.path()));
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

#[test]
fn build_site_emits_a_real_root_index_page() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let root_html = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(root_html.contains("<title>Home</title>"));
    // The nested copy still exists (no link breakage).
    assert!(out.path().join("index/index.html").is_file());
    // Both are byte-identical (same rendered home doc).
    let nested = std::fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert_eq!(root_html, nested);
}

#[test]
fn no_home_doc_means_no_root_index_page() {
    // A site lacking `docs/index.md` writes no `dist/index.html` (no regression:
    // same as pre-P6), while its other pages still build.
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs/guide")).unwrap();
    fs::write(
        root.path().join("docs/guide/intro.md"),
        "# Introduction\nbody\n",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    assert!(!out.path().join("index.html").exists());
    assert!(out.path().join("guide/intro/index.html").is_file());
}

#[test]
fn graph_feature_off_skips_graph_page_and_island() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"), "[features]\ngraph = false\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    assert!(!out.path().join("graph/index.html").exists());
    assert!(!out.path().join("islands/graph.js").exists());
}

#[test]
fn search_feature_off_skips_index_and_client() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"), "[features]\nsearch = false\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    assert!(!out.path().join("search-index.json").exists());
    let home = std::fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(!home.contains("data-docgen-search"));
}

#[test]
fn title_from_config_suffixes_page_titles() {
    let root = tempfile::tempdir().unwrap();
    setup_fixture(root.path());
    std::fs::write(root.path().join("docgen.toml"), "title = \"Acme Docs\"\n").unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    let intro = std::fs::read_to_string(out.path().join("guide/intro/index.html")).unwrap();
    assert!(intro.contains("— Acme Docs</title>"));
}
