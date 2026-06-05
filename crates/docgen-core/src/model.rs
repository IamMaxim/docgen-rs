use serde::Serialize;

/// A discovered markdown file, before processing.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDoc {
    /// Path relative to the docs root, using `/` separators, e.g. `guide/intro.md`.
    pub rel_path: String,
    /// Raw file contents.
    pub raw: String,
}

/// A fully processed document ready to render.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Doc {
    /// Path relative to docs root, e.g. `guide/intro.md`.
    pub rel_path: String,
    /// URL slug without extension, e.g. `guide/intro`.
    pub slug: String,
    /// Resolved page title.
    pub title: String,
    /// Rendered body HTML.
    pub body_html: String,
    /// Whether this doc contains math (drives the conditional KaTeX `<head>` link).
    pub has_math: bool,
    /// Whether this doc contains a mermaid diagram (drives the lazy island load).
    pub has_mermaid: bool,
    /// Names of custom components rendered on this page (drives per-page island load).
    #[serde(default)]
    pub components_used: std::collections::BTreeSet<String>,
}

/// A node in the sidebar tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TreeNode {
    Dir { name: String, children: Vec<TreeNode> },
    Doc { name: String, slug: String, title: String },
}

/// One resolved wikilink edge: `from` doc links to `to` doc (both slugs).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LinkEdge {
    pub from: String,
    pub to: String,
}

/// Per-target inbound reference, for rendering a "Backlinks" section.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Backlink {
    pub slug: String,
    pub title: String,
}

/// One entry in the static search index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchEntry {
    pub slug: String,
    pub title: String,
    pub text: String,
}
