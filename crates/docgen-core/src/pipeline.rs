use std::collections::BTreeMap;

use comrak::{parse_document, Arena};

use crate::frontmatter::parse_frontmatter;
use crate::graph::{build_link_graph, LinkGraph};
use crate::markdown::{comrak_options, format_ast};
use crate::model::{Doc, RawDoc, SearchEntry};
use crate::search::plaintext;
use crate::wikilink::{transform_wikilinks, SlugSet};

/// Docs-relative path → frontmatter-stripped body, for `:include` targets.
pub type Partials = std::collections::BTreeMap<String, String>;

/// Docs-relative path → raw PlantUML source, for `:::plantuml{src=...}` targets.
/// Preloaded by the build (analogous to [`Partials`]) so the render pipeline
/// itself performs no filesystem reads.
pub type Diagrams = std::collections::BTreeMap<String, String>;

/// A doc is an include-only *partial* (never its own page) when its filename
/// starts with `_`. Only the basename matters — a `_dir/` directory does not
/// hide the pages inside it.
pub fn is_partial_rel(rel_path: &str) -> bool {
    rel_path
        .rsplit('/')
        .next()
        .map(|name| name.starts_with('_'))
        .unwrap_or(false)
}

/// Split discovered raw docs into rendered pages and the include-only partial
/// map (keyed by docs-relative path, frontmatter stripped).
pub fn partition_partials(raws: Vec<RawDoc>) -> (Vec<RawDoc>, Partials) {
    let mut pages = Vec::new();
    let mut partials = Partials::new();
    for raw in raws {
        if is_partial_rel(&raw.rel_path) {
            let body = parse_frontmatter(&raw.raw).body;
            partials.insert(raw.rel_path, body);
        } else {
            pages.push(raw);
        }
    }
    (pages, partials)
}

