//! Runs the interactive-bases island's Node parity suite as part of `cargo test`.
//!
//! The island (`assets/docgen/islands/bases.js`) ports `docgen-bases`'
//! `Value::loose_cmp`/filter semantics to JavaScript; `js-tests/bases.test.mjs`
//! asserts that port stays exact. These are pure-logic tests (no DOM), run via
//! `node --test`. Node is a dev/CI tool only (never a build/runtime dependency),
//! so if `node` is unavailable the test SKIPS rather than fails — CI runners have
//! Node, local contributors may not.

use std::process::Command;

#[test]
fn island_pure_logic_parity_suite() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    // Probe for node; skip cleanly if it isn't installed.
    match Command::new("node").arg("--version").output() {
        Ok(o) if o.status.success() => {}
        _ => {
            eprintln!("SKIP island_pure_logic_parity_suite: `node` not available");
            return;
        }
    }

    // Pass the test files explicitly instead of the directory: what `--test`
    // makes of a directory argument has changed across Node releases (v22.23
    // requires it as a module and dies with MODULE_NOT_FOUND, where v20 scans
    // it), and an explicit file list behaves identically on every version.
    // Enumerating also turns "suite missing" into a loud failure instead of a
    // run over zero files.
    let mut test_files: Vec<std::path::PathBuf> =
        std::fs::read_dir(std::path::Path::new(manifest).join("js-tests"))
            .expect("js-tests/ directory missing")
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".test.mjs"))
            })
            .collect();
    test_files.sort();
    assert!(
        !test_files.is_empty(),
        "no *.test.mjs files found in js-tests/"
    );

    let out = Command::new("node")
        .arg("--test")
        .args(&test_files)
        .current_dir(manifest)
        .output()
        .expect("failed to spawn node --test");

    if !out.status.success() {
        panic!(
            "node --test failed:\n--- stdout ---\n{}\n--- stderr ---\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
