use crate::model::{Doc, RawDoc};
use crate::pipeline::{prepare, render_docs};

/// Derive a URL slug from a docs-relative path: strip a trailing `.md`.
pub fn slug_for(rel_path: &str) -> String {
    rel_path.strip_suffix(".md").unwrap_or(rel_path).to_string()
}

/// Process a single RawDoc into a renderable Doc (back-compat single-doc path).
/// Wikilinks resolve only against this one doc's slug, so cross-doc links render
/// broken here — full resolution happens via `pipeline::render_docs`.
pub fn assemble(raw: RawDoc) -> Doc {
    let prepared = prepare(raw);
    render_docs(
        vec![prepared],
        &crate::pipeline::Partials::new(),
        &docgen_config::SiteConfig::default(),
        &docgen_components::Registry::empty(),
    )
    .docs
    .pop()
    .expect("one doc in, one doc out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;

    #[test]
    fn slug_strips_md_extension() {
        assert_eq!(slug_for("guide/intro.md"), "guide/intro");
    }

    #[test]
    fn title_prefers_frontmatter() {
        let raw = RawDoc {
            rel_path: "a.md".into(),
            raw: "---\ntitle: From FM\n---\n# From Heading\n".into(),
        };
        let doc = assemble(raw);
        assert_eq!(doc.title, "From FM");
        assert_eq!(doc.slug, "a");
        assert!(doc.body_html.contains("From Heading"));
    }

    #[test]
    fn title_falls_back_to_first_h1_then_slug() {
        let with_h1 = assemble(RawDoc {
            rel_path: "b.md".into(),
            raw: "# Just Heading\n".into(),
        });
        assert_eq!(with_h1.title, "Just Heading");

        let bare = assemble(RawDoc {
            rel_path: "c.md".into(),
            raw: "no heading here\n".into(),
        });
        assert_eq!(bare.title, "c");
    }

    #[test]
    fn frontmatter_without_title_key_falls_back_to_h1() {
        // Frontmatter present but no `title` key -> first H1.
        let doc = assemble(RawDoc {
            rel_path: "d.md".into(),
            raw: "---\nweight: 3\n---\n# H1 Title\n".into(),
        });
        assert_eq!(doc.title, "H1 Title");
    }

    #[test]
    fn non_string_frontmatter_title_falls_back_to_slug() {
        // `title: 42` is not a string -> as_str() is None -> fall through to slug.
        let doc = assemble(RawDoc {
            rel_path: "e.md".into(),
            raw: "---\ntitle: 42\n---\nbody\n".into(),
        });
        assert_eq!(doc.title, "e");
    }
}
