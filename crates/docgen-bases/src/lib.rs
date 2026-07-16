//! `docgen-bases` — a faithful, dependency-light engine and static HTML renderer
//! for [Obsidian Bases](https://obsidian.md/help/bases).
//!
//! A `.base` file is a YAML document describing filtered, sorted, grouped *views*
//! over a vault's notes, computed from note frontmatter, file metadata, and an
//! expression language (functions, methods, operators, formulas, summaries). This
//! crate parses that YAML, evaluates the expression language over a caller-built
//! [`Corpus`] of [`Note`]s, and renders each view to self-contained HTML — no
//! scripts, no runtime — for build-time inclusion in a static site.
//!
//! The crate is pure: it performs no I/O and knows nothing about docgen. The host
//! (docgen-build / docgen-core) constructs the [`Corpus`] from discovered docs and
//! supplies a [`RenderOptions`] with the site base path.
//!
//! ## Quick start
//! ```
//! use docgen_bases::{render_base_source, Corpus, Note, RenderOptions};
//!
//! let mut note = Note::default();
//! note.slug = "books/dune".into();
//! note.basename = "Dune".into();
//! note.tags = vec!["book".into()];
//! let corpus = Corpus::new(vec![note]);
//!
//! let yaml = "filters:\n  and:\n    - file.hasTag(\"book\")\nviews:\n  - type: table\n";
//! let html = render_base_source(yaml, &corpus, &RenderOptions::default());
//! assert!(html.contains("docgen-base-table"));
//! ```

// Test helpers build `Note`/`View` fixtures by field-reassigning a `default()`,
// which is clearest for large structs; allow it in test code only.
#![cfg_attr(test, allow(clippy::field_reassign_with_default))]

pub mod ast;
pub mod eval;
pub mod filter;
pub mod format;
pub mod functions;
mod interactive;
pub mod lexer;
pub mod model;
pub mod note;
pub mod parser;
pub mod render;
pub mod summary;
pub mod value;

pub use interactive::view_interactive_enabled;
pub use model::{parse_base, BaseFile};
pub use note::{parse_date, parse_wikilink, properties_from_yaml, value_from_yaml, Corpus, Note};
pub use render::{error_block, render_base, render_base_source, RenderOptions};
pub use value::{BaseDate, BaseLink, Value};

/// Errors surfaced by the base engine. Parsing/eval degrade gracefully inside the
/// renderer (producing error blocks / empty cells), so this is mostly for callers
/// that want to parse a base explicitly.
#[derive(Debug, thiserror::Error)]
pub enum BaseError {
    #[error("parsing .base YAML: {0}")]
    Yaml(#[from] serde_yml::Error),
    #[error("parsing expression `{expr}`: {message}")]
    Expr { expr: String, message: String },
}
