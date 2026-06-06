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
    /// Frontmatter `description:`, if any. Feeds the home dashboard subtitle and
    /// the "Recent" list. `None` when the doc has no `description:`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Rendered body HTML.
    pub body_html: String,
    /// Whether this doc contains math (drives the conditional KaTeX `<head>` link).
    pub has_math: bool,
    /// Whether this doc contains a mermaid diagram (drives the lazy island load).
    pub has_mermaid: bool,
    /// Names of custom components rendered on this page (drives per-page island load).
    #[serde(default)]
    pub components_used: std::collections::BTreeSet<String>,
    /// The `h2`/`h3` outline of this page, in document order, for the right-rail
    /// "On this page" table of contents. Ids match the `id` attributes stamped
    /// onto the rendered heading tags in `body_html`.
    #[serde(default)]
    pub headings: Vec<crate::headings::Heading>,
}

/// A node in the sidebar tree.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TreeNode {
    Dir {
        name: String,
        /// Slug of this folder's "folder note" — an `index.md` directly inside the
        /// directory. `Some` makes the folder label a link to that page (clicking
        /// the folder focuses its note); `None` is a plain, non-navigable group.
        /// The note doc is NOT also emitted as a child of this dir.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slug: Option<String>,
        children: Vec<TreeNode>,
    },
    Doc {
        name: String,
        slug: String,
        title: String,
    },
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
    /// The linking doc's frontmatter description, rendered as a `<small>` under
    /// the title in the rail's "Referenced by" cards. `None` when the linking
    /// doc has no `description:` in its frontmatter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One entry in the static search index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchEntry {
    pub slug: String,
    pub title: String,
    pub text: String,
}
