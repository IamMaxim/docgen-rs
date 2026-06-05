use docgen_core::model::{Backlink, TreeNode};
use minijinja::{context, Environment};
use serde::Serialize;

/// The built-in page template, embedded at compile time.
pub const DEFAULT_PAGE_TEMPLATE: &str = include_str!("../templates/page.html");

/// The vendored search client script, emitted to `dist/search.js`.
///
/// Deprecated: assets now flow through the `docgen-assets` crate. Kept for one
/// phase so dependents migrate without breakage. The bytes are byte-identical to
/// `docgen-assets`' embedded copy.
#[deprecated(note = "use docgen-assets::core_assets() / emit()")]
pub const SEARCH_JS: &str = include_str!("../assets/search.js");

/// Minimal stylesheet for wikilinks/backlinks/search, emitted to `dist/docgen.css`.
///
/// Deprecated: assets now flow through the `docgen-assets` crate. Kept for one
/// phase so dependents migrate without breakage. The bytes are byte-identical to
/// `docgen-assets`' embedded copy.
#[deprecated(note = "use docgen-assets::core_assets() / emit()")]
pub const DOCGEN_CSS: &str = include_str!("../assets/docgen.css");

/// The built-in per-doc history-timeline template, embedded at compile time.
pub const DEFAULT_HISTORY_TEMPLATE: &str = include_str!("../templates/history.html");

/// The built-in `/graph/` doc-link-graph template, embedded at compile time.
pub const DEFAULT_GRAPH_TEMPLATE: &str = include_str!("../templates/graph.html");

/// One diff line, render-friendly. `kind`/line numbers are pre-stringified by
/// the caller so `docgen-render` stays free of the `docgen-diff` domain types.
#[derive(Serialize)]
pub struct LineView {
    pub kind: String,
    pub text: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

/// A contiguous diff hunk (run of lines).
#[derive(Serialize)]
pub struct HunkView {
    pub lines: Vec<LineView>,
}

/// One changed file within a timeline point.
#[derive(Serialize)]
pub struct FileView {
    pub path: String,
    pub status: String,
    pub hunks: Vec<HunkView>,
}

/// One commit in the timeline (render-friendly projection of a `DocDiffTimelinePoint`).
#[derive(Serialize)]
pub struct TimelinePointView {
    pub short_hash: String,
    pub subject: String,
    pub author: Option<String>,
    pub date: Option<String>,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub files: Vec<FileView>,
}

/// A labelled bucket of timeline points (e.g. "Today").
#[derive(Serialize)]
pub struct TimelineBucketView {
    pub label: String,
    pub points: Vec<TimelinePointView>,
}

/// Everything the history page render needs.
#[derive(Serialize)]
pub struct HistoryContext<'a> {
    pub title: &'a str,
    pub slug: &'a str,
    pub tree: &'a [TreeNode],
    pub buckets: &'a [TimelineBucketView],
    /// Deployed base path (e.g. `/docs`); `""` → no `<base>` tag (default).
    pub base: &'a str,
    /// Site title; `""` → no `"page — site"` suffix (default).
    pub site_title: &'a str,
}

/// Everything the `/graph/` page render needs. `graph_json` is the serialized
/// `GraphData` embedded verbatim into a `<script type="application/json">` tag.
#[derive(Serialize)]
pub struct GraphContext<'a> {
    pub tree: &'a [TreeNode],
    pub graph_json: &'a str,
    pub node_count: usize,
    pub edge_count: usize,
    /// Deployed base path (e.g. `/docs`); `""` → no `<base>` tag (default).
    pub base: &'a str,
    /// Site title; `""` → no `"page — site"` suffix (default).
    pub site_title: &'a str,
    /// Whether the search UI ships (gates the trigger + `search.js`).
    pub search_enabled: bool,
}

