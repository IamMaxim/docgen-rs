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
    assert!(!idx.contains("<")); // no HTML markup in indexed text

    // Vendored client assets emitted.
    let js = fs::read_to_string(tmp.join("dist/search.js")).unwrap();
    assert!(js.contains("search-index.json"));
    assert!(tmp.join("dist/docgen.css").exists());

    // Template wires the search trigger + script.
    assert!(home.contains("data-docgen-search"));
    assert!(home.contains(r#"src="/search.js""#));

    let _ = fs::remove_dir_all(&tmp);
}
