use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use docgen_core::discover::discover_docs;
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

/// Build the site at `project_root` (which must contain `docs/`) into `project_root/dist`.
pub fn build(project_root: &Path) -> Result<()> {
    let docs_dir = project_root.join("docs");
    let dist_dir = project_root.join("dist");

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Two-pass: prepare all docs, then render with full slug knowledge.
    let prepared: Vec<_> = raws.into_iter().map(prepare).collect();
    let site = render_docs(prepared);
    let tree = build_tree(&site.docs);

    let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;

    // Clean and recreate dist.
    let _ = fs::remove_dir_all(&dist_dir);
    fs::create_dir_all(&dist_dir)?;

    let empty: Vec<docgen_core::model::Backlink> = Vec::new();
    for doc in &site.docs {
        let backlinks = site.graph.backlinks.get(&doc.slug).unwrap_or(&empty);
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            body_html: &doc.body_html,
            tree: &tree,
            backlinks,
        })?;

        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), html)?;
    }

    // Cluster C adds: emit search-index.json, search.js, docgen.css here.
    println!("Built {} page(s) -> {}", site.docs.len(), dist_dir.display());
    Ok(())
}
