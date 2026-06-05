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

    // No page in this fixture uses a diagram, so the mermaid lib + island are
    // NOT emitted, and no page links the island script.
    assert!(!tmp.join("dist/vendor/mermaid/mermaid.min.js").exists());
    assert!(!tmp.join("dist/islands/mermaid.js").exists());
    assert!(!home.contains("islands/mermaid.js"));

    // The /graph/ page + island always ship (P4 default-on).
    assert!(tmp.join("dist/graph/index.html").is_file());
    assert!(tmp.join("dist/islands/graph.js").is_file());
    // Every doc page links to /graph/.
    assert!(home.contains(r#"href="/graph""#));

    let _ = fs::remove_dir_all(&tmp);
}

/// Graph view: the build emits /graph/ with embedded GraphData JSON, mounts the
/// docgenGraph island, ships islands/graph.js, and every page links to /graph/.
#[test]
fn builds_graph_page_with_island_and_nav_link() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap().parent().unwrap();
    let fixture = workspace.join("fixtures/site-basic");

    let tmp = std::env::temp_dir()
        .join(format!("docgen_build_cli_graph_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), tmp.join("docs/index.md")).unwrap();
    fs::copy(
        fixture.join("docs/guide/intro.md"),
        tmp.join("docs/guide/intro.md"),
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    // /graph/ page exists with the island + embedded data + meta.
    let graph = fs::read_to_string(tmp.join("dist/graph/index.html")).unwrap();
    assert!(graph.contains("<title>Graph</title>"));
    assert!(graph.contains(r#"x-data="docgenGraph""#));
    assert!(graph.contains(r#"id="docgen-graph-data""#));
    assert!(graph.contains(r#"src="/islands/graph.js""#));

    // Embedded JSON is real, parseable, and reflects the two docs + their links.
    let start = graph.find("docgen-graph-data").unwrap();
    let open = graph[start..].find('>').unwrap() + start + 1;
    let close = graph[open..].find("</script>").unwrap() + open;
    let json = &graph[open..close];
    let data: serde_json::Value = serde_json::from_str(json).unwrap();
    let nodes = data["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().any(|n| n["slug"] == "index"));
    assert!(nodes.iter().any(|n| n["slug"] == "guide/intro"));
    let edges = data["edges"].as_array().unwrap();
    assert!(edges
        .iter()
        .any(|e| e["from"] == "index" && e["to"] == "guide/intro"));

    // Positions are finite + in-bounds (integration-level determinism check).
    for n in nodes {
        let (x, y) = (n["x"].as_f64().unwrap(), n["y"].as_f64().unwrap());
        assert!(x.is_finite() && y.is_finite());
        assert!((74.0..=1420.0 - 74.0).contains(&x));
        assert!((74.0..=760.0 - 74.0).contains(&y));
    }

    // The island JS is emitted, with no vendored graph lib.
    assert!(tmp.join("dist/islands/graph.js").is_file());
    let island = fs::read_to_string(tmp.join("dist/islands/graph.js")).unwrap();
    assert!(!island.to_lowercase().contains("d3"));

    // Every doc page links to /graph/.
    let home = fs::read_to_string(tmp.join("dist/index/index.html")).unwrap();
    assert!(home.contains(r#"href="/graph""#));

    let _ = fs::remove_dir_all(&tmp);
}

/// Mermaid island: a doc with a ```mermaid fence builds an inert island
/// container, the page links the island script, and the lazy lib + island JS are
/// emitted. A page-level gate keeps both off pages without diagrams.
#[test]
fn builds_mermaid_page_with_lazy_island() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap().parent().unwrap();
    let fixture = workspace.join("fixtures/site-basic");

    let tmp = std::env::temp_dir()
        .join(format!("docgen_build_cli_mermaid_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    fs::copy(fixture.join("docs/diagram.md"), tmp.join("docs/diagram.md")).unwrap();
    // A plain page alongside it to prove per-page gating of the island script.
    fs::copy(fixture.join("docs/index.md"), tmp.join("docs/index.md")).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    let diag = fs::read_to_string(tmp.join("dist/diagram/index.html")).unwrap();
    // Inert island container with the diagram source preserved.
    assert!(diag.contains("docgen-mermaid"), "no container: {diag}");
    assert!(diag.contains(r#"x-data="docgenMermaid""#));
    assert!(diag.contains("graph TD")); // source preserved
    // The diagram page links the lazy island script.
    assert!(diag.contains(r#"src="/islands/mermaid.js""#));

    // Lazy lib + island JS emitted (the island fetches the lib at runtime).
    assert!(tmp.join("dist/vendor/mermaid/mermaid.min.js").is_file());
    assert!(tmp.join("dist/islands/mermaid.js").is_file());

    // Per-page gate: the plain page does NOT link the island script.
    let home = fs::read_to_string(tmp.join("dist/index/index.html")).unwrap();
    assert!(!home.contains("islands/mermaid.js"));

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

/// End-to-end graceful degradation: a doc carrying a malformed math expression
/// must NOT fail the build. It flows the full comrak `$...$` -> NodeValue::Math
/// -> transform_math -> render_math path (not the hand-passed unit-test string),
/// and the failed expression must land as `docgen-math-error` markup with HTML
/// metacharacters escaped, while the rest of the page still renders.
#[test]
fn broken_math_degrades_without_failing_build() {
    let tmp = std::env::temp_dir()
        .join(format!("docgen_build_cli_badmath_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs")).unwrap();
    // A genuinely-broken inline expression carrying HTML metacharacters, plus
    // surrounding prose that must survive intact.
    fs::write(
        tmp.join("docs/bad.md"),
        "# Bad\n\nProse before. Broken inline math: $<script>\\frac{$ and prose after.\n",
    )
    .unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    // The central safety claim: a malformed expression does not abort the build.
    assert!(status.success(), "build must not fail on broken math");

    let page = fs::read_to_string(tmp.join("dist/bad/index.html")).unwrap();
    // Failed expression degraded to the escaped error markup, not a crash.
    assert!(
        page.contains("docgen-math-error"),
        "no math-error fallback: {page}"
    );
    // HTML metacharacters in the failed expression are escaped (render.unsafe=true downstream).
    assert!(page.contains("&lt;script&gt;"), "metachars not escaped: {page}");
    assert!(!page.contains("<script>\\frac"), "raw script tag leaked: {page}");
    // The rest of the page still rendered.
    assert!(page.contains("Prose before."));
    assert!(page.contains("and prose after."));

    let _ = fs::remove_dir_all(&tmp);
}
