//! The reusable site-build pipeline: discover -> render -> emit the whole
//! `docs/` tree into an output dir. Both `docgen build` and the dev server
//! (`docgen-server`) call [`build_site`], so the pipeline lives here once
//! rather than inline in the bin.

mod history;
use history::report_to_buckets;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use docgen_core::discover::discover_docs;
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{GraphContext, HistoryContext, PageContext, Renderer, DEFAULT_PAGE_TEMPLATE};

/// The slug docgen treats as the site home (served at `/`).
const HOME_SLUG: &str = "index";

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

/// Whether this build is for static distribution or for the dev server.
///
/// [`build_site`] emits ONLY production assets in BOTH modes; the dev server
/// adds dev-only assets/HTML itself, AFTER `build_site` returns. The mode is
/// recorded for logging + so the dev server can assert its build context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildMode {
    #[default]
    Production,
    Dev,
}

/// Inputs to a full site build.
pub struct BuildOptions<'a> {
    /// Project root containing `docs/`.
    pub project_root: &'a Path,
    /// Where the static site is written. `docgen build` passes `project_root/dist`;
    /// the dev server passes a temp dir it owns.
    pub out_dir: &'a Path,
    pub mode: BuildMode,
}

/// Result of a build (counts for logging; extend later if needed).
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub page_count: usize,
    pub any_mermaid: bool,
    pub out_dir: PathBuf,
}

/// Back-compat thin wrapper used by `docgen build`: builds `root/docs` into
/// `root/dist` in Production mode. Equivalent to the old `build::build(root)`.
pub fn build(project_root: &Path) -> Result<BuildOutcome> {
    build_site(&BuildOptions {
        project_root,
        out_dir: &project_root.join("dist"),
        mode: BuildMode::Production,
    })
}

