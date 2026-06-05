use docgen_core::model::TreeNode;
use minijinja::{context, Environment};
use serde::Serialize;

/// The built-in page template, embedded at compile time.
pub const DEFAULT_PAGE_TEMPLATE: &str = include_str!("../templates/page.html");

/// Everything a single page render needs.
#[derive(Serialize)]
pub struct PageContext<'a> {
    pub title: String,
    pub body_html: String,
    pub tree: &'a [TreeNode],
}

/// Owns a configured minijinja environment with the `page` template registered.
pub struct Renderer {
    env: Environment<'static>,
}

impl Renderer {
    /// Build a renderer from a page-template source string.
    pub fn new(page_template: &str) -> Result<Self, minijinja::Error> {
        let mut env = Environment::new();
        env.add_template_owned("page", page_template.to_string())?;
        Ok(Self { env })
    }

    /// Render one page to a full HTML document.
    pub fn render_page(&self, ctx: &PageContext) -> Result<String, minijinja::Error> {
        let tmpl = self.env.get_template("page")?;
        tmpl.render(context! {
            title => ctx.title,
            body => ctx.body_html,
            tree => ctx.tree,
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
                title: "My Page".into(),
                body_html: "<p>hello</p>".into(),
                tree: &[],
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
            .render_page(&PageContext { title: "X".into(), body_html: String::new(), tree: &tree })
            .unwrap();
        assert!(html.contains(r#"href="/guide/intro""#));
        assert!(html.contains(">Intro</a>"));
    }
}
