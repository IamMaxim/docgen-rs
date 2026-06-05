use crate::frontmatter::parse_frontmatter;
use crate::markdown::render_markdown;
use crate::model::{Doc, RawDoc};

/// Derive a URL slug from a docs-relative path: strip a trailing `.md`.
pub fn slug_for(rel_path: &str) -> String {
    rel_path.strip_suffix(".md").unwrap_or(rel_path).to_string()
}

/// Extract the text of the first ATX `# ` heading, if any.
fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Process a RawDoc into a renderable Doc.
pub fn assemble(raw: RawDoc) -> Doc {
    let parsed = parse_frontmatter(&raw.raw);
    let slug = slug_for(&raw.rel_path);

    let fm_title = parsed
        .frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let title = fm_title
        .or_else(|| first_h1(&parsed.body))
        .unwrap_or_else(|| {
            // Last path segment of the slug as a final fallback.
            // `rsplit` always yields at least one segment, so `next()` is never None.
            slug.rsplit('/').next().unwrap_or("").to_string()
        });

    let body_html = render_markdown(&parsed.body);

    Doc { rel_path: raw.rel_path, slug, title, body_html }
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
        let with_h1 = assemble(RawDoc { rel_path: "b.md".into(), raw: "# Just Heading\n".into() });
        assert_eq!(with_h1.title, "Just Heading");

        let bare = assemble(RawDoc { rel_path: "c.md".into(), raw: "no heading here\n".into() });
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
