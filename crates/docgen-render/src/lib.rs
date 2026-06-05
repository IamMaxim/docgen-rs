use docgen_core::model::{Backlink, TreeNode};
use minijinja::{context, Environment};
use serde::Serialize;

/// The built-in page template, embedded at compile time.
pub const DEFAULT_PAGE_TEMPLATE: &str = include_str!("../templates/page.html");

/// The vendored search client script, emitted to `dist/search.js`.
pub const SEARCH_JS: &str = include_str!("../assets/search.js");

/// Minimal stylesheet for wikilinks/backlinks/search, emitted to `dist/docgen.css`.
pub const DOCGEN_CSS: &str = include_str!("../assets/docgen.css");

/// Everything a single page render needs.
#[derive(Serialize)]
pub struct PageContext<'a> {
    pub title: &'a str,
    pub body_html: &'a str,
    pub tree: &'a [TreeNode],
    pub backlinks: &'a [Backlink],
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
        Ok(Self { env })
    }

    /// Render one page to a full HTML document.
    pub fn render_page(&self, ctx: &PageContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("page.html")?;
        tmpl.render(context! {
            title => ctx.title,
            body => ctx.body_html,
            tree => ctx.tree,
            backlinks => ctx.backlinks,
            search_enabled => true,
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
                body_html: "<p>hello</p>",
                tree: &[],
                backlinks: &[],
            })
            .unwrap();
        assert!(html.contains("<title>My Page</title>"));
        assert!(html.contains("<p>hello</p>"));
    }

    #[test]
    fn renders_sidebar_links() {
        let tree = vec![TreeNode::Doc {
            name: "intro".into(),
            slug: "guide/intro".into(),
            title: "Intro".into(),
        }];
        let html = renderer()
            .render_page(&PageContext { title: "X", body_html: "", tree: &tree, backlinks: &[] })
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
                body_html: "<p>raw & ok</p>",
                tree: &tree,
                backlinks: &[],
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
        let backlinks = vec![Backlink { slug: "a".into(), title: "Page A".into() }];
        let html = renderer()
            .render_page(&PageContext {
                title: "X",
                body_html: "",
                tree: &[],
                backlinks: &backlinks,
            })
            .unwrap();
        assert!(html.contains("Backlinks"));
        assert!(html.contains(r#"href="/a""#));
        assert!(html.contains(">Page A</a>"));
    }

    #[test]
    fn omits_backlinks_section_when_empty() {
        let html = renderer()
            .render_page(&PageContext { title: "X", body_html: "", tree: &[], backlinks: &[] })
            .unwrap();
        assert!(!html.contains("Backlinks"));
    }

    #[test]
    fn ships_self_contained_search_assets() {
        assert!(SEARCH_JS.contains("search-index.json"));
        assert!(SEARCH_JS.contains("metaKey"));
        assert!(!SEARCH_JS.contains("import ")); // no module imports / npm
        assert!(DOCGEN_CSS.contains("docgen-search-modal"));
    }
}
