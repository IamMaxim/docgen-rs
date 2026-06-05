use std::process::Command;

#[test]
fn init_then_build_renders_the_scaffolded_site_with_custom_component() {
    let tmp = std::env::temp_dir().join(format!("docgen_init_build_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // `docgen init <tmp> --force`
    let st = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("init")
        .arg(&tmp)
        .arg("--force")
        .status()
        .unwrap();
    assert!(st.success());
    assert!(tmp.join("docgen.toml").is_file());
    assert!(tmp.join("docs/index.md").is_file());
    assert!(tmp.join("components/note/template.html").is_file());

    // `docgen build <tmp>`
    let st = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(st.success());

    // Root page exists (the A-3 fix) and renders BOTH the built-in callout and the
    // project `note` component from the scaffolded content.
    let home = std::fs::read_to_string(tmp.join("dist/index.html")).unwrap();
    assert!(home.contains("docgen-callout--warning")); // built-in, dogfooded
    assert!(home.contains("Heads up"));
    assert!(home.contains("docgen-note")); // project leaf component
    assert!(home.contains("a project component")); // its label
    assert!(home.contains("— My Docs</title>")); // config title suffix
    assert!(tmp.join("dist/components.css").is_file()); // bundled component styles

    // guide page: wikilink resolved + mermaid island gated on
    let guide = std::fs::read_to_string(tmp.join("dist/guide/index.html")).unwrap();
    assert!(guide.contains(r#"href="/index""#));
    assert!(guide.contains(r#"src="/islands/mermaid.js""#));

    let _ = std::fs::remove_dir_all(&tmp);
}
