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

pub mod incremental;

pub use incremental::{DevState, RebuildKind, Rebuilt};

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Local;
use docgen_core::discover::{discover_assets, discover_bases, discover_docs, BaseFileInput};
use docgen_core::model::{Doc, SearchEntry};
use docgen_core::pipeline::{prepare, render_docs};
use docgen_core::tree::build_tree;
use docgen_render::{
    GraphContext, HomeData, HomeRecent, HomeSection, PageContext, Renderer, DEFAULT_PAGE_TEMPLATE,
};

/// The slug docgen treats as the site home (served at `/`).
const HOME_SLUG: &str = "index";

/// Lightweight per-phase build timer. Inert unless `DOCGEN_TIMINGS` is set in
/// the environment, in which case each [`mark`](PhaseTimer::mark) records the
/// elapsed time since the previous mark and [`report`](PhaseTimer::report)
/// prints the breakdown to stderr. Used to profile where a (re)build spends its
/// time — see the dev-server incremental-rebuild work.
struct PhaseTimer {
    enabled: bool,
    last: std::time::Instant,
    start: std::time::Instant,
    rows: Vec<(&'static str, u128)>,
}

impl PhaseTimer {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            enabled: std::env::var_os("DOCGEN_TIMINGS").is_some(),
            last: now,
            start: now,
            rows: Vec::new(),
        }
    }

    fn mark(&mut self, label: &'static str) {
        if !self.enabled {
            return;
        }
        let now = std::time::Instant::now();
        self.rows
            .push((label, now.duration_since(self.last).as_millis()));
        self.last = now;
    }

    fn report(&self) {
        if !self.enabled {
            return;
        }
        let total = self.start.elapsed().as_millis();
        eprintln!("── build timings (ms) ──");
        for (label, ms) in &self.rows {
            eprintln!("  {label:<16} {ms:>6}");
        }
        eprintln!("  {:<16} {total:>6}", "TOTAL");
    }
}

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

/// Copy every authored static asset (non-`.md` file) from `docs_dir` into
/// `out_dir`, mirroring its relative path. Parent dirs are created as needed. A
/// clean-URL page like `docs/system/index.md` (served at `/system/index/`) can
/// then reference `./attachments/img.png`, which the asset pass rewrote to
/// `/system/attachments/img.png` — exactly where this writes the file.
pub(crate) fn copy_assets(docs_dir: &Path, out_dir: &Path) -> Result<()> {
    let assets = discover_assets(docs_dir)
        .with_context(|| format!("discovering assets in {}", docs_dir.display()))?;
    for asset in &assets {
        let dest = out_dir.join(&asset.rel_path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating asset dir {}", parent.display()))?;
        }
        fs::copy(&asset.src_path, &dest).with_context(|| {
            format!(
                "copying asset {} -> {}",
                asset.src_path.display(),
                dest.display()
            )
        })?;
    }
    Ok(())
}

