use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Local;
use docgen_core::discover::discover_docs;
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{HistoryContext, PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

use crate::history::report_to_buckets;

/// Default per-doc commit-history depth (parity with the original `diffLimit`).
const DEFAULT_DIFF_LIMIT: usize = 50;
/// Hard cap so a pathological `DOC_DIFF_LIMIT` can't blow up build time.
const MAX_DIFF_LIMIT: usize = 200;

fn diff_limit() -> usize {
    std::env::var("DOC_DIFF_LIMIT")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_DIFF_LIMIT)
        .clamp(1, MAX_DIFF_LIMIT)
}

/// Compute the doc path as git sees it, relative to the repo working directory.
/// e.g. docs_dir `/repo/docs`, workdir `/repo/`, `rel_path` `guide/intro.md`
/// -> `docs/guide/intro.md`. Returns `None` if `docs_dir` is not under `workdir`.
fn git_rel_path(docs_dir: &Path, workdir: &Path, rel_path: &str) -> Option<String> {
    // Canonicalize both sides: on macOS `env::temp_dir()` yields `/var/...`
    // while git2's `workdir()` resolves the `/private/var` symlink, so a raw
    // `strip_prefix` would spuriously fail.
    let docs_canon = docs_dir.canonicalize().ok();
    let work_canon = workdir.canonicalize().ok();
    let (docs_ref, work_ref) = match (&docs_canon, &work_canon) {
        (Some(d), Some(w)) => (d.as_path(), w.as_path()),
        _ => (docs_dir, workdir),
    };
    let prefix = docs_ref.strip_prefix(work_ref).ok()?;
    let mut parts: Vec<String> = prefix
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.push(rel_path.to_string());
    Some(parts.join("/"))
}

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

    // Phase 1: build the per-doc git history pages (graceful no-op outside a
    // git repo or for docs with no commit history). Collect which slugs got a
    // history page so the doc page can conditionally show its "History" link.
    let mut docs_with_history: HashSet<String> = HashSet::new();
    let repo = docgen_diff::discover_repo(&docs_dir)
        .with_context(|| format!("discovering git repo for {}", docs_dir.display()))?;
    if let Some(repo) = repo {
        if let Some(workdir) = repo.workdir().map(Path::to_path_buf) {
            let limit = diff_limit();
            let now = Local::now();
            for doc in &site.docs {
                let Some(git_path) = git_rel_path(&docs_dir, &workdir, &doc.rel_path) else {
                    continue;
                };
                let report = docgen_diff::build_doc_diff_report(&repo, &git_path, limit)
                    .with_context(|| format!("building diff report for {git_path}"))?;
                let Some(report) = report else { continue };

                let buckets = report_to_buckets(&report, now);
                let html = renderer.render_history(&HistoryContext {
                    title: &doc.title,
                    slug: &doc.slug,
                    tree: &tree,
                    buckets: &buckets,
                })?;
                let out_dir = dist_dir.join(&doc.slug).join("history");
                fs::create_dir_all(&out_dir)?;
                fs::write(out_dir.join("index.html"), html)?;
                docs_with_history.insert(doc.slug.clone());
            }
        }
    }

    // Phase 2: render the doc pages, linking to history where one was emitted.
    let empty: Vec<docgen_core::model::Backlink> = Vec::new();
    for doc in &site.docs {
        let backlinks = site.graph.backlinks.get(&doc.slug).unwrap_or(&empty);
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            slug: &doc.slug,
            body_html: &doc.body_html,
            tree: &tree,
            backlinks,
            has_history: docs_with_history.contains(&doc.slug),
            has_mermaid: false,
            has_math: doc.has_math,
        })?;

        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), html)?;
    }

    // Static search index.
    fs::write(
        dist_dir.join("search-index.json"),
        docgen_core::search::index_json(&site.search),
    )?;

    // All vendored + authored client assets flow through docgen-assets. The
    // mermaid signal is wired in Cluster C (C-6); for now the build-time KaTeX
    // path and no-mermaid defaults apply.
    let include_mermaid = false;
    let emit_opts = docgen_assets::EmitOptions {
        include_katex_runtime: false,
        include_mermaid,
    };
    docgen_assets::emit(&docgen_assets::assets_for(&emit_opts), &dist_dir)?;

    println!(
        "Built {} page(s) -> {}",
        site.docs.len(),
        dist_dir.display()
    );
    Ok(())
}