/// Resolve a relative include `src` against the docs-relative directory
/// `base_dir` into a normalized docs-relative key (no `./`, `..` collapsed). A
/// leading `/` is treated as docs-root-absolute. Returns `None` if the path
/// escapes above the docs root.
pub fn resolve_include_key(base_dir: &str, src: &str) -> Option<String> {
    let src = src.trim();
    let combined = if let Some(rest) = src.strip_prefix('/') {
        rest.to_string()
    } else if base_dir.is_empty() {
        src.to_string()
    } else {
        format!("{base_dir}/{src}")
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

/// A document after pass 1: frontmatter parsed, slug/title derived, raw body kept.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDoc {
    pub rel_path: String,
    pub slug: String,
    pub title: String,
    /// Optional `description:` from frontmatter, surfaced in backlink cards.
    pub description: Option<String>,
    pub body_md: String,
    /// The full parsed frontmatter, retained so the bases engine can build a
    /// queryable corpus of note properties. `Value::Null` when there is none.
    pub frontmatter: serde_yml::Value,
    /// `sidebar: false` in frontmatter — omit this page from the sidebar tree.
    pub hidden_from_sidebar: bool,
}

/// The fully assembled site after pass 2.
pub struct SiteBuild {
    pub docs: Vec<Doc>,
    pub graph: LinkGraph,
    /// Per-doc resolved outbound link targets (slug → target slugs), the input
    /// the link graph was built from. Retained so an incremental rebuild can
    /// reconstruct the graph after re-rendering only the changed docs (swapping
    /// their entries) instead of re-rendering every doc.
    pub outbound: BTreeMap<String, Vec<String>>,
    pub search: Vec<SearchEntry>,
    /// True if any doc contains a mermaid diagram. Lets the build subcommand flip
    /// `EmitOptions.include_mermaid` once for the whole site.
    pub any_mermaid: bool,
    /// True if any doc used ≥1 custom component (gates the components asset slice).
    pub any_components: bool,
}

impl SiteBuild {
    /// Build the deterministic `GraphData` for the `/graph/` page from this
    /// site's docs (node order = doc order) and its already-built `LinkGraph`.
    /// Never recomputes links.
    pub fn graph_data(
        &self,
        params: crate::graphlayout::LayoutParams,
    ) -> crate::graphlayout::GraphData {
        let meta: Vec<(String, String)> = self
            .docs
            .iter()
            .map(|d| (d.slug.clone(), d.title.clone()))
            .collect();
        crate::graphlayout::layout_graph(&meta, &self.graph, params)
    }
}

/// The first `# `-prefixed line of a body, as the build's title fallback sees
/// it. Deliberately a raw line scan (NOT an AST walk): a `# x` line inside a
/// code fence still counts. Public so the linter's `missing-title` rule can
/// mirror the build's derivation exactly instead of drifting on edge cases.
pub fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Pass 1: pure per-doc preparation, no cross-doc knowledge.
pub fn prepare(raw: RawDoc) -> PreparedDoc {
    let parsed = parse_frontmatter(&raw.raw);
    let slug = raw
        .rel_path
        .strip_suffix(".md")
        .unwrap_or(&raw.rel_path)
        .to_string();

    let fm_title = parsed
        .frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let title = fm_title
        .or_else(|| first_h1(&parsed.body))
        .unwrap_or_else(|| slug.rsplit('/').next().unwrap_or("").to_string());

    let description = parsed
        .frontmatter
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // `sidebar: false` opts a page out of the sidebar tree. Any other value
    // (missing, `true`, non-bool) keeps the default: shown.
    let hidden_from_sidebar =
        parsed.frontmatter.get("sidebar").and_then(|v| v.as_bool()) == Some(false);

    PreparedDoc {
        rel_path: raw.rel_path,
        slug,
        title,
        description,
        body_md: parsed.body,
        frontmatter: parsed.frontmatter,
        hidden_from_sidebar,
    }
}

/// Render a markdown fragment (a block directive's inner content) to inner HTML,
/// running the full directive + AST pipeline but emitting no page chrome.
///
/// Wikilinks inside a directive body are resolved against the same site `slugs`
/// and `base` as top-level body content, so `[[target|label]]` becomes a resolved
/// `<a>` (or a broken span) exactly as it would outside a directive. The
/// nested-directive case works because `substitute` recurses through this fn.
///
/// Note: resolved targets discovered inside directive bodies are NOT folded into
/// the link graph / backlinks (the graph is built from the top-level pass only);
/// the rendered HTML is correct, but a wikilink that *only* appears inside a
/// directive body does not yet create a graph edge.
#[allow(clippy::too_many_arguments)]
pub fn render_block_markdown(
    md: &str,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    slugs: &SlugSet,
    partials: &Partials,
    base_dir: &str,
    stack: &[String],
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
    plantuml: Option<&crate::plantuml::PlantumlSupport>,
) -> String {
    let (rewritten, instances) = crate::directivepass::extract(md);
    let options = comrak_options();
    let arena = Arena::new();
    let root = parse_document(&arena, &rewritten, &options);
    // Resolve wikilinks in the directive body the same way top-level body content
    // does, before math/mermaid rewrite their nodes.
    let _pass = transform_wikilinks(root, &arena, slugs, &config.base);
    // Relative asset references inside a directive body resolve against the same
    // source directory the directive/include lives in (`base_dir`).
    crate::assetpass::transform_asset_urls(root, &config.base, base_dir, slugs, asset_urls);
    if config.features.math {
        crate::mathpass::transform_math(root);
    }
    if config.features.mermaid {
        crate::mermaidpass::transform_mermaid(root);
    }
    // Wrap tables in a horizontal-scroll container (also inside directive/include
    // bodies, so a table transcluded via `:include` scrolls like a top-level one).
    crate::tablepass::transform_tables(root, &arena);
    let inner_html = format_ast(root, &options);
    let render_inner = |m: &str| {
        render_block_markdown(
            m, config, registry, slugs, partials, base_dir, stack, asset_urls, plantuml,
        )
    };
    let resolve_include = |src: &str| {
        resolve_include_src(
            src, base_dir, partials, stack, config, registry, slugs, asset_urls, plantuml,
        )
    };
    let render_plantuml = |idx: usize, inst: &crate::directivepass::DirectiveInstance| {
        crate::plantuml::render_directive(
            inst,
            base_dir,
            plantuml,
            &format!("docgen-plantuml-{idx}"),
        )
    };
    let (out, _used) = crate::directivepass::substitute(
        &inner_html,
        &instances,
        registry,
        &render_inner,
        &resolve_include,
        &render_plantuml,
    );
    out
}

/// Resolve `:include{src}` against `base_dir`, render the partial's body through
/// the recursive pipeline. Missing target or an include cycle degrades to an
/// inert error span (never panics). `stack` holds the include keys currently on
/// the rendering path, for cycle detection.
#[allow(clippy::too_many_arguments)]
fn resolve_include_src(
    src: &str,
    base_dir: &str,
    partials: &Partials,
    stack: &[String],
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    slugs: &SlugSet,
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
    plantuml: Option<&crate::plantuml::PlantumlSupport>,
) -> String {
    let key = match resolve_include_key(base_dir, src) {
        Some(k) => k,
        None => return crate::directivepass::error_span("include", "src escapes docs root"),
    };
    if stack.iter().any(|s| s == &key) {
        return crate::directivepass::error_span("include", "include cycle");
    }
    let Some(body) = partials.get(&key) else {
        return crate::directivepass::error_span("include", "missing `src`");
    };
    let mut next = stack.to_vec();
    next.push(key.clone());
    let child_dir = key.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    render_block_markdown(
        body, config, registry, slugs, partials, child_dir, &next, asset_urls, plantuml,
    )
}

/// A single rendered doc plus the by-products the site assembly needs: its
/// search plaintext and the slugs it links out to (for the link graph). Returned
/// by [`render_doc`] so both the whole-site build and the editor live-preview run
/// the *same* per-doc pipeline rather than two drifting copies.
pub struct RenderedDoc {
    pub doc: Doc,
    /// Plaintext extracted from the pristine AST (no markup), for the search index.
    pub search_text: String,
    /// Resolved outbound wikilink target slugs, in document order (for the graph).
    pub resolved_links: Vec<String>,
}

/// Render ONE prepared doc to its final inner HTML, running the full per-doc
/// pipeline: directive pre-pass → parse → search plaintext → headings → wikilink
/// resolve → math → mermaid → format → heading-id stamp → directive substitute.
///
/// `slugs` is the *whole site's* slug set so `[[wikilinks]]` resolve against every
/// doc, not just this one — the caller must build it from all docs. This is the
/// single source of truth the static build ([`render_docs`]) and the dev server's
/// editor preview both call, so a doc previewed in the editor renders byte-for-byte
/// like its published page.
#[allow(clippy::too_many_arguments)]
pub fn render_doc(
    p: &PreparedDoc,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    slugs: &SlugSet,
    partials: &Partials,
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
    plantuml: Option<&crate::plantuml::PlantumlSupport>,
    bases: Option<&docgen_bases::Corpus>,
) -> RenderedDoc {
    let options = comrak_options();

    // Directive pre-pass: rewrite the raw body, replacing each `:::`/`:leaf`
    // directive with an HTML-comment sentinel that survives comrak verbatim.
    let (rewritten, instances) = crate::directivepass::extract(&p.body_md);

    // Parse the (directive-free) body once. Extract search plaintext from the
    // pristine AST *before* the wikilink pass rewrites `[[...]]` Text nodes.
    let arena = Arena::new();
    let root = parse_document(&arena, &rewritten, &options);

    let search_text = plaintext(root);

    // Heading outline for the right-rail TOC. Collected from the pristine
    // AST (after parse, before formatting) so the anchorized ids match what
    // `stamp_heading_ids` writes onto the rendered tags below.
    let headings = crate::headings::collect_headings(root);

    // Wikilink AST pass (mutates `root`) + highlighted HTML.
    let pass = transform_wikilinks(root, &arena, slugs, &config.base);
    let resolved_links = pass.resolved;
    // Rewrite relative asset references (`![](./img.png)`, `[x](./y.pdf)`) to
    // base-absolute URLs resolved against this page's source directory, so they
    // survive clean-URL nesting and point at the copied asset.
    let source_dir = p.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    crate::assetpass::transform_asset_urls(root, &config.base, source_dir, slugs, asset_urls);
    // Build-time math: replace math nodes with KaTeX HTML before formatting.
    let math_count = if config.features.math {
        crate::mathpass::transform_math(root)
    } else {
        0
    };
    // Mermaid: replace ```mermaid fences with island containers before formatting.
    let mermaid_count = if config.features.mermaid {
        crate::mermaidpass::transform_mermaid(root)
    } else {
        0
    };
    // Obsidian Bases: replace ```base fenced blocks with rendered view HTML,
    // computed against the whole-site corpus. Feature-gated + inert without a
    // corpus (embedded bases render as plain code then).
    if config.features.bases {
        if let Some(corpus) = bases {
            crate::basepass::transform_bases(root, corpus, &config.base);
        }
    }
    // Wrap every table in a horizontal-scroll container so wide tables scroll
    // instead of squishing their columns (desktop and mobile alike).
    crate::tablepass::transform_tables(root, &arena);
    let formatted = format_ast(root, &options);
    // Stamp the anchorized ids onto the `<h2>`/`<h3>` tags so the rail TOC +
    // scroll-spy can target them via `h2[id]` / `h3[id]`.
    let formatted = crate::headings::stamp_heading_ids(&formatted, &headings);

    // Directive post-pass: substitute each sentinel with the component's
    // rendered HTML; block inner content + `:include` partials are rendered by
    // the full recursive pipeline. `used` drives per-page island/style gating.
    let base_dir = p.rel_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let stack: Vec<String> = Vec::new();
    let render_inner = |m: &str| {
        render_block_markdown(
            m, config, registry, slugs, partials, base_dir, &stack, asset_urls, plantuml,
        )
    };
    let resolve_include = |src: &str| {
        resolve_include_src(
            src, base_dir, partials, &stack, config, registry, slugs, asset_urls, plantuml,
        )
    };
    let render_plantuml = |idx: usize, inst: &crate::directivepass::DirectiveInstance| {
        crate::plantuml::render_directive(
            inst,
            base_dir,
            plantuml,
            &format!("docgen-plantuml-{idx}"),
        )
    };
    let (body_html, used) = crate::directivepass::substitute(
        &formatted,
        &instances,
        registry,
        &render_inner,
        &resolve_include,
        &render_plantuml,
    );

    RenderedDoc {
        doc: Doc {
            rel_path: p.rel_path.clone(),
            slug: p.slug.clone(),
            title: p.title.clone(),
            description: p.description.clone(),
            body_html,
            has_math: math_count > 0,
            has_mermaid: mermaid_count > 0,
            components_used: used,
            headings,
            hidden_from_sidebar: p.hidden_from_sidebar,
        },
        search_text,
        resolved_links,
    }
}

/// Pass 2: build the slug set, run the wikilink pass + syntect highlight per doc,
/// assemble the link graph + search index. Input order preserved.
#[allow(clippy::too_many_arguments)]
pub fn render_docs(
    prepared: Vec<PreparedDoc>,
    partials: &Partials,
    config: &docgen_config::SiteConfig,
    registry: &docgen_components::Registry,
    asset_urls: Option<&dyn crate::asseturl::AssetUrlResolver>,
    plantuml: Option<&crate::plantuml::PlantumlSupport>,
    bases: Option<&docgen_bases::Corpus>,
) -> SiteBuild {
    let slugs: SlugSet = prepared.iter().map(|p| p.slug.clone()).collect();
    let doc_meta: Vec<(String, String, Option<String>)> = prepared
        .iter()
        .map(|p| (p.slug.clone(), p.title.clone(), p.description.clone()))
        .collect();

    let mut docs = Vec::with_capacity(prepared.len());
    let mut outbound: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut search = Vec::with_capacity(prepared.len());

    for p in &prepared {
        // Same per-doc pipeline the editor preview runs (single source of truth).
        let rendered = render_doc(
            p, config, registry, &slugs, partials, asset_urls, plantuml, bases,
        );
        search.push(SearchEntry {
            slug: p.slug.clone(),
            title: p.title.clone(),
            text: rendered.search_text,
        });
        outbound.insert(p.slug.clone(), rendered.resolved_links);
        docs.push(rendered.doc);
    }

    let graph = build_link_graph(&doc_meta, &outbound);
    let any_mermaid = docs.iter().any(|d| d.has_mermaid);
    let any_components = docs.iter().any(|d| !d.components_used.is_empty());
    SiteBuild {
        docs,
        graph,
        outbound,
        search,
        any_mermaid,
        any_components,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;

    fn raw(path: &str, body: &str) -> RawDoc {
        RawDoc {
            rel_path: path.into(),
            raw: body.into(),
        }
    }

    #[test]
    fn is_partial_rel_detects_underscore_basename() {
        assert!(is_partial_rel("dev/server/_systems.gen.md"));
        assert!(is_partial_rel("_root.md"));
        assert!(!is_partial_rel("dev/server/index.md"));
        assert!(!is_partial_rel("dev/_dir/page.md")); // only the *basename* counts
    }

    #[test]
    fn partition_partials_splits_pages_and_strips_frontmatter() {
        let raws = vec![
            raw("a/index.md", "# Page\n"),
            raw("a/_inc.md", "---\ntitle: x\n---\n## Inc\n"),
        ];
        let (pages, partials) = partition_partials(raws);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].rel_path, "a/index.md");
        assert_eq!(
            partials.get("a/_inc.md").map(String::as_str),
            Some("## Inc\n")
        );
    }

    #[test]
    fn resolve_include_key_normalizes_relative_and_absolute() {
        assert_eq!(
            resolve_include_key("dev/server", "./_s.gen.md").as_deref(),
            Some("dev/server/_s.gen.md")
        );
        assert_eq!(
            resolve_include_key("dev/server", "../_top.md").as_deref(),
            Some("dev/_top.md")
        );
        assert_eq!(
            resolve_include_key("dev/server", "/root/_x.md").as_deref(),
            Some("root/_x.md")
        );
        assert_eq!(resolve_include_key("", "_x.md").as_deref(), Some("_x.md"));
        assert_eq!(resolve_include_key("dev", "../../escape.md"), None); // escapes docs root
    }

    #[test]
    fn prepare_keeps_raw_body_and_derives_meta() {
        let p = prepare(raw(
            "guide/intro.md",
            "---\ntitle: Intro\n---\n# H\nbody [[index]]\n",
        ));
        assert_eq!(p.slug, "guide/intro");
        assert_eq!(p.title, "Intro");
        assert!(p.body_md.contains("[[index]]"));
        assert!(!p.body_md.contains("title:")); // frontmatter stripped
    }

    #[test]
    fn render_doc_matches_render_docs_for_one_doc() {
        // The preview path (render_doc) and the build path (render_docs) must run
        // the identical per-doc pipeline: same body_html, search text, and links.
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw(
                "guide/intro.md",
                "# Intro\n```rust\nfn x(){}\n```\nBack to [[index]] and [[ghost]].\n",
            )),
        ];
        let slugs: SlugSet = prepared.iter().map(|p| p.slug.clone()).collect();
        let cfg = docgen_config::SiteConfig::default();
        let reg = docgen_components::Registry::empty();

        let site = render_docs(
            prepared.clone(),
            &Partials::new(),
            &cfg,
            &reg,
            None,
            None,
            None,
        );
        let single = render_doc(
            &prepared[1],
            &cfg,
            &reg,
            &slugs,
            &Partials::new(),
            None,
            None,
            None,
        );

        assert_eq!(single.doc.body_html, site.docs[1].body_html);
        assert_eq!(single.doc.has_mermaid, site.docs[1].has_mermaid);
        assert_eq!(single.doc.has_math, site.docs[1].has_math);
        assert_eq!(single.doc.headings, site.docs[1].headings);
        assert_eq!(single.search_text, site.search[1].text);
        // Resolved outbound links match what the graph was built from (ghost dropped).
        assert!(single.resolved_links.contains(&"index".to_string()));
        assert!(!single.resolved_links.contains(&"ghost".to_string()));
    }

    #[test]
    fn render_docs_resolves_links_highlights_and_indexes() {
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw(
                "guide/intro.md",
                "# Intro\n```rust\nfn x(){}\n```\nBack to [[index]] and [[ghost]].\n",
            )),
        ];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );

        // Doc order preserved.
        assert_eq!(site.docs[0].slug, "index");
        assert_eq!(site.docs[1].slug, "guide/intro");

        // index links to guide/intro (resolved anchor).
        assert!(site.docs[0].body_html.contains(r#"href="/guide/intro""#));
        // intro has highlighted code (class-based) + a resolved link + a broken span.
        assert!(site.docs[1]
            .body_html
            .contains(r#"<pre class="docgen-code">"#));
        assert!(site.docs[1].body_html.contains(r#"href="/index""#));
        assert!(site.docs[1].body_html.contains("docgen-wikilink--broken"));

        // Graph: index->guide/intro and guide/intro->index (ghost dropped).
        assert!(site
            .graph
            .edges
            .iter()
            .any(|e| e.from == "index" && e.to == "guide/intro"));
        assert!(site
            .graph
            .edges
            .iter()
            .any(|e| e.from == "guide/intro" && e.to == "index"));
        assert!(!site.graph.edges.iter().any(|e| e.to == "ghost"));

        // Backlinks: index is linked from guide/intro.
        assert_eq!(
            site.graph.backlinks.get("index").unwrap()[0].slug,
            "guide/intro"
        );

        // Search index: one entry per doc, plaintext, no markup.
        assert_eq!(site.search.len(), 2);
        let home = site.search.iter().find(|e| e.slug == "index").unwrap();
        assert_eq!(home.title, "Home");
        assert!(home.text.contains("Go to"));
        assert!(!home.text.contains("[["));
    }

    #[test]
    fn render_docs_renders_math_at_build_time() {
        let prepared = vec![prepare(raw("m.md", "# M\nmass: $E=mc^2$\n"))];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(site.docs[0].body_html.contains("katex"));
        assert!(site.docs[0].has_math);
        assert!(!site.docs[0].body_html.contains("$E=mc^2$"));
    }

    #[test]
    fn math_feature_off_skips_build_time_katex() {
        let prepared = vec![prepare(raw("m.md", "# M\n$E=mc^2$\n"))];
        let mut cfg = docgen_config::SiteConfig::default();
        cfg.features.math = false;
        let site = render_docs(
            prepared,
            &Partials::new(),
            &cfg,
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(!site.docs[0].has_math);
        assert!(!site.docs[0].body_html.contains("katex"));
    }

    #[test]
    fn mermaid_feature_off_leaves_code_block() {
        let prepared = vec![prepare(raw(
            "d.md",
            "# D\n```mermaid\ngraph TD;A-->B;\n```\n",
        ))];
        let mut cfg = docgen_config::SiteConfig::default();
        cfg.features.mermaid = false;
        let site = render_docs(
            prepared,
            &Partials::new(),
            &cfg,
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(!site.docs[0].has_mermaid);
        assert!(!site.any_mermaid);
    }

    #[test]
    fn render_docs_marks_mermaid_pages_and_site() {
        let prepared = vec![
            prepare(raw("d.md", "# D\n```mermaid\ngraph TD;A-->B;\n```\n")),
            prepare(raw("p.md", "# P\nplain\n")),
        ];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(site.docs[0].has_mermaid && site.docs[0].body_html.contains("docgen-mermaid"));
        assert!(!site.docs[1].has_mermaid);
        assert!(site.any_mermaid);
    }

    #[test]
    fn site_graph_data_matches_docs_and_links() {
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw("guide/intro.md", "# Intro\nBack to [[index]].\n")),
        ];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        let gd = site.graph_data(crate::graphlayout::LayoutParams::default());
        assert_eq!(gd.nodes.len(), 2);
        assert!(gd
            .nodes
            .iter()
            .any(|n| n.slug == "index" && n.title == "Home"));
        assert!(gd
            .nodes
            .iter()
            .any(|n| n.slug == "guide/intro" && n.title == "Intro"));
        // Reciprocal [[..]] pair collapses to a single undirected edge.
        let is_pair = |e: &crate::graphlayout::GraphDataEdge| {
            (e.from == "index" && e.to == "guide/intro")
                || (e.from == "guide/intro" && e.to == "index")
        };
        assert_eq!(gd.edges.iter().filter(|e| is_pair(e)).count(), 1);
        assert_eq!(gd.edges.len(), 1);
    }

    #[test]
    fn render_docs_without_mermaid_clears_site_flag() {
        let prepared = vec![prepare(raw("p.md", "# P\nplain\n"))];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(!site.any_mermaid);
    }

    #[test]
    fn render_docs_renders_callout_directive_with_inner_markdown() {
        let mut reg = docgen_components::Registry::empty();
        reg.insert(docgen_components::Component::from_parts(
            "callout",
            "<aside class=\"docgen-callout--{{ attrs.type | default('note') }}\">{{ content | safe }}</aside>",
            None,
            None,
        ));
        let prepared = vec![prepare(raw(
            "d.md",
            "# D\n\n:::callout{type=warning}\nBe **careful**.\n:::\n",
        ))];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &reg,
            None,
            None,
            None,
        );
        let h = &site.docs[0].body_html;
        assert!(h.contains("docgen-callout--warning"));
        assert!(h.contains("<strong>careful</strong>")); // inner markdown rendered
        assert!(site.docs[0].components_used.contains("callout"));
        assert!(site.any_components);
    }

    #[test]
    fn unknown_directive_in_doc_yields_error_span_not_crash() {
        let prepared = vec![prepare(raw("d.md", "# D\n\n:nope[x]{}\n"))];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );
        assert!(site.docs[0].body_html.contains("docgen-directive-error"));
        assert!(!site.any_components);
    }

    #[test]
    fn wikilink_outside_directive_still_resolves() {
        let mut reg = docgen_components::Registry::empty();
        reg.insert(docgen_components::Component::from_parts(
            "callout",
            "<aside>{{ content | safe }}</aside>",
            None,
            None,
        ));
        let prepared = vec![
            prepare(raw(
                "index.md",
                "# Home\nSee [[guide]].\n\n:::callout{}\nx\n:::\n",
            )),
            prepare(raw("guide.md", "# Guide\n")),
        ];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &reg,
            None,
            None,
            None,
        );
        assert!(site.docs[0].body_html.contains(r#"href="/guide""#));
    }

    #[test]
    fn wikilink_inside_directive_body_resolves_to_anchor() {
        let mut reg = docgen_components::Registry::empty();
        reg.insert(docgen_components::Component::from_parts(
            "callout",
            "<aside>{{ content | safe }}</aside>",
            None,
            None,
        ));
        let prepared = vec![
            prepare(raw(
                "index.md",
                "# Home\n\n:::callout{}\nSee [[guide/intro|wikilink]] and [[ghost]].\n:::\n",
            )),
            prepare(raw("guide/intro.md", "# Intro\n")),
        ];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &reg,
            None,
            None,
            None,
        );
        let h = &site.docs[0].body_html;
        // The resolved wikilink inside the directive body is a real anchor with the
        // label text, not literal `[[...]]`.
        assert!(h.contains(r#"href="/guide/intro""#));
        assert!(h.contains(r#">wikilink</a>"#));
        assert!(!h.contains("[[guide/intro|wikilink]]"));
        // An unresolved target inside a directive body still gets the broken span.
        assert!(h.contains("docgen-wikilink--broken"));
        assert!(!h.contains("[[ghost]]"));
    }

    #[test]
    fn self_link_renders_anchor_but_no_self_backlink() {
        // A doc that links to its own slug renders a resolved anchor, but the
        // self-edge is dropped from the graph (no self-backlink).
        let prepared = vec![prepare(raw("index.md", "# Home\nBack to [[index]].\n"))];
        let site = render_docs(
            prepared,
            &Partials::new(),
            &docgen_config::SiteConfig::default(),
            &docgen_components::Registry::empty(),
            None,
            None,
            None,
        );

        assert!(site.docs[0].body_html.contains(r#"href="/index""#));
        assert!(!site
            .graph
            .edges
            .iter()
            .any(|e| e.from == "index" && e.to == "index"));
        assert!(!site.graph.backlinks.contains_key("index"));
    }
}
