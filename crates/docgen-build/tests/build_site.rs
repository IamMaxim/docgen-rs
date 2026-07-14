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
    fs::write(
        root.path().join("docs/index.md"),
        "# Home\n\nSee [[guide]] now.\n",
    )
    .unwrap();
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
    assert!(
        html.contains(r#"href="/docs/docgen.css""#),
        "asset href: {html}"
    );
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
    assert!(
        html.contains(r#"window.DOCGEN_BASE = "/docs";"#),
        "DOCGEN_BASE: {html}"
    );
}

#[test]
fn relative_image_is_copied_and_url_rewritten_end_to_end() {
    // The reported bug: a page at docs/system/index.md references an image with a
    // relative path (./attachments/image.png). The image must be copied into the
    // output mirroring the docs tree, and the rendered <img> src must point at it
    // absolutely (surviving the clean-URL `/system/index/` nesting).
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs/system/attachments")).unwrap();
    fs::write(
        root.path().join("docs/system/index.md"),
        "# System\n\n![Diagram](./attachments/image.png)\n\n[spec](./attachments/spec.pdf)\n",
    )
    .unwrap();
    let png = b"\x89PNG\r\n\x1a\n";
    fs::write(root.path().join("docs/system/attachments/image.png"), png).unwrap();
    fs::write(
        root.path().join("docs/system/attachments/spec.pdf"),
        b"%PDF-1.4",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    // (1) The asset was copied mirroring the docs tree.
    let copied = out.path().join("system/attachments/image.png");
    assert!(copied.is_file(), "image should be copied to {copied:?}");
    assert_eq!(fs::read(&copied).unwrap(), png, "copied bytes must match");
    assert!(out.path().join("system/attachments/spec.pdf").is_file());

    // (2) The page (served at /system/index/) references the image absolutely, so
    // the browser requests /system/attachments/image.png — where the file landed.
    let html = fs::read_to_string(out.path().join("system/index/index.html")).unwrap();
    assert!(
        html.contains(r#"src="/system/attachments/image.png""#),
        "img src should be base-absolute: {html}"
    );
    assert!(
        html.contains(r#"href="/system/attachments/spec.pdf""#),
        "asset link should be base-absolute: {html}"
    );
    // The un-rewritten relative form must not survive.
    assert!(!html.contains(r#"src="./attachments/image.png""#));
}

#[test]
fn relative_md_link_between_pages_resolves_to_clean_url_end_to_end() {
    // A standard markdown link between pages: docs/system/index.md links to a
    // sibling page via `[Other](./other.md)`. Served at /system/index/, the raw
    // relative href would resolve to the wrong place; it must be rewritten to the
    // target page's clean URL (/system/other). A link to a non-existent page is
    // left untouched.
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs/system")).unwrap();
    fs::write(
        root.path().join("docs/system/index.md"),
        "# System\n\n[Other](./other.md) and [Missing](./ghost.md)\n",
    )
    .unwrap();
    fs::write(
        root.path().join("docs/system/other.md"),
        "# Other\n\nBody.\n",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let html = fs::read_to_string(out.path().join("system/index/index.html")).unwrap();
    assert!(
        html.contains(r#"href="/system/other""#),
        "known page link should resolve to its clean URL: {html}"
    );
    assert!(
        !html.contains(r#"href="./other.md""#),
        "raw relative page link must not survive: {html}"
    );
    // The link to a page that doesn't exist is left as the author wrote it.
    assert!(
        html.contains(r#"href="./ghost.md""#),
        "unknown page link should be untouched: {html}"
    );
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
fn dev_emits_no_dev_only_surface_only_the_mermaid_superset() {
    // The fixture has NO mermaid diagram, so production omits the mermaid runtime.
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

    let prod = emitted_paths(prod_out.path());
    let dev = emitted_paths(dev_out.path());

    // build_site emits NO dev-only surface (editor/livereload) in either mode —
    // the dev server layers that on AFTER build_site returns. The ONLY difference
    // is the mermaid runtime, which dev ships unconditionally so the editor
    // preview can render a just-typed diagram (production gates it on usage).
    let only_in_dev: std::collections::BTreeSet<_> = dev.difference(&prod).cloned().collect();
    let only_in_prod: std::collections::BTreeSet<_> = prod.difference(&dev).cloned().collect();
    assert!(
        only_in_prod.is_empty(),
        "production emitted files dev lacks: {only_in_prod:?}"
    );
    assert_eq!(
        only_in_dev,
        ["islands/mermaid.js", "vendor/mermaid/mermaid.min.js"]
            .iter()
            .map(|s| s.to_string())
            .collect::<std::collections::BTreeSet<_>>(),
        "dev's only extra over production must be the mermaid runtime"
    );
    // No dev-only editor/livereload surface leaked into either on-disk build.
    for p in dev.iter().chain(prod.iter()) {
        assert!(!p.contains("__docgen") && !p.contains("editor") && !p.contains("livereload"));
    }
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
    std::fs::write(
        root.path().join("docgen.toml"),
        "[features]\ngraph = false\n",
    )
    .unwrap();
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
    std::fs::write(
        root.path().join("docgen.toml"),
        "[features]\nsearch = false\n",
    )
    .unwrap();
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
fn s3_fallback_and_dev_mode_never_upload() {
    // Both halves of this test manipulate the process-global AWS_* env vars, and
    // Rust runs tests in the same binary concurrently. Merging them into one
    // test (run sequentially within a single function) avoids racing against
    // any other test that might touch these same vars.

    // --- Part 1: no credentials -> Production build falls back to local copy.
    // Guard against ambient AWS creds in the dev environment leaking in and
    // flipping this into a real-upload attempt.
    std::env::remove_var("AWS_ACCESS_KEY_ID");
    std::env::remove_var("AWS_SECRET_ACCESS_KEY");

    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs/system")).unwrap();
    fs::write(
        root.path().join("docs/system/index.md"),
        "# System\n\n![](./img.png)\n",
    )
    .unwrap();
    fs::write(root.path().join("docs/system/img.png"), b"PNGDATA").unwrap();
    fs::write(
        root.path().join("docgen.toml"),
        "[s3]\nbucket=\"b\"\nregion=\"auto\"\npublic_url=\"https://cdn.example.com\"\n",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    // Attachment copied locally (NOT offloaded), HTML uses the local base URL.
    assert!(
        out.path().join("system/img.png").is_file(),
        "attachment should be copied locally"
    );
    let html = fs::read_to_string(out.path().join("system/index/index.html")).unwrap();
    assert!(
        html.contains(r#"src="/system/img.png""#),
        "expected local url in html: {html}"
    );

    // --- Part 2: credentials present, but BuildMode::Dev -> must never upload.
    // If the mode gate were missing, this would attempt a real (failing/hanging)
    // network upload against the fake credentials below.
    std::env::set_var("AWS_ACCESS_KEY_ID", "test-fake");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "test-fake");

    let dev_root = tempfile::tempdir().unwrap();
    fs::create_dir_all(dev_root.path().join("docs/system/attachments")).unwrap();
    fs::write(
        dev_root.path().join("docs/system/index.md"),
        "# System\n\n![Diagram](./attachments/image.png)\n",
    )
    .unwrap();
    let png = b"\x89PNG\r\n\x1a\n";
    fs::write(
        dev_root.path().join("docs/system/attachments/image.png"),
        png,
    )
    .unwrap();
    fs::write(
        dev_root.path().join("docgen.toml"),
        "[s3]\nbucket=\"b\"\nregion=\"auto\"\npublic_url=\"https://cdn.example.com\"\n",
    )
    .unwrap();
    let dev_out = tempfile::tempdir().unwrap();

    build_site(&BuildOptions {
        project_root: dev_root.path(),
        out_dir: dev_out.path(),
        mode: BuildMode::Dev,
    })
    .unwrap();

    std::env::remove_var("AWS_ACCESS_KEY_ID");
    std::env::remove_var("AWS_SECRET_ACCESS_KEY");

    let copied = dev_out.path().join("system/attachments/image.png");
    assert!(
        copied.is_file(),
        "dev build should copy the attachment locally, not upload it"
    );
    assert_eq!(fs::read(&copied).unwrap(), png, "copied bytes must match");
    let dev_html =
        fs::read_to_string(dev_out.path().join("system/index/index.html")).unwrap();
    assert!(
        dev_html.contains(r#"src="/system/attachments/image.png""#),
        "dev build must use a local url, never an S3 url: {dev_html}"
    );
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