/// Discover -> render -> emit the whole site into `opts.out_dir`. This is the
/// single pipeline both `docgen build` and `docgen dev` call.
///
/// The build is **atomic**: the whole site is rendered into a fresh staging dir
/// (a sibling temp dir) and only swapped into `out_dir` on full success. A
/// failure anywhere in the pipeline therefore leaves any pre-existing `out_dir`
/// untouched — so the dev server genuinely keeps serving the last good build
/// (the swap is the only step that mutates `out_dir`). Emits ONLY production
/// assets regardless of `opts.mode`.
pub fn build_site(opts: &BuildOptions) -> Result<BuildOutcome> {
    let docs_dir = opts.project_root.join("docs");
    let final_dir = opts.out_dir;

    // The mode is recorded for logging/parity; the pipeline is mode-independent.
    let _ = opts.mode;

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Two-pass: prepare all docs, then render with full slug knowledge.
    let prepared: Vec<_> = raws.into_iter().map(prepare).collect();
    // Load `docgen.toml` (absent → defaults reproduce pre-P6 behaviour).
    let config = docgen_config::load(opts.project_root)
        .with_context(|| format!("loading docgen.toml from {}", opts.project_root.display()))?;
    let site = render_docs(prepared, &config);
    let tree = build_tree(&site.docs);

    let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;

    // Render the whole site into a fresh staging dir; only swap it into
    // `final_dir` once everything below succeeds. This makes the build atomic:
    // a failure leaves any existing `final_dir` (the last good build) intact.
    // Staging lives alongside `final_dir` so the final move is a same-filesystem
    // rename (atomic) rather than a cross-device copy.
    let staging = StagingDir::new(final_dir)?;
    let dist_dir = staging.path();

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
                    base: &config.base,
                    site_title: config.title.as_deref().unwrap_or(""),
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
    let mut home_html: Option<String> = None;
    for doc in &site.docs {
        let backlinks = site.graph.backlinks.get(&doc.slug).unwrap_or(&empty);
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            slug: &doc.slug,
            body_html: &doc.body_html,
            tree: &tree,
            backlinks,
            has_history: docs_with_history.contains(&doc.slug),
            has_mermaid: doc.has_mermaid,
            has_math: doc.has_math,
            base: &config.base,
            site_title: config.title.as_deref().unwrap_or(""),
            search_enabled: config.features.search,
        })?;

        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), &html)?;
        if doc.slug == HOME_SLUG {
            home_html = Some(html);
        }
    }

    // Root page: serve the home doc at `/` too, so the site has a real index.
    // The nested `dist/index/index.html` is still emitted above, so existing
    // `/index` links keep working — this is purely additive.
    if let Some(html) = home_html {
        fs::write(dist_dir.join("index.html"), html)?;
    }

    // Phase 3: the /graph/ page (default-on, gated off by `[features] graph =
    // false`). Deterministic force layout from the already-built link graph —
    // never recomputes links.
    if config.features.graph {
        let graph_data = site.graph_data(docgen_core::graphlayout::LayoutParams::default());
        let graph_json = docgen_core::graphlayout::graph_data_json(&graph_data);
        let graph_html = renderer.render_graph(&GraphContext {
            tree: &tree,
            graph_json: &graph_json,
            node_count: graph_data.nodes.len(),
            edge_count: graph_data.edges.len(),
            base: &config.base,
            site_title: config.title.as_deref().unwrap_or(""),
            search_enabled: config.features.search,
        })?;
        let graph_dir = dist_dir.join("graph");
        fs::create_dir_all(&graph_dir)?;
        fs::write(graph_dir.join("index.html"), graph_html)?;
    }

    // Static search index (gated off by `[features] search = false`).
    if config.features.search {
        fs::write(
            dist_dir.join("search-index.json"),
            docgen_core::search::index_json(&site.search),
        )?;
    }

    // All vendored + authored client assets flow through docgen-assets. Mermaid
    // (lib + island) ships only when a page used a diagram; math renders at build
    // time (the default), so no runtime KaTeX JS ships. The graph island ships
    // only when the /graph/ page is emitted.
    let emit_opts = docgen_assets::EmitOptions {
        include_katex_runtime: false,
        include_mermaid: site.any_mermaid,
        include_graph: config.features.graph,
    };
    docgen_assets::emit(&docgen_assets::assets_for(&emit_opts), dist_dir)?;

    // Everything rendered cleanly: atomically swap staging into place. Only now
    // is the previously-served `final_dir` replaced.
    staging.commit(final_dir)?;

    Ok(BuildOutcome {
        page_count: site.docs.len(),
        any_mermaid: site.any_mermaid,
        out_dir: final_dir.to_path_buf(),
    })
}

/// A staging directory for an atomic build. Renders happen here; [`commit`]
/// swaps it into the final location only on success. If dropped without
/// committing (i.e. the build errored), the staging dir is best-effort removed,
/// leaving the previous `final_dir` untouched.
///
/// [`commit`]: StagingDir::commit
struct StagingDir {
    path: PathBuf,
    committed: bool,
}

impl StagingDir {
    /// Create a fresh, empty staging dir as a sibling of `final_dir` (same
    /// filesystem -> the final rename is atomic).
    fn new(final_dir: &Path) -> Result<Self> {
        let parent = final_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        fs::create_dir_all(&parent)
            .with_context(|| format!("creating staging parent {}", parent.display()))?;
        let file_name = final_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "dist".to_string());
        let path = parent.join(format!(".{file_name}.staging-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)
            .with_context(|| format!("creating staging dir {}", path.display()))?;
        Ok(Self {
            path,
            committed: false,
        })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically replace `final_dir` with the staging dir. Removes any existing
    /// `final_dir` first, then renames staging into place.
    fn commit(mut self, final_dir: &Path) -> Result<()> {
        let _ = fs::remove_dir_all(final_dir);
        if let Some(parent) = final_dir.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&self.path, final_dir).with_context(|| {
            format!(
                "swapping build {} -> {}",
                self.path.display(),
                final_dir.display()
            )
        })?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
