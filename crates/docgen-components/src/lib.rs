//! Custom-component directive registry. A `Component` is a directory
//! `<name>/{template.html, island.js?, style.css?}`. Built-ins ship embedded in
//! `docgen-assets` and load through the SAME `Component::from_parts` path that
//! reads project components — so built-ins dogfood the mechanism. A project
//! component overrides a built-in of the same name.

use std::collections::BTreeMap;

use minijinja::{context, Environment};
use serde::Serialize;

/// One loaded component.
#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,
    pub template: String,
    pub island_js: Option<String>,
    pub style_css: Option<String>,
}

/// The render inputs for a single directive instance.
#[derive(Debug, Clone, Serialize)]
pub struct DirectiveContext {
    pub attrs: BTreeMap<String, String>,
    /// Rendered inner HTML (block form); empty for leaf form.
    pub content: String,
    /// The `[label]` text (leaf form); empty for block form.
    pub label: String,
    /// Unique per-instance id for island wiring.
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ComponentError {
    #[error("component `{name}`: template render failed: {source}")]
    Render {
        name: String,
        #[source]
        source: minijinja::Error,
    },
}

impl Component {
    /// Build a component from its raw parts (used by BOTH project discovery and
    /// the embedded built-in loader).
    pub fn from_parts(
        name: impl Into<String>,
        template: impl Into<String>,
        island_js: Option<String>,
        style_css: Option<String>,
    ) -> Self {
        Self {
            name: name.into(),
            template: template.into(),
            island_js,
            style_css,
        }
    }

    /// Render this component for one directive instance to HTML.
    ///
    /// The template is registered under a `.html` name so minijinja's default
    /// auto-escape callback HTML-escapes `{{ attrs.x }}` / `{{ label }}`. The
    /// already-rendered inner markdown is exposed as `content`, so a template
    /// author writes `{{ content | safe }}` to splice block content raw.
    pub fn render(&self, ctx: &DirectiveContext) -> Result<String, ComponentError> {
        let mut env = Environment::new();
        env.add_template("c.html", &self.template)
            .map_err(|e| ComponentError::Render {
                name: self.name.clone(),
                source: e,
            })?;
        let tmpl = env
            .get_template("c.html")
            .expect("template just added under this name");
        tmpl.render(context! {
            attrs => &ctx.attrs,
            content => &ctx.content,
            label => &ctx.label,
            id => &ctx.id,
        })
        .map_err(|e| ComponentError::Render {
            name: self.name.clone(),
            source: e,
        })
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn renders_block_component_with_attrs_and_content() {
        let c = Component::from_parts(
            "callout",
            "<aside class=\"c--{{ attrs.type | default('note') }}\">\
             {% if attrs.title %}<p>{{ attrs.title }}</p>{% endif %}\
             <div>{{ content | safe }}</div></aside>",
            None,
            None,
        );
        let html = c
            .render(&DirectiveContext {
                attrs: attrs(&[("type", "warning"), ("title", "Back up first")]),
                content: "<p>destructive</p>".into(),
                label: "".into(),
                id: "d0".into(),
            })
            .unwrap();
        assert!(html.contains("c--warning"));
        assert!(html.contains("Back up first"));
        assert!(html.contains("<p>destructive</p>")); // content raw
    }

    #[test]
    fn renders_leaf_component_with_label() {
        let c = Component::from_parts(
            "youtube",
            "<figure><iframe title=\"{{ label }}\" \
             src=\"https://yt/embed/{{ attrs.id }}\"></iframe><figcaption>{{ label }}</figcaption></figure>",
            None,
            None,
        );
        let html = c
            .render(&DirectiveContext {
                attrs: attrs(&[("id", "abc123")]),
                content: "".into(),
                label: "Intro to docgen".into(),
                id: "d1".into(),
            })
            .unwrap();
        assert!(html.contains("embed/abc123"));
        assert!(html.contains("Intro to docgen"));
    }

    #[test]
    fn attrs_and_label_are_html_escaped() {
        let c = Component::from_parts("x", "<i title=\"{{ label }}\">{{ attrs.a }}</i>", None, None);
        let html = c
            .render(&DirectiveContext {
                attrs: attrs(&[("a", "<script>")]),
                content: "".into(),
                label: "a&b".into(),
                id: "d".into(),
            })
            .unwrap();
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("a&amp;b"));
    }
}
