use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Copy the checked-in fixture into a temp dir, run `docgen build`, assert output.
#[test]
fn builds_fixture_site() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // crates/docgen
    let workspace = manifest.parent().unwrap().parent().unwrap(); // repo root
    let fixture = workspace.join("fixtures/site-basic");

    let tmp = std::env::temp_dir().join(format!("docgen_build_cli_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), tmp.join("docs/index.md")).unwrap();
    fs::copy(fixture.join("docs/guide/intro.md"), tmp.join("docs/guide/intro.md")).unwrap();
    fs::copy(fixture.join("docs/markup.md"), tmp.join("docs/markup.md")).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    let home = fs::read_to_string(tmp.join("dist/index/index.html")).unwrap();
    assert!(home.contains("<title>Home</title>"));
    assert!(home.contains("<strong>basic</strong>"));
    // Sidebar renders on the index page too: links to the guide doc and the dir label.
    assert!(home.contains(r#"href="/guide/intro""#));
    assert!(home.contains(">guide<"));

    let intro = fs::read_to_string(tmp.join("dist/guide/intro/index.html")).unwrap();
    assert!(intro.contains("<title>Introduction</title>"));
    // Sidebar shows both entries on every page, including the dir node.
    assert!(intro.contains(r#"href="/index""#));
    assert!(intro.contains(r#"href="/guide/intro""#));
    assert!(intro.contains(">guide<"));

    // Resolved wikilink on the home page.
    assert!(home.contains(r#"<a class="docgen-wikilink" href="/guide/intro">Intro guide</a>"#));

    // Intro page: resolved backlink target, broken wikilink, highlighted code.
    assert!(intro.contains(r#"href="/index""#));
    assert!(intro.contains("docgen-wikilink--broken"));
    assert!(intro.contains("style=\"color:")); // syntect highlight

    // Backlinks section: intro links to index, so index's page lists intro as a backlink.
    assert!(home.contains("Backlinks"));
    assert!(home.contains(r#"href="/guide/intro""#));

    // Search index emitted with one entry per doc, plaintext, no markup.
    let idx = fs::read_to_string(tmp.join("dist/search-index.json")).unwrap();
    assert!(idx.contains(r#""slug":"index""#));
    assert!(idx.contains(r#""slug":"guide/intro""#));
    assert!(idx.contains(r#""title":"Home""#));
    assert!(!idx.contains("[[")); // wikilink brackets stripped from indexed text

    // Parse the index for real and exercise the markup-stripping path on a doc
    // that actually contains `<`, `>`, `&`, raw inline HTML and an autolink.
    let entries: serde_json::Value = serde_json::from_str(&idx).unwrap();
    let markup = entries
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["slug"] == "markup")
        .expect("markup entry present");
    let text = markup["text"].as_str().unwrap();
    // Human-readable words survive, including the literal `<` between a and b...
    assert!(text.contains("Compare a < b"), "got: {text}");
    // ...and the inner text of raw inline HTML, but never the tag itself.
    assert!(text.contains("inline"));
    assert!(!text.contains("<em>"));
    assert!(!text.contains("</em>"));
    // The broken wikilink's display word is preserved (brackets stripped).
    assert!(text.contains("missing-page"));
    assert!(!text.contains("[["));

    // Vendored client assets emitted.
    let js = fs::read_to_string(tmp.join("dist/search.js")).unwrap();
    assert!(js.contains("search-index.json"));
    assert!(tmp.join("dist/docgen.css").exists());

    // docgen-assets emitted the island infra alongside search + css.
    assert!(tmp.join("dist/bootstrap.js").is_file());
    assert!(tmp.join("dist/vendor/alpine/alpine.min.js").is_file());

    // Template wires the search trigger + script.
    assert!(home.contains("data-docgen-search"));
    assert!(home.contains(r#"src="/search.js""#));

    // Template wires the island bootstrap + Alpine on every page.
    assert!(home.contains(r#"src="/bootstrap.js""#));
    assert!(home.contains(r#"src="/vendor/alpine/alpine.min.js""#));

    let _ = fs::remove_dir_all(&tmp);
}

/// Build-time KaTeX: a doc with inline + display math renders to math HTML in
/// the built page, the KaTeX css + fonts are emitted, and NO runtime KaTeX JS
/// ships (the default build-time path).
#[test]
fn builds_math_page_with_build_time_katex() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap().parent().unwrap();
    let fixture = workspace.join("fixtures/site-basic");

    let tmp =
        std::env::temp_dir().join(format!("docgen_build_cli_math_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::copy(fixture.join("docs/math.md"), tmp.join("docs/math.md")).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    let math = fs::read_to_string(tmp.join("dist/math/index.html")).unwrap();
    // Build-time KaTeX HTML present (both inline and display).
    assert!(math.contains("class=\"katex\""), "no katex html: {math}");
    assert!(math.contains("katex-display"), "no display math: {math}");
    // Raw `$`-delimiters are gone — math was rendered, not passed through.
    // (KaTeX preserves the LaTeX source inside a MathML <annotation>, so the
    // expression text itself legitimately survives; the delimiters do not.)
    assert!(!math.contains("$E=mc^2$"));
    assert!(!math.contains("$$\\sum"));
    // KaTeX keeps the source in a TeX annotation — proof it rendered, not raw md.
    assert!(math.contains("application/x-tex"));
    // The page links the KaTeX stylesheet (gated on has_math).
    assert!(math.contains(r#"href="/vendor/katex/katex.min.css""#));

    // CSS + fonts emitted under dist/vendor/katex/.
    assert!(tmp.join("dist/vendor/katex/katex.min.css").is_file());
    assert!(tmp
        .join("dist/vendor/katex/fonts/KaTeX_Main-Regular.woff2")
        .is_file());
    // 16 woff2 fonts shipped.
    let fonts = fs::read_dir(tmp.join("dist/vendor/katex/fonts"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .map(|x| x == "woff2")
                .unwrap_or(false)
        })
        .count();
    assert_eq!(fonts, 16, "expected 16 katex fonts");

    // Build-time path: NO runtime KaTeX JS anywhere in dist.
    assert!(!tmp.join("dist/vendor/katex/katex.min.js").exists());
    assert!(!tmp.join("dist/vendor/katex/auto-render.min.js").exists());

    let _ = fs::remove_dir_all(&tmp);
}