/// Read filesystem facts for a doc (size + creation/modification time) for the
/// bases `file.size`/`file.ctime`/`file.mtime` properties. Best-effort: any field
/// that can't be read (or isn't available on the platform) is left absent. Times
/// are epoch-milliseconds.
fn file_facts(docs_dir: &Path, rel_path: &str) -> docgen_core::FileFacts {
    let path = docs_dir.join(rel_path);
    let Ok(meta) = fs::metadata(&path) else {
        return docgen_core::FileFacts::default();
    };
    let to_ms = |t: std::io::Result<std::time::SystemTime>| -> Option<i64> {
        t.ok()
            .and_then(|st| st.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
    };
    docgen_core::FileFacts {
        size: meta.len(),
        ctime_ms: to_ms(meta.created()),
        mtime_ms: to_ms(meta.modified()),
    }
}

/// Render every discovered `.base` file to a synthetic [`Doc`] (its views as
/// static HTML) plus a search entry. The pages flow through the same tree/page
/// pipeline as markdown docs, so a `.base` appears in the sidebar and gets a
/// clean-URL page. `corpus` is the note set the bases query (markdown docs only —
/// bases never query other bases).
fn render_base_pages(
    base_inputs: &[BaseFileInput],
    corpus: &docgen_bases::Corpus,
    base_path: &str,
    taken_slugs: &std::collections::BTreeSet<String>,
) -> (Vec<Doc>, Vec<SearchEntry>) {
    let opts = docgen_bases::RenderOptions {
        base: base_path.to_string(),
        default_view_name: String::new(),
        interactive: true,
        // A standalone `.base` file is the only base on its own page.
        block_index: 0,
    };
    let mut docs = Vec::with_capacity(base_inputs.len());
    let mut search = Vec::with_capacity(base_inputs.len());
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for b in base_inputs {
        // A `.base` whose slug collides with a markdown page (or a previously seen
        // base, or the site home) would overwrite that page's output. Skip it with
        // a warning rather than corrupt the site.
        if taken_slugs.contains(&b.slug) || !seen.insert(b.slug.clone()) {
            eprintln!(
                "bases: skipping {} — its slug `{}` collides with another page",
                b.rel_path, b.slug
            );
            continue;
        }
        let title = b.slug.rsplit('/').next().unwrap_or(&b.slug).to_string();
        let body_html = docgen_bases::render_base_source(&b.source, corpus, &opts);
        docs.push(Doc {
            rel_path: b.rel_path.clone(),
            slug: b.slug.clone(),
            title: title.clone(),
            description: None,
            body_html,
            has_math: false,
            has_mermaid: false,
            components_used: Default::default(),
            headings: Vec::new(),
            // A `.base` page is always shown in the sidebar; it's typically the
            // one entry meant to surface a directory whose pages are hidden.
            hidden_from_sidebar: false,
        });
        search.push(SearchEntry {
            slug: b.slug.clone(),
            title,
            // The base's data lives in the rendered HTML; index the title only.
            text: String::new(),
        });
    }
    (docs, search)
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

/// Whole-site inputs shared by every per-page render — everything a
/// [`PageContext`] needs that is NOT derived from the individual [`Doc`]. Built
/// once per (re)build and reused for every page, so the dev server's incremental
/// path can re-render a single changed page (via [`render_one_page`]) without
/// recomputing the rest of the site.
pub(crate) struct PageShared<'a> {
    pub tree: &'a [docgen_core::model::TreeNode],
    pub graph: &'a docgen_core::graph::LinkGraph,
    pub commit: &'a str,
    pub built: &'a str,
    pub base: &'a str,
    pub site_title: &'a str,
    pub search_enabled: bool,
    pub has_diff: bool,
    pub has_components_css: bool,
    pub island_components: &'a std::collections::BTreeSet<String>,
    pub graph_payload: &'a Option<(String, usize, usize)>,
    pub home_sections: &'a [HomeSection<'a>],
    pub home_recent: &'a [HomeRecent<'a>],
    pub pages_count: usize,
    pub total_links: usize,
}

