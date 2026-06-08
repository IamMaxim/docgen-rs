//! Integration: the custom-component directive system end-to-end through
//! `build_site` — built-in callout, a project component, override-by-name, and
//! per-page island gating.

use std::fs;
use std::path::{Path, PathBuf};

use docgen_build::{build_site, BuildMode, BuildOptions};

/// Recursively copy a directory tree.
fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// The checked-in fixture exercising directives ships a `directives.md` doc and a
/// project `components/note/`; build it and assert the directive system rendered.
#[test]
fn checked_in_fixture_exercises_directives() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen-build
    let workspace = manifest.parent().unwrap().parent().unwrap(); // repo root
    let fixture = workspace.join("fixtures/site-basic");

    let root = tempfile::tempdir().unwrap();
    copy_tree(&fixture.join("docs"), &root.path().join("docs"));
    copy_tree(&fixture.join("components"), &root.path().join("components"));

    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let page = fs::read_to_string(out.path().join("directives/index.html")).unwrap();
    // Built-in callout (block) with attrs + inner markdown.
    assert!(page.contains("docgen-callout--warning"));
    assert!(page.contains("Back up first"));
    // Nested callout rendered by the recursive pipeline.
    assert!(page.contains("docgen-callout--note"));
    // Wikilink inside the callout body still produces an anchor target text.
    assert!(page.contains("wikilink"));
    // Project leaf component rendered inline.
    assert!(page.contains("docgen-note"));
    assert!(page.contains("a project component"));
    // Unknown directive degrades to an inert, marked error span.
    assert!(page.contains("docgen-directive-error"));
    assert!(page.contains("unknown directive"));
    // Component CSS bundle emitted + linked (built-in callout + project note).
    assert!(page.contains(r#"href="/components.css""#));
    let css = fs::read_to_string(out.path().join("components.css")).unwrap();
    assert!(css.contains("docgen-callout"));
    assert!(css.contains("docgen-note"));
    // No island components used → no components.js.
    assert!(!out.path().join("components.js").exists());
}

#[test]
fn build_renders_builtin_callout_and_project_component() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docs/index.md"),
        "# Home\n\n:::callout{type=warning title=\"Heads up\"}\nBe **careful**.\n:::\n\n:note[hi]{}\n",
    )
    .unwrap();
    // project component `note` (leaf) with a style
    let nd = root.path().join("components/note");
    fs::create_dir_all(&nd).unwrap();
    fs::write(nd.join("template.html"), "<span class=\"note\">{{ label }}</span>").unwrap();
    fs::write(nd.join("style.css"), ".note{color:teal}").unwrap();

    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();

    let home = fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(home.contains("docgen-callout--warning")); // built-in callout
    assert!(home.contains("Heads up"));
    assert!(home.contains("<strong>careful</strong>")); // inner markdown
    assert!(home.contains("class=\"note\">hi")); // project leaf component
    assert!(home.contains(r#"href="/components.css""#));
    // callout + note are island-free → no components.js linked/emitted
    assert!(!out.path().join("components.js").exists());
    let css = fs::read_to_string(out.path().join("components.css")).unwrap();
    assert!(css.contains("docgen-callout")); // built-in style bundled
    assert!(css.contains(".note")); // project style bundled
}

#[test]
fn project_component_overrides_builtin_callout_in_build() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docs/index.md"),
        "# Home\n\n:::callout{}\nx\n:::\n",
    )
    .unwrap();
    let cd = root.path().join("components/callout");
    fs::create_dir_all(&cd).unwrap();
    fs::write(
        cd.join("template.html"),
        "<div class=\"my-callout\">{{ content | safe }}</div>",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    let home = fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(home.contains("my-callout"));
    assert!(!home.contains("docgen-callout--note")); // builtin overridden
}

#[test]
fn island_component_emits_components_js_only_on_pages_that_use_it() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("docs")).unwrap();
    fs::write(
        root.path().join("docs/index.md"),
        "# Home\n\n:::rating{id=p max=5}\n:::\n",
    )
    .unwrap();
    fs::write(
        root.path().join("docs/plain.md"),
        "# Plain\nno directive\n",
    )
    .unwrap();
    let rd = root.path().join("components/rating");
    fs::create_dir_all(&rd).unwrap();
    fs::write(
        rd.join("template.html"),
        "<div x-data=\"docgenRating()\" data-id=\"{{ attrs.id }}\"></div>",
    )
    .unwrap();
    fs::write(
        rd.join("island.js"),
        "Alpine.data('docgenRating',()=>({}))",
    )
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    assert!(out.path().join("components.js").is_file());
    let home = fs::read_to_string(out.path().join("index.html")).unwrap();
    let plain = fs::read_to_string(out.path().join("plain/index.html")).unwrap();
    assert!(home.contains(r#"src="/components.js""#));
    assert!(!plain.contains(r#"src="/components.js""#)); // gated per-page
}

/// Build a throwaway project from in-memory files; returns the kept-alive
/// output tempdir.
fn build_files(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (rel, content) in files {
        let p = root.path().join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }
    let out = tempfile::tempdir().unwrap();
    build_site(&BuildOptions {
        project_root: root.path(),
        out_dir: out.path(),
        mode: BuildMode::Production,
    })
    .unwrap();
    out
}

#[test]
fn include_directive_transcludes_partial_and_excludes_it_as_page() {
    let out = build_files(&[
        ("docs/guide/index.md", "# Guide\n\n:include{src=\"./_facts.gen.md\"}\n"),
        ("docs/guide/_facts.gen.md", "## Facts\n\n- alpha\n- beta\n"),
    ]);
    // The partial's content is spliced into the host page...
    let host = fs::read_to_string(out.path().join("guide/index/index.html")).unwrap();
    assert!(host.contains("Facts"), "partial heading missing: {host}");
    assert!(host.contains("alpha"), "partial list missing");
    // ...and the partial never becomes its own page.
    assert!(!out.path().join("guide/_facts.gen/index.html").exists(), "partial leaked as a page");
}

#[test]
fn include_missing_src_degrades_to_error_span() {
    let out = build_files(&[("docs/index.md", "# Home\n\n:include{src=\"./_nope.md\"}\n")]);
    let html = fs::read_to_string(out.path().join("index/index.html")).unwrap();
    assert!(html.contains("docgen-directive-error"), "expected inert error span: {html}");
    // Reaching here means the build did not panic / fail.
}