/// Everything a single page render needs.
#[derive(Serialize)]
pub struct PageContext<'a> {
    pub title: &'a str,
    pub slug: &'a str,
    pub body_html: &'a str,
    pub tree: &'a [TreeNode],
    pub backlinks: &'a [Backlink],
    /// Whether this doc has an emitted `/<slug>/history/` page (drives the nav link).
    pub has_history: bool,
    /// Whether this page contains a mermaid diagram (gates the mermaid island script).
    pub has_mermaid: bool,
    /// Whether this page contains math (gates the KaTeX stylesheet `<head>` link).
    pub has_math: bool,
    /// Deployed base path (e.g. `/docs`); `""` → no `<base>` tag (default).
    pub base: &'a str,
    /// Site title; `""` → no `"page — site"` suffix (default).
    pub site_title: &'a str,
    /// Whether the search UI ships (gates the trigger + `search.js`).
    pub search_enabled: bool,
    /// Whether any component shipped a `style.css` (links `/components.css`). The
    /// component stylesheet is small + cacheable, so it links on every page when
    /// present rather than per-page.
    pub has_components_css: bool,
    /// Whether this page used ≥1 component with an `island.js` (links
    /// `/components.js`, gated per-page like the mermaid island).
    pub has_component_island: bool,
}

/// Owns a configured minijinja environment with the `page` template registered.
pub struct Renderer {
    env: Environment<'static>,
}

impl Renderer {
    /// Build a renderer from a page-template source string.
    pub fn new(page_template: &str) -> Result<Self, minijinja::Error> {
        let mut env = Environment::new();
        // Register under a `.html` name so minijinja's default auto-escape callback
        // enables HTML escaping for `{{ title }}`, `{{ node.name }}`, `{{ node.title }}`.
        // `{{ body | safe }}` remains raw, as intended for already-rendered markdown.
        env.add_template_owned("page.html", page_template.to_string())?;
        env.add_template_owned("history.html", DEFAULT_HISTORY_TEMPLATE.to_string())?;
        env.add_template_owned("graph.html", DEFAULT_GRAPH_TEMPLATE.to_string())?;
        Ok(Self { env })
    }

    /// Render one page to a full HTML document.
    pub fn render_page(&self, ctx: &PageContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("page.html")?;
        tmpl.render(context! {
            title => ctx.title,
            body => ctx.body_html,
            slug => ctx.slug,
            tree => ctx.tree,
            backlinks => ctx.backlinks,
            has_history => ctx.has_history,
            has_mermaid => ctx.has_mermaid,
            has_math => ctx.has_math,
            base => ctx.base,
            site_title => ctx.site_title,
            search_enabled => ctx.search_enabled,
            has_components_css => ctx.has_components_css,
            has_component_island => ctx.has_component_island,
        })
    }

    /// Render the `/graph/` doc-link-graph page to a full HTML document.
    ///
    /// `graph_json` is injected raw (the island's `JSON.parse` needs valid JSON,
    /// not HTML-escaped text). To stop a literal `</script>` inside a doc title
    /// from breaking out of the embedding `<script type="application/json">` tag,
    /// `</` is rewritten to `<\/` first — still valid JSON, inert as markup.
    pub fn render_graph(&self, ctx: &GraphContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("graph.html")?;
        let safe_json = ctx.graph_json.replace("</", "<\\/");
        tmpl.render(context! {
            tree => ctx.tree,
            graph_json => safe_json,
            node_count => ctx.node_count,
            edge_count => ctx.edge_count,
            base => ctx.base,
            site_title => ctx.site_title,
            search_enabled => ctx.search_enabled,
        })
    }