/// Render ONE doc to its full-page HTML. The single source of truth shared by
/// the full-build page loop and the dev server's incremental re-render, so a
/// page rebuilt incrementally is byte-identical to the same page in a full
/// build. Pure (no disk I/O); the caller writes the result.
pub(crate) fn render_one_page(
    renderer: &Renderer,
    shared: &PageShared,
    doc: &docgen_core::model::Doc,
) -> Result<String> {
    // A doc with no inbound links has no backlinks entry; borrow a shared empty.
    static EMPTY: Vec<docgen_core::model::Backlink> = Vec::new();
    let backlinks = shared.graph.backlinks.get(&doc.slug).unwrap_or(&EMPTY);
    let is_home = doc.slug == HOME_SLUG;
    // Only the home doc carries the graph payload (empty graph_json → the
    // template skips the block + the graph island script).
    let (graph_json, graph_node_count, graph_edge_count) = match (is_home, shared.graph_payload) {
        (true, Some((json, nodes, edges))) => (json.as_str(), *nodes, *edges),
        _ => ("", 0, 0),
    };
    Ok(renderer.render_page(&PageContext {
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
        tree: shared.tree,
        backlinks,
        headings: &doc.headings,
        commit: shared.commit,
        built: shared.built,
        has_history: false,
        has_mermaid: doc.has_mermaid,
        // Interactive base marker: the emitter's payload wrapper. Match the full
        // opening tag (literal `<`) so prose/code that merely mentions the class
        // name — where `<` is HTML-escaped to `&lt;` — never false-positives.
        // Covers both `.base` pages and regular docs embedding a ```base block.
        has_base_island: doc
            .body_html
            .contains("<script type=\"application/json\" class=\"docgen-base-data\">"),
        has_math: doc.has_math,
        base: shared.base,
        site_title: shared.site_title,
        search_enabled: shared.search_enabled,
        has_diff: shared.has_diff,
        has_components_css: shared.has_components_css,
        has_component_island: doc
            .components_used
            .iter()
            .any(|c| shared.island_components.contains(c)),
        is_home,
        graph_json,
        graph_node_count,
        graph_edge_count,
        home: if is_home {
            Some(HomeData {
                description: doc.description.as_deref().unwrap_or(""),
                pages: shared.pages_count,
                links: shared.total_links,
                sections: shared.home_sections,
                recent: shared.home_recent,
            })
        } else {
            None
        },
    })?)
}

