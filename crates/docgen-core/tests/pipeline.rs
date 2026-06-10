use std::fs;

use docgen_core::assemble::assemble;
use docgen_core::discover::discover_docs;
use docgen_core::tree::build_tree;

#[test]
fn discovers_and_processes_a_temp_site() {
    let dir =
        std::env::temp_dir().join(format!("docgen_core_pipeline_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("docs/guide")).unwrap();
    fs::write(dir.join("docs/index.md"), "# Home\n").unwrap();
    fs::write(
        dir.join("docs/guide/intro.md"),
        "---\ntitle: Intro\n---\nbody\n",
    )
    .unwrap();

    let mut raws = discover_docs(&dir.join("docs")).unwrap();
    raws.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    assert_eq!(raws.len(), 2);
    assert_eq!(raws[0].rel_path, "guide/intro.md");
    assert_eq!(raws[1].rel_path, "index.md");

    let docs: Vec<_> = raws.into_iter().map(assemble).collect();
    let tree = build_tree(&docs);
    assert_eq!(tree.len(), 2); // one dir (guide) + one doc (index)

    let _ = fs::remove_dir_all(&dir);
}
