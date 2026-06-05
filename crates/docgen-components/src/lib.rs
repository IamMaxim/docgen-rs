//! Custom-component directive registry. A `Component` is a directory
//! `<name>/{template.html, island.js?, style.css?}`. Built-ins ship embedded in
//! `docgen-assets` and load through the SAME `Component::from_parts` path that
//! reads project components — so built-ins dogfood the mechanism. A project
//! component overrides a built-in of the same name.

use std::collections::BTreeMap;
use std::path::Path;

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

/// A name → component map. Built-ins inserted first, project components last
/// (so a project `<name>` overrides a built-in `<name>`).
#[derive(Debug, Clone, Default)]
pub struct Registry {
    map: BTreeMap<String, Component>,
}

impl Registry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Insert (or override) a component by its `name`.
    pub fn insert(&mut self, c: Component) {
        self.map.insert(c.name.clone(), c);
    }

    pub fn get(&self, name: &str) -> Option<&Component> {
        self.map.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// All components with an `island.js`, in BTreeMap name-key order — the
    /// concatenation order for the emitted `components.js` (deterministic).
    pub fn islands(&self) -> Vec<&Component> {
        self.map.values().filter(|c| c.island_js.is_some()).collect()
    }

    /// All components with a `style.css`, in BTreeMap name-key order (deterministic).
    pub fn styles(&self) -> Vec<&Component> {
        self.map.values().filter(|c| c.style_css.is_some()).collect()
    }
}

/// Read every `<name>/` subdir of `dir` into components. `template.html` is
/// required; a subdir without it is skipped (with no error — a stray dir is not
/// fatal). Missing `dir` → no components (empty). Deterministic (sorted names).
pub fn discover(dir: &Path) -> std::io::Result<Vec<Component>> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in rd {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    for name in names {
        let base = dir.join(&name);
        let template = match std::fs::read_to_string(base.join("template.html")) {
            Ok(t) => t,
            Err(_) => continue, // no template.html → not a component
        };
        let island_js = std::fs::read_to_string(base.join("island.js")).ok();
        let style_css = std::fs::read_to_string(base.join("style.css")).ok();
        out.push(Component::from_parts(name, template, island_js, style_css));
    }
    Ok(out)
}

/// Build the full registry: embedded built-ins first, then project components
/// from `project_dir` (which override built-ins by name).
pub fn build_registry(builtins: Vec<Component>, project_dir: &Path) -> std::io::Result<Registry> {
    let mut reg = Registry::empty();
    for c in builtins {
        reg.insert(c);
    }
    for c in discover(project_dir)? {
        reg.insert(c);
    }
    Ok(reg)
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    fn write_component(root: &Path, name: &str, tpl: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("template.html"), tpl).unwrap();
    }

    #[test]
    fn discovers_project_components_sorted_and_requires_template() {
        let dir = tempfile::tempdir().unwrap();
        write_component(dir.path(), "note", "<div>{{ content | safe }}</div>");
        // a stray dir with no template.html is ignored
        std::fs::create_dir_all(dir.path().join("empty")).unwrap();
        let comps = discover(dir.path()).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "note");
    }

    #[test]
    fn missing_components_dir_is_empty_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let comps = discover(&dir.path().join("nope")).unwrap();
        assert!(comps.is_empty());
    }

    #[test]
    fn project_component_overrides_builtin_of_same_name() {
        let dir = tempfile::tempdir().unwrap();
        write_component(
            dir.path(),
            "callout",
            "<div class=\"project-callout\">{{ content | safe }}</div>",
        );
        let builtin =
            Component::from_parts("callout", "<div class=\"builtin-callout\"></div>", None, None);
        let reg = build_registry(vec![builtin], dir.path()).unwrap();
        let c = reg.get("callout").unwrap();
        assert!(c.template.contains("project-callout"));
        assert!(!c.template.contains("builtin-callout"));
    }

    #[test]
    fn picks_up_island_and_style_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().join("rating");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("template.html"), "<div></div>").unwrap();
        std::fs::write(d.join("island.js"), "Alpine.data('r',()=>({}))").unwrap();
        std::fs::write(d.join("style.css"), ".r{}").unwrap();
        let comps = discover(dir.path()).unwrap();
        assert!(comps[0].island_js.is_some());
        assert!(comps[0].style_css.is_some());
        let mut reg = Registry::empty();
        reg.insert(comps.into_iter().next().unwrap());
        assert_eq!(reg.islands().len(), 1);
        assert_eq!(reg.styles().len(), 1);
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