/// Compute the home dashboard's section + recent rows from the doc set (owned, so
/// the caller can hold them across the borrowed [`HomeSection`]/[`HomeRecent`]
/// views). "Sections" = top-level folders ordered by first appearance, each with
/// a doc count and a link to its first page; "Recent" = the first 6 docs in build
/// order (home excluded). Shared by the full build and the incremental engine.
#[allow(clippy::type_complexity)]
pub(crate) fn compute_home_rows(
    docs: &[docgen_core::model::Doc],
) -> (Vec<(String, String, usize)>, Vec<(String, String, String)>) {
    let mut section_rows: Vec<(String, String, usize)> = Vec::new();
    let mut section_idx: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for doc in docs {
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
    let recent_rows: Vec<(String, String, String)> = docs
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
    (section_rows, recent_rows)
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
    Ok(build_site_inner(opts, false)?.0)
}

/// The full-build implementation behind [`build_site`]. When `capture` is true it
/// additionally returns a [`CapturedBuild`] holding the in-memory artifacts (docs,
/// graph, layout, tree, …) so the dev server can seed its incremental engine from
/// the initial build without a second pass. `capture = false` (the production
/// path) clones nothing extra.
pub(crate) fn build_site_inner(
    opts: &BuildOptions,
    capture: bool,
) -> Result<(BuildOutcome, Option<crate::incremental::CapturedBuild>)> {
    let docs_dir = opts.project_root.join("docs");
    let final_dir = opts.out_dir;
    let mut t = PhaseTimer::new();

    let raws = discover_docs(&docs_dir)
        .with_context(|| format!("reading docs from {}", docs_dir.display()))?;
    t.mark("discover");

    // Split include-only partials (`_*.md`) out of the page set; they render
    // only where a `:include` transcludes them, never as standalone pages.
    let (pages, partials) = docgen_core::pipeline::partition_partials(raws);
    // Two-pass: prepare all pages, then render with full slug knowledge.
    let prepared: Vec<_> = pages.into_iter().map(prepare).collect();
    t.mark("prepare");
    // Only the incremental engine needs the prepared docs preserved past the
    // render pass; the production path skips the clone.
    let prepared_cap = if capture {
        Some(prepared.clone())
    } else {
        None
    };
    // Load `docgen.toml` (absent → defaults reproduce pre-P6 behaviour).
    let mut config = docgen_config::load(opts.project_root)
        .with_context(|| format!("loading docgen.toml from {}", opts.project_root.display()))?;
    // Resolve the effective deploy base: DOCGEN_BASE override → docgen.toml `base`
    // → GitLab Pages auto-detect (CI_PAGES_URL / CI_PROJECT_PATH) → root. This is
    // what makes a sub-path Pages deploy work with no per-project CI config, and
    // it normalizes hand-written `base` values too.
    config.base = docgen_config::resolve_base(&config.base);
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
    t.mark("config+registry");
    // --- S3 asset offload decision (only affects non-.md attachments) -------
    // `s3_manifest` is Some only when the `s3` feature is on AND [s3] config is
    // present. `s3_creds` additionally requires credentials in the environment.
    #[cfg(feature = "s3")]
    let (s3_manifest, s3_creds): (Option<docgen_s3::AssetManifest>, _) = match &config.s3 {
        Some(s3cfg) if opts.mode == BuildMode::Production => {
            let assets = discover_assets(&docs_dir)
                .with_context(|| format!("discovering assets in {}", docs_dir.display()))?;
            let manifest =
                docgen_s3::build_manifest(&assets, s3cfg).context("building S3 asset manifest")?;
            let creds = docgen_s3::credentials_from_env();
            (Some(manifest), creds)
        }
        _ => (None, None),
    };

    #[cfg(not(feature = "s3"))]
    if config.s3.is_some() && opts.mode == BuildMode::Production {
        eprintln!(
            "S3: [s3] configured but this docgen build was compiled without the `s3` \
             feature — copying attachments into dist/ instead"
        );
    }

    #[cfg(feature = "s3")]
    let resolver: Option<&dyn docgen_core::asseturl::AssetUrlResolver> =
        match (&s3_manifest, &s3_creds) {
            // Upload active: rewrite to S3 URLs.
            (Some(m), Some(_)) => Some(m),
            // Config present but no creds: fall back to local URLs.
            _ => None,
        };
    #[cfg(not(feature = "s3"))]
    let resolver: Option<&dyn docgen_core::asseturl::AssetUrlResolver> = None;

    // --- PlantUML build-time rendering ------------------------------------
    // Load `.puml` sources (docs-relative → source), and, when the feature is on,
    // construct the networked renderer pointed at the resolved server. `support`
    // is `None` when the feature is off, which makes every `:::plantuml` emit a
    // "disabled" notice instead of contacting a server.
    let diagrams = docgen_core::discover::discover_diagrams(&docs_dir)
        .with_context(|| format!("loading PlantUML sources in {}", docs_dir.display()))?;
    let plantuml_server = if config.features.plantuml {
        Some(docgen_config::resolve_plantuml_server(
            &config.plantuml.server,
        ))
    } else {
        None
    };
    // --- Obsidian Bases ---------------------------------------------------
    // Build the corpus (notes queryable by `.base` files and ```base blocks) from
    // the prepared docs plus filesystem metadata, and load the `.base` files. Both
    // are feature-gated: when `bases` is off the corpus is `None` (embedded blocks
    // render as plain code) and no `.base` pages are emitted.
    let bases_corpus = if config.features.bases {
        Some(docgen_core::build_corpus(&prepared, &|rel| {
            file_facts(&docs_dir, rel)
        }))
    } else {
        None
    };
    let base_inputs = if config.features.bases {
        discover_bases(&docs_dir)
            .with_context(|| format!("loading .base files in {}", docs_dir.display()))?
    } else {
        Vec::new()
    };

    // Render in a scope so the renderer/support borrows of `diagrams` end before
    // `diagrams` is (optionally) moved into the captured build below.
    let mut site = {
        let plantuml_renderer = plantuml_server.as_ref().map(|server| {
            docgen_plantuml::HttpRenderer::new(server.clone(), opts.project_root.join(".docgen"))
        });
        let plantuml_support =
            plantuml_renderer
                .as_ref()
                .map(|r| docgen_core::plantuml::PlantumlSupport {
                    diagrams: &diagrams,
                    renderer: Some(r as &dyn docgen_core::PlantumlRenderer),
                });
        render_docs(
            prepared,
            &partials,
            &config,
            &registry,
            resolver,
            plantuml_support.as_ref(),
            bases_corpus.as_ref(),
        )
    };
    t.mark("render_docs");

    // Render each `.base` file to a page (its views as static HTML) and a search
    // entry, querying the markdown corpus. Kept as a separate list from `site.docs`
    // so the link graph (and the dev server's incremental equivalence check) stays
    // computed from markdown docs alone; base pages carry no links.
    let (base_pages, base_search): (Vec<Doc>, Vec<SearchEntry>) = match &bases_corpus {
        Some(corpus) => {
            // Slugs already claimed by markdown pages — a `.base` must not overwrite one.
            let taken: std::collections::BTreeSet<String> =
                site.docs.iter().map(|d| d.slug.clone()).collect();
            render_base_pages(&base_inputs, corpus, &config.base, &taken)
        }
        None => (Vec::new(), Vec::new()),
    };
    // Fold base search entries into the site index.
    site.search.extend(base_search);
    // Whether any base consumer exists (a `.base` page or an embedded ```base
    // block). Used by the dev server: because a base queries the whole corpus
    // (frontmatter + body tags/links), any content edit invalidates it, so a
    // consumer present forces a full rebuild instead of the incremental fast path.
    let has_base_consumers = config.features.bases
        && (!base_inputs.is_empty()
            || site
                .docs
                .iter()
                .any(|d| d.body_html.contains("docgen-base")));
    t.mark("render_bases");

    // Every downstream consumer (sidebar tree, page loop, home rows, page count)
    // treats markdown docs and base pages uniformly.
    let all_docs: Vec<Doc> = site
        .docs
        .iter()
        .cloned()
        .chain(base_pages.iter().cloned())
        .collect();
    // Whether any rendered doc carries an interactive base payload (a `.base`
    // page or an embedded ```base block). Gates shipping the bases island asset.
    // Match the full payload wrapper tag (literal `<`) so a doc that only mentions
    // the class name in prose/code (where `<` is escaped to `&lt;`) can't trip it.
    let any_base = all_docs.iter().any(|d| {
        d.body_html
            .contains("<script type=\"application/json\" class=\"docgen-base-data\">")
    });
    let tree = build_tree(&all_docs);
    t.mark("build_tree");

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
                t.mark("diff_report");
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
    t.mark("graph_layout");

    // Home dashboard data (mirrors the original home: title/stats/sections/recent).
    // "Sections" = top-level folders (the sidebar's grouping), ordered by first
    // appearance, each linking to its first page with a doc count. "Recent" = the
    // first docs in build order (home excluded). Computed once; only the index
    // doc's render consumes it (every other page passes `home: None`).
    let total_links = site.graph.edges.len();
    let (section_rows, recent_rows) = compute_home_rows(&all_docs);
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

    let shared = PageShared {
        tree: &tree,
        graph: &site.graph,
        commit: &commit_hash,
        built: &built_stamp,
        base: &config.base,
        site_title: config.title.as_deref().unwrap_or(""),
        search_enabled: config.features.search,
        has_diff,
        has_components_css,
        island_components: &island_components,
        graph_payload: &graph_payload,
        home_sections: &home_sections,
        home_recent: &home_recent,
        pages_count: all_docs.len(),
        total_links,
    };

    // Phase 2: render the doc pages via the shared per-page renderer (markdown
    // pages + `.base` pages alike).
    let mut home_html: Option<String> = None;
    for doc in &all_docs {
        let html = render_one_page(&renderer, &shared, doc)?;
        // `guide/intro` -> `dist/guide/intro/index.html` (clean URLs).
        let out_dir = dist_dir.join(&doc.slug);
        fs::create_dir_all(&out_dir)?;
        fs::write(out_dir.join("index.html"), &html)?;
        if doc.slug == HOME_SLUG {
            home_html = Some(html);
        }
    }

    t.mark("render_pages");

    // Copy authored static assets (images, PDFs, …) from the docs tree into the
    // output, mirroring their relative path. Pages reference these relatively
    // (`![](./attachments/img.png)`); the asset pass rewrote those refs to
    // base-absolute URLs pointing exactly here (`/system/attachments/img.png`).
    #[cfg(feature = "s3")]
    {
        match (&s3_manifest, &s3_creds) {
            (Some(manifest), Some(creds)) => {
                let s3cfg = config
                    .s3
                    .as_ref()
                    .expect("s3 config present when manifest is");
                let stats =
                    docgen_s3::upload(manifest, s3cfg, creds).context("uploading assets to S3")?;
                let prefix = s3cfg.prefix.as_deref().unwrap_or("");
                eprintln!(
                    "S3: {} uploaded, {} already present -> s3://{}/{} (public: {})",
                    stats.uploaded, stats.skipped, s3cfg.bucket, prefix, s3cfg.public_url
                );
                // Attachments intentionally NOT copied into dist/ (that is the point).
            }
            (Some(manifest), None) => {
                eprintln!(
                    "S3: [s3] configured but AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY not \
                     set — copying {} attachments into dist/ instead",
                    manifest.entries().len()
                );
                copy_assets(&docs_dir, dist_dir)?;
            }
            (None, _) => copy_assets(&docs_dir, dist_dir)?,
        }
    }
    #[cfg(not(feature = "s3"))]
    copy_assets(&docs_dir, dist_dir)?;
    t.mark("copy_assets");

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
        backlinks: &[],
        headings: &[],
        commit: &commit_hash,
        built: &built_stamp,
        has_history: false,
        has_mermaid: false,
        has_base_island: false,
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
        // Ship the bases island only when a rendered page carries an interactive
        // base payload (marker-based, matching the emitter).
        include_bases: any_base,
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

    t.mark("emit_assets");

    // Everything rendered cleanly: atomically swap staging into place. Only now
    // is the previously-served `final_dir` replaced.
    staging.commit(final_dir)?;
    t.mark("commit");
    t.report();

    let page_count = all_docs.len();
    let any_mermaid = site.any_mermaid;
    let outcome = BuildOutcome {
        page_count,
        any_mermaid,
        out_dir: final_dir.to_path_buf(),
    };

    // Seed the dev server's incremental engine from the artifacts this build
    // already computed — no second render pass.
    let captured = if capture {
        Some(crate::incremental::CapturedBuild {
            diagrams,
            plantuml_server,
            bases_corpus,
            base_inputs,
            base_pages,
            has_base_consumers,
            config,
            registry,
            partials,
            prepared: prepared_cap.expect("prepared is cloned whenever capture is set"),
            docs: site.docs,
            outbound: site.outbound,
            graph: site.graph,
            tree,
            graph_payload,
            island_components,
            has_components_css,
            commit_hash,
            built_stamp,
            has_diff,
            search: site.search,
        })
    } else {
        None
    };

    Ok((outcome, captured))
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

#[cfg(test)]
mod bases_tests {
    use super::*;

    /// Write a small vault with two book notes, a standalone `.base` file, and a
    /// page that embeds a ```base block, then build it.
    fn build_fixture() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let docs = root.join("docs");
        fs::create_dir_all(docs.join("Books")).unwrap();
        fs::create_dir_all(docs.join("Bases")).unwrap();
        fs::write(docs.join("index.md"), "# Home\n").unwrap();
        fs::write(
            docs.join("Books/Dune.md"),
            "---\ntags: [book]\nrating: 5\n---\n# Dune\n",
        )
        .unwrap();
        fs::write(
            docs.join("Books/Neuromancer.md"),
            "---\ntags: [book]\nrating: 4\n---\n# Neuromancer\n",
        )
        .unwrap();
        fs::write(docs.join("Books/NotABook.md"), "# NotABook\n").unwrap();
        // A standalone `.base` file → its own page.
        fs::write(
            docs.join("Bases/Books.base"),
            "filters:\n  and:\n    - file.hasTag(\"book\")\nviews:\n  - type: table\n    name: All books\n    order: [file.name, note.rating]\n    sort:\n      - property: note.rating\n        direction: DESC\n",
        )
        .unwrap();
        // A page embedding a ```base block.
        fs::write(
            docs.join("gallery.md"),
            "# Gallery\n\n```base\nfilters:\n  and:\n    - file.hasTag(\"book\")\nviews:\n  - type: cards\n    order: [file.name]\n```\n",
        )
        .unwrap();
        let out = build(root).unwrap();
        (tmp, out.out_dir)
    }

    #[test]
    fn standalone_base_file_becomes_a_page() {
        let (_tmp, dist) = build_fixture();
        let page = dist.join("Bases/Books/index.html");
        let html = fs::read_to_string(&page).expect("base page emitted");
        // Scope assertions to the base content (the sidebar nav lists every doc,
        // including the filtered-out note, so check inside the base view only).
        let base = &html[html.find("class=\"docgen-base\"").expect("base present")..];
        assert!(base.contains("docgen-base-table"));
        assert!(base.contains("All books"));
        // Both books present, filtered note excluded from the table.
        assert!(base.contains(">Dune<"));
        assert!(base.contains(">Neuromancer<"));
        assert!(!base.contains("NotABook"));
        // Descending rating sort: Dune (5) before Neuromancer (4).
        assert!(base.find("Dune").unwrap() < base.find("Neuromancer").unwrap());
    }

    #[test]
    fn base_file_is_not_copied_as_an_asset() {
        let (_tmp, dist) = build_fixture();
        assert!(
            !dist.join("Bases/Books.base").exists(),
            ".base files are build inputs, never published assets"
        );
    }

    #[test]
    fn embedded_base_block_renders_inline() {
        let (_tmp, dist) = build_fixture();
        let html = fs::read_to_string(dist.join("gallery/index.html")).unwrap();
        assert!(html.contains("docgen-base-cards"));
        assert!(html.contains(">Dune<"));
        // The raw fence is gone (rendered, not a code block).
        assert!(!html.contains("<code class=\"language-base\""));
    }

    #[test]
    fn base_page_appears_in_sidebar_nav() {
        let (_tmp, dist) = build_fixture();
        // The sidebar tree is embedded in every page; the base page's slug links.
        let home = fs::read_to_string(dist.join("index.html")).unwrap();
        assert!(home.contains("/Bases/Books"));
    }

    #[test]
    fn base_slug_colliding_with_a_markdown_page_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let docs = root.join("docs");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("index.md"), "# Home\n").unwrap();
        // A markdown page and a .base with the SAME slug (`Foo`).
        fs::write(docs.join("Foo.md"), "# Foo Markdown\n").unwrap();
        fs::write(docs.join("Foo.base"), "views:\n  - type: table\n").unwrap();
        let out = build(root).unwrap();
        // The markdown page's content wins; the base did not overwrite it.
        let html = fs::read_to_string(out.out_dir.join("Foo/index.html")).unwrap();
        assert!(html.contains("Foo Markdown"));
        assert!(!html.contains("docgen-base-table"));
    }

    #[test]
    fn bases_feature_off_skips_pages_and_leaves_block_as_code() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let docs = root.join("docs");
        fs::create_dir_all(docs.join("Bases")).unwrap();
        fs::write(root.join("docgen.toml"), "[features]\nbases = false\n").unwrap();
        fs::write(docs.join("index.md"), "# Home\n").unwrap();
        fs::write(docs.join("Bases/X.base"), "views:\n  - type: table\n").unwrap();
        fs::write(
            docs.join("p.md"),
            "# P\n\n```base\nviews:\n  - type: table\n```\n",
        )
        .unwrap();
        let out = build(root).unwrap();
        // No base page emitted.
        assert!(!out.out_dir.join("Bases/X/index.html").exists());
        // The block stays a plain code block.
        let html = fs::read_to_string(out.out_dir.join("p/index.html")).unwrap();
        assert!(!html.contains("docgen-base-table"));
        assert!(html.contains("<code"));
    }
}