    /// Render one doc's history timeline to a full HTML document.
    pub fn render_history(&self, ctx: &HistoryContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("history.html")?;
        tmpl.render(context! {
            title => ctx.title,
            slug => ctx.slug,
            tree => ctx.tree,
            buckets => ctx.buckets,
            base => ctx.base,
            site_title => ctx.site_title,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docgen_core::model::TreeNode;

    fn renderer() -> Renderer {
        Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap()
    }

    #[test]
    fn renders_title_and_body() {
        let html = renderer()
            .render_page(&PageContext {
                title: "My Page",
                slug: "my-page",
                body_html: "<p>hello</p>",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(html.contains("<title>My Page</title>"));
        assert!(html.contains("<p>hello</p>"));
    }

    #[test]
    fn component_asset_links_are_gated() {
        let off = renderer()
            .render_page(&PageContext {
                title: "P",
                slug: "p",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(!off.contains("/components.css"));
        assert!(!off.contains("/components.js"));

        let on = renderer()
            .render_page(&PageContext {
                title: "P",
                slug: "p",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: true,
                has_component_island: true,
            })
            .unwrap();
        assert!(on.contains(r#"<link rel="stylesheet" href="/components.css" />"#));
        assert!(on.contains(r#"<script src="/components.js"></script>"#));
    }

    #[test]
    fn page_title_gets_site_suffix_when_configured() {
        let html = renderer()
            .render_page(&PageContext {
                title: "Intro",
                site_title: "My Docs",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
                base: "",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
            })
            .unwrap();
        assert!(html.contains("<title>Intro — My Docs</title>"));
    }

    #[test]
    fn no_site_title_leaves_plain_title_and_no_base() {
        let html = renderer()
            .render_page(&PageContext {
                title: "Intro",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
                base: "",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
            })
            .unwrap();
        assert!(html.contains("<title>Intro</title>"));
        assert!(!html.contains("<base"));
    }

    #[test]
    fn search_disabled_hides_search_ui() {
        let on = renderer()
            .render_page(&PageContext {
                title: "X",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
                base: "",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
            })
            .unwrap();
        assert!(on.contains("data-docgen-search"));

        let off = renderer()
            .render_page(&PageContext {
                title: "X",
                site_title: "",
                search_enabled: false,
                has_components_css: false,
                has_component_island: false,
                base: "",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
            })
            .unwrap();
        assert!(!off.contains("data-docgen-search"));
        assert!(!off.contains("/search.js"));
    }

    #[test]
    fn base_emits_base_tag() {
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
                base: "/docs",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
            })
            .unwrap();
        assert!(html.contains(r#"<base href="/docs/" />"#));
    }

    #[test]
    fn renders_sidebar_links() {
        let tree = vec![TreeNode::Doc {
            name: "intro".into(),
            slug: "guide/intro".into(),
            title: "Intro".into(),
        }];
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &tree,
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(html.contains(r#"href="/guide/intro""#));
        assert!(html.contains(">Intro</a>"));
    }

    #[test]
    fn escapes_title_and_sidebar_text_but_not_body() {
        let tree = vec![TreeNode::Doc {
            name: "intro".into(),
            slug: "guide/intro".into(),
            title: "A & B <x>".into(),
        }];
        let html = renderer()
            .render_page(&PageContext {
                title: "Tom & Jerry <script>",
                slug: "tj",
                body_html: "<p>raw & ok</p>",
                tree: &tree,
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        // Title is HTML-escaped.
        assert!(html.contains("<title>Tom &amp; Jerry &lt;script&gt;</title>"));
        assert!(!html.contains("<title>Tom & Jerry <script>"));
        // Sidebar link text is escaped.
        assert!(html.contains("A &amp; B &lt;x&gt;"));
        // Body marked `| safe` is emitted raw.
        assert!(html.contains("<p>raw & ok</p>"));
    }

    #[test]
    fn renders_backlinks_section() {
        use docgen_core::model::Backlink;
        let backlinks = vec![Backlink {
            slug: "a".into(),
            title: "Page A".into(),
        }];
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &backlinks,
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(html.contains("Backlinks"));
        assert!(html.contains(r#"href="/a""#));
        assert!(html.contains(">Page A</a>"));
    }

    #[test]
    fn omits_backlinks_section_when_empty() {
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(!html.contains("Backlinks"));
    }

    #[test]
    fn renders_history_link_only_when_has_history() {
        let with = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "guide/intro",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: true,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(with.contains(r#"href="/guide/intro/history""#));

        let without = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "guide/intro",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(!without.contains(r#"href="/guide/intro/history""#));
    }

    #[test]
    fn page_loads_bootstrap_and_alpine_and_gates_mermaid_island() {
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(html.contains(r#"src="/bootstrap.js""#));
        assert!(html.contains(r#"src="/vendor/alpine/alpine.min.js""#));
        assert!(!html.contains("islands/mermaid.js")); // gated off

        let withm = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: true,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(withm.contains(r#"src="/islands/mermaid.js""#));
    }

    #[test]
    fn page_links_katex_css_only_when_has_math() {
        let no_math = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(!no_math.contains("katex.min.css"));

        let with_math = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: true,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(with_math.contains(r#"href="/vendor/katex/katex.min.css""#));
    }

    #[test]
    #[allow(deprecated)] // consts kept one phase as byte-identical re-exports
    fn ships_self_contained_search_assets() {
        assert!(SEARCH_JS.contains("search-index.json"));
        assert!(SEARCH_JS.contains("metaKey"));
        assert!(!SEARCH_JS.contains("import ")); // no module imports / npm
        assert!(DOCGEN_CSS.contains("docgen-search-modal"));
    }

    #[test]
    #[allow(deprecated)] // consts kept one phase as byte-identical re-exports
    fn ships_diff_timeline_styles() {
        assert!(DOCGEN_CSS.contains("docgen-diff-line--added"));
        assert!(DOCGEN_CSS.contains("docgen-diff-line--removed"));
    }

    fn sample_buckets() -> Vec<TimelineBucketView> {
        vec![TimelineBucketView {
            label: "Today".into(),
            points: vec![TimelinePointView {
                short_hash: "abc1234".into(),
                subject: "edit a".into(),
                author: Some("docgen test".into()),
                date: Some("2026-05-15".into()),
                added_lines: 1,
                removed_lines: 1,
                files: vec![FileView {
                    path: "docs/a.md".into(),
                    status: "modified".into(),
                    hunks: vec![HunkView {
                        lines: vec![
                            LineView {
                                kind: "context".into(),
                                text: "# A".into(),
                                old_line: Some(1),
                                new_line: Some(1),
                            },
                            LineView {
                                kind: "removed".into(),
                                text: "first".into(),
                                old_line: Some(2),
                                new_line: None,
                            },
                            LineView {
                                kind: "added".into(),
                                text: "second".into(),
                                old_line: None,
                                new_line: Some(2),
                            },
                        ],
                    }],
                }],
            }],
        }]
    }

    // ---- P4 B-4: /graph/ page ----

    #[test]
    fn renders_graph_page_with_embedded_json_and_island() {
        let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
        let json = r#"{"nodes":[{"slug":"a","title":"A","x":1.0,"y":2.0,"degree":0}],"edges":[]}"#;
        let html = r
            .render_graph(&GraphContext {
                tree: &[],
                graph_json: json,
                node_count: 1,
                edge_count: 0,
                base: "",
                site_title: "",
                search_enabled: true,
            })
            .unwrap();
        assert!(html.contains("<title>Graph</title>"));
        assert!(html.contains(r#"id="docgen-graph-data""#));
        assert!(html.contains(r#"type="application/json""#));
        assert!(html.contains(json)); // JSON embedded verbatim, NOT escaped
        assert!(html.contains(r#"x-data="docgenGraph""#));
        assert!(html.contains(r#"src="/islands/graph.js""#));
        assert!(html.contains(r#"src="/bootstrap.js""#));
        assert!(html.contains(r#"src="/vendor/alpine/alpine.min.js""#));
        assert!(html.contains("1 nodes")); // meta caption
    }

    #[test]
    fn graph_page_renders_sidebar_tree() {
        let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
        let tree = vec![docgen_core::model::TreeNode::Doc {
            name: "intro".into(),
            slug: "guide/intro".into(),
            title: "Intro".into(),
        }];
        let html = r
            .render_graph(&GraphContext {
                tree: &tree,
                graph_json: r#"{"nodes":[],"edges":[]}"#,
                node_count: 0,
                edge_count: 0,
                base: "",
                site_title: "",
                search_enabled: true,
            })
            .unwrap();
        assert!(html.contains(r#"href="/guide/intro""#));
    }

    #[test]
    fn embedded_json_neutralizes_script_close() {
        let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
        let json =
            r#"{"nodes":[{"slug":"x","title":"a</script>b","x":0.0,"y":0.0,"degree":0}],"edges":[]}"#;
        let html = r
            .render_graph(&GraphContext {
                tree: &[],
                graph_json: json,
                node_count: 1,
                edge_count: 0,
                base: "",
                site_title: "",
                search_enabled: true,
            })
            .unwrap();
        assert!(!html.contains("a</script>b")); // raw close-tag must not survive
        assert!(html.contains(r#"a<\/script>b"#)); // escaped form present
    }

    #[test]
    fn graph_page_has_graph_nav_link() {
        let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
        let html = r
            .render_graph(&GraphContext {
                tree: &[],
                graph_json: r#"{"nodes":[],"edges":[]}"#,
                node_count: 0,
                edge_count: 0,
                base: "",
                site_title: "",
                search_enabled: true,
            })
            .unwrap();
        assert!(html.contains(r#"href="/graph""#));
    }

    #[test]
    fn page_has_graph_nav_link() {
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                slug: "x",
                body_html: "",
                tree: &[],
                backlinks: &[],
                has_history: false,
                has_mermaid: false,
                has_math: false,
                base: "",
                site_title: "",
                search_enabled: true,
                has_components_css: false,
                has_component_island: false,
            })
            .unwrap();
        assert!(html.contains(r#"href="/graph""#));
    }

    #[test]
    fn renders_history_timeline_with_buckets_and_diff_lines() {
        let buckets = sample_buckets();
        let html = renderer()
            .render_history(&HistoryContext {
                title: "A",
                slug: "a",
                tree: &[],
                buckets: &buckets,
                base: "",
                site_title: "",
            })
            .unwrap();
        assert!(html.contains("<title>History: A</title>"));
        assert!(html.contains("Today"));
        assert!(html.contains("edit a"));
        assert!(html.contains("abc1234"));
        assert!(html.contains("docgen-diff-line--removed"));
        assert!(html.contains("docgen-diff-line--added"));
        assert!(html.contains("first"));
        assert!(html.contains(r#"href="/a""#));
    }

    #[test]
    fn history_escapes_diff_text() {
        let buckets = vec![TimelineBucketView {
            label: "Today".into(),
            points: vec![TimelinePointView {
                short_hash: "abc1234".into(),
                subject: "edit".into(),
                author: None,
                date: None,
                added_lines: 1,
                removed_lines: 0,
                files: vec![FileView {
                    path: "docs/a.md".into(),
                    status: "modified".into(),
                    hunks: vec![HunkView {
                        lines: vec![LineView {
                            kind: "added".into(),
                            text: "<script>alert(1)</script>".into(),
                            old_line: None,
                            new_line: Some(1),
                        }],
                    }],
                }],
            }],
        }];
        let html = renderer()
            .render_history(&HistoryContext {
                title: "A",
                slug: "a",
                tree: &[],
                buckets: &buckets,
                base: "",
                site_title: "",
            })
            .unwrap();
        assert!(html.contains("&lt;script&gt;alert(1)&lt;&#x2f;script&gt;"));
        assert!(!html.contains("<script>alert(1)</script>"));
    }
}
