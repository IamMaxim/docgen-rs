use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use docgen_core::assemble::assemble;
use docgen_core::discover::discover_docs;
use docgen_core::tree::build_tree;
use docgen_render::{PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

/// Build the site at `project_root` (which must contain `docs/`) into `project_root/dist`.
pub fn build(project_root: &Path) -> Result<()> {
    let docs_dir = project_root.join("docs");
    let dist_dir = project_root.join("dist");

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;
    let docs: Vec<_> = raws.into_iter().map(assemble).collect();
    let tree = build_tree(&docs);

    let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;

    // Clean and recreate dist.
    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    for doc in &docs {
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            body_html: &doc.body_html,
            tree: &tree,
        })?;

        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), html)?;
    }

    println!("Built {} page(s) -> {}", docs.len(), dist_dir.display());
    Ok(())
}
