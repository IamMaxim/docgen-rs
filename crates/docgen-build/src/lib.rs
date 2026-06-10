//! The reusable site-build pipeline: discover -> render -> emit the whole
//! `docs/` tree into an output dir. Both `docgen build` and the dev server
//! (`docgen-server`) call [`build_site`], so the pipeline lives here once
//! rather than inline in the bin.
//!
//! [`build_site`] loads an optional `docgen.toml` (`docgen-config`) and builds a
//! custom-component registry (`docgen-components`: embedded built-ins overridden
//! by a project `components/` dir). Config gates the graph/search/math/mermaid
//! features and supplies the site title/`base`; the registry drives directive
//! rendering and per-page component asset emission.

// Retained for its timeline-bucket grouping logic + tests; the per-doc history
// page it fed was superseded by the global `/diff` workspace.
#[allow(dead_code)]
mod history;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use docgen_core::discover::discover_docs;
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{
    GraphContext, HomeData, HomeRecent, HomeSection, PageContext, Renderer, DEFAULT_PAGE_TEMPLATE,
};

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
/// (the swap is the only step that mutates `out_dir`). Emits NO dev-only surface
/// (editor/livereload) in either mode — the dev server layers that on afterward.
/// The one mode-dependent emission is the mermaid runtime: Dev ships it
/// unconditionally so the editor preview can render a just-typed diagram.
pub fn build_site(opts: &BuildOptions) -> Result<BuildOutcome> {
    let docs_dir = opts.project_root.join("docs");
    let final_dir = opts.out_dir;

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;

    // Split include-only partials (`_*.md`) out of the page set; they render
    // only where a `:include` transcludes them, never as standalone pages.
    let (pages, partials) = docgen_core::pipeline::partition_partials(raws);
    // Two-pass: prepare all pages, then render with full slug knowledge.
    let prepared: Vec<_> = pages.into_iter().map(prepare).collect();
    // Load `docgen.toml` (absent → defaults reproduce pre-P6 behaviour).
    let config = docgen_config::load(opts.project_root)
        .with_context(|| format!("loading docgen.toml from {}", opts.project_root.display()))?;
    // Build the component registry: embedded built-ins first, then project
    // `components/<name>/` (which override a built-in of the same name).
    let builtins: Vec<docgen_components::Component> = docgen_assets::builtin_components()
        .into_iter()
        .map(|b| {
            docgen_components::Component::from_parts(
                b.name,
                b.template,
                b.island_js.map(str::to_string),
                b.style_css.map(str::to_string),
            )
        })
        .collect();
    let components_dir = opts.project_root.join(&config.components.dir);
    let registry = docgen_components::build_registry(builtins, &components_dir)
        .with_context(|| format!("discovering components in {}", components_dir.display()))?;
    let site = render_docs(prepared, &partials, &config, &registry);
    let tree = build_tree(&site.docs);

    // Concatenate the component asset bundle. `components.css` carries every
    // registry component's style (small + cacheable, linked on every page that
    // has any style); `components.js` carries only the island.js of components
    // actually *used* across the site (per-page link gating below decides which
    // pages reference it). Emitted in BTreeMap name-key order (deterministic).
    let has_components_css = !registry.styles().is_empty();
    let used_components: std::collections::BTreeSet<&str> = site
        .docs
        .iter()
        .flat_map(|d| d.components_used.iter().map(String::as_str))
        .collect();
    let component_css: String = registry
        .styles()
        .iter()
        .filter_map(|c| c.style_css.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    let component_js: String = registry
        .islands()
        .iter()
        .filter(|c| used_components.contains(c.name.as_str()))
        .filter_map(|c| c.island_js.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    // Set of components that ship an island AND were used → drives per-page gating.
    let island_components: std::collections::BTreeSet<String> = registry
        .islands()
        .iter()
        .filter(|c| used_components.contains(c.name.as_str()))
        .map(|c| c.name.clone())
        .collect();

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
    let repo = docgen_diff::discover_repo(&docs_dir)
        .with_context(|| format!("discovering git repo for {}", docs_dir.display()))?;
    // Build metadata for the right-rail "Additional info" section. Best-effort:
    // the short HEAD hash is empty outside a git repo (the Commit row is then
    // omitted by the template); `built` is the wall-clock build time.
    let now = Local::now();
    let built_stamp = now.format("%Y-%m-%d %H:%M").to_string();
    let mut commit_hash = String::new();
    // Whether the interactive `/diff/` workspace was emitted (repo has doc
    // history) — drives the topbar diff icon + the diff asset slice.
    let mut has_diff = false;
    if let Some(repo) = repo {
        if let Ok(head) = repo.head() {
            if let Some(oid) = head.target() {
                let s = oid.to_string();
                commit_hash = s.chars().take(7).collect();
            }
        }
        if let Some(workdir) = repo.workdir().map(Path::to_path_buf) {
            // The docs dir as git sees it, e.g. `docs` (trailing slash trimmed —
            // `git_rel_path` joins an empty leaf to the prefix).
            if let Some(docs_prefix) =
                git_rel_path(&docs_dir, &workdir, "").map(|p| p.trim_end_matches('/').to_string())
            {
                let limit = diff_limit();
                // The global doc-diff report across all docs, with rendered block
                // diffs — the analogue of the original `/docs/diff` payload.
                let report =
                    docgen_diff::build_global_doc_diff_report(&repo, &docs_prefix, limit, true)
                        .with_context(|| {
                            format!("building global doc diff report for {docs_prefix}")
                        })?;
                if let Some(report) = report {
                    let diff_dir = dist_dir.join("diff");
                    fs::create_dir_all(diff_dir.join("revisions"))?;
                    // timeline.json — the lightweight index (hunks/blocks stripped).
                    let summary = docgen_diff::summarize_report(&report);
                    fs::write(
                        diff_dir.join("timeline.json"),
                        serde_json::to_vec(&summary)?,
                    )?;
                    // revisions/<id>.json — each commit's full per-file block diff,
                    // lazily fetched by the island when a commit is selected.
                    for point in &report.timeline {
                        fs::write(
                            diff_dir
                                .join("revisions")
                                .join(format!("{}.json", point.id)),
                            serde_json::to_vec(point)?,
                        )?;
                    }
                    // The /diff workspace shell (hydrated client-side by diff.js).
                    let diff_html = renderer.render_diff(&docgen_render::DiffContext {
                        tree: &tree,
                        base: &config.base,
                        site_title: config.title.as_deref().unwrap_or(""),
                        search_enabled: config.features.search,
                    })?;
                    fs::write(diff_dir.join("index.html"), diff_html)?;
                    has_diff = true;
                }
            }
        }
    }

    // Force-layout graph data, computed once when the graph feature is on, and
    // reused by both the home-page embed (Phase 2) and the standalone /graph
    // page (Phase 3). The original docgen surfaces the graph ON the home page
    // (not in the sidebar), so the home doc gets the graph block.
    let graph_payload: Option<(String, usize, usize)> = if config.features.graph {
        let graph_data = site.graph_data(docgen_core::graphlayout::LayoutParams::default());
        let graph_json = docgen_core::graphlayout::graph_data_json(&graph_data);
        Some((graph_json, graph_data.nodes.len(), graph_data.edges.len()))
    } else {
        None
    };

    // Home dashboard data (mirrors the original home: title/stats/sections/recent).
    // "Sections" = top-level folders (the sidebar's grouping), ordered by first
    // appearance, each linking to its first page with a doc count. "Recent" = the
    // first docs in build order (home excluded). Computed once; only the index
    // doc's render consumes it (every other page passes `home: None`).
    let total_links = site.graph.edges.len();
    let mut section_rows: Vec<(String, String, usize)> = Vec::new();
    let mut section_idx: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for doc in &site.docs {
        if doc.slug == HOME_SLUG || !doc.slug.contains('/') {
            continue;
        }
        let label = doc.slug.split('/').next().unwrap_or("").to_string();
        match section_idx.get(&label) {
            Some(&i) => section_rows[i].2 += 1,
            None => {
                section_idx.insert(label.clone(), section_rows.len());
                section_rows.push((label, doc.slug.clone(), 1));
            }
        }
    }
    let recent_rows: Vec<(String, String, String)> = site
        .docs
        .iter()
        .filter(|d| d.slug != HOME_SLUG)
        .take(6)
        .map(|d| {
            let section = d
                .slug
                .split_once('/')
                .map(|(s, _)| s.to_string())
                .unwrap_or_default();
            (d.title.clone(), d.slug.clone(), section)
        })
        .collect();
    let home_sections: Vec<HomeSection> = section_rows
        .iter()
        .map(|(label, slug, count)| HomeSection {
            label,
            slug,
            count: *count,
        })
        .collect();
    let home_recent: Vec<HomeRecent> = recent_rows
        .iter()
        .map(|(title, slug, section)| HomeRecent {
            title,
            slug,
            section,
        })
        .collect();

    // Phase 2: render the doc pages, linking to history where one was emitted.
    let empty: Vec<docgen_core::model::Backlink> = Vec::new();
    let mut home_html: Option<String> = None;
    for doc in &site.docs {
        let backlinks = site.graph.backlinks.get(&doc.slug).unwrap_or(&empty);
        let is_home = doc.slug == HOME_SLUG;
        // Only the home doc carries the graph payload (empty graph_json → the
        // template skips the block + the graph island script).
        let (graph_json, graph_node_count, graph_edge_count) = match (is_home, &graph_payload) {
            (true, Some((json, nodes, edges))) => (json.as_str(), *nodes, *edges),
            _ => ("", 0, 0),
        };
        let html = renderer.render_page(&PageContext {
            title: &doc.title,
            // Frontmatter description → page header lede (non-home pages). The home
            // dashboard consumes it via `HomeData.description` instead.
            description: if is_home {
                ""
            } else {
                doc.description.as_deref().unwrap_or("")
            },
            slug: &doc.slug,
            body_html: &doc.body_html,
            tree: &tree,
            backlinks,
            headings: &doc.headings,
            commit: &commit_hash,
            built: &built_stamp,
            has_history: false,
            has_mermaid: doc.has_mermaid,
            has_math: doc.has_math,
            base: &config.base,
            site_title: config.title.as_deref().unwrap_or(""),
            search_enabled: config.features.search,
            has_diff,
            has_components_css,
            has_component_island: doc
                .components_used
                .iter()
                .any(|c| island_components.contains(c)),
            is_home,
            graph_json,
            graph_node_count,
            graph_edge_count,
            home: if is_home {
                Some(HomeData {
                    description: doc.description.as_deref().unwrap_or(""),
                    pages: site.docs.len(),
                    links: total_links,
                    sections: &home_sections,
                    recent: &home_recent,
                })
            } else {
                None
            },
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

    // 404 page: a full app-shell page (sidebar + search + theme) so a miss lands
    // somewhere navigable instead of bare "not found". The dev server serves this
    // on any unresolved path; static hosts (GitHub Pages, Netlify, …) pick up
    // `404.html` by convention too.
    let not_found_html = renderer.render_page(&PageContext {
        title: "404",
        description: "This page could not be found.",
        slug: "404",
        body_html: "<p>The page you’re looking for doesn’t exist or was moved. \
            Use the navigation sidebar or search (<kbd class=\"docgen-kbd\">⌘K</kbd>) \
            to find your way.</p>",
        tree: &tree,
        backlinks: &empty,
        headings: &[],
        commit: &commit_hash,
        built: &built_stamp,
        has_history: false,
        has_mermaid: false,
        has_math: false,
        base: &config.base,
        site_title: config.title.as_deref().unwrap_or(""),
        search_enabled: config.features.search,
        has_diff,
        has_components_css: false,
        has_component_island: false,
        is_home: false,
        graph_json: "",
        graph_node_count: 0,
        graph_edge_count: 0,
        home: None,
    })?;
    fs::write(dist_dir.join("404.html"), not_found_html)?;

    // Phase 3: the standalone /graph/ page (default-on, gated off by `[features]
    // graph = false`). Reuses the force layout computed above — never recomputes.
    if let Some((graph_json, node_count, edge_count)) = &graph_payload {
        let graph_html = renderer.render_graph(&GraphContext {
            tree: &tree,
            graph_json,
            node_count: *node_count,
            edge_count: *edge_count,
            base: &config.base,
            site_title: config.title.as_deref().unwrap_or(""),
            search_enabled: config.features.search,
            has_diff,
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
        // Production gates the mermaid lib + island on actual usage. In Dev we
        // ship them unconditionally so the editor's live preview can render a
        // diagram the moment it's typed — before the first save+rebuild that would
        // flip `any_mermaid`. These are production assets (no dev-only surface), so
        // shipping a superset in dev keeps the build's "no dev assets on disk in a
        // static build" invariant (editor/livereload) untouched.
        include_mermaid: site.any_mermaid || opts.mode == BuildMode::Dev,
        include_graph: config.features.graph,
        include_diff: has_diff,
        // Component bundles are written separately (B-8) via emit_component_bundle;
        // these flags are inert in assets_for.
        include_component_css: false,
        include_component_js: false,
        // Honour `[features] search = false`: the page template already gates the
        // search trigger + script link, so the client script would otherwise be an
        // orphan file in the dist.
        include_search: config.features.search,
    };
    docgen_assets::emit(&docgen_assets::assets_for(&emit_opts), dist_dir)?;

    // Authored component bundle (dynamic bytes concatenated from the registry).
    // Empty strings skip their file.
    docgen_assets::emit_component_bundle(dist_dir, &component_css, &component_js)?;

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
