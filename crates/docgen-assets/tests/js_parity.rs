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

    let out = Command::new("node")
        .arg("--test")
        .arg("js-tests/")
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
