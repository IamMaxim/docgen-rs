#[test]
fn release_workflow_and_binstall_metadata_exist() {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().unwrap().parent().unwrap();
    let wf = repo.join(".github/workflows/release.yml");
    assert!(wf.is_file(), "release workflow missing");
    let wf_text = std::fs::read_to_string(&wf).unwrap();
    assert!(wf_text.contains("cargo build --release"));
    assert!(wf_text.contains("x86_64-unknown-linux-gnu"));

    let cargo = std::fs::read_to_string(manifest.join("Cargo.toml")).unwrap();
    assert!(cargo.contains("[package.metadata.binstall]"));
    assert!(cargo.contains("pkg-url"));

    let readme = std::fs::read_to_string(repo.join("README.md")).unwrap();
    assert!(readme.contains("cargo binstall docgen"));
    assert!(readme.contains("docgen init"));
}
