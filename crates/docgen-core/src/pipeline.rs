use std::collections::BTreeMap;

use comrak::{parse_document, Arena};

use crate::frontmatter::parse_frontmatter;
use crate::graph::{build_link_graph, LinkGraph};
use crate::markdown::{comrak_options, format_ast};
use crate::model::{Doc, RawDoc, SearchEntry};
use crate::search::plaintext;
use crate::wikilink::{transform_wikilinks, SlugSet};

/// A document after pass 1: frontmatter parsed, slug/title derived, raw body kept.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedDoc {
    pub rel_path: String,
    pub slug: String,
    pub title: String,
    pub body_md: String,
}

/// The fully assembled site after pass 2.
pub struct SiteBuild {
    pub docs: Vec<Doc>,
    pub graph: LinkGraph,
    pub search: Vec<SearchEntry>,
}

fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.strip_prefix("# ").map(|h| h.trim().to_string()))
}

/// Pass 1: pure per-doc preparation, no cross-doc knowledge.
pub fn prepare(raw: RawDoc) -> PreparedDoc {
    let parsed = parse_frontmatter(&raw.raw);
    let slug = raw
        .rel_path
        .strip_suffix(".md")
        .unwrap_or(&raw.rel_path)
        .to_string();

    let fm_title = parsed
        .frontmatter
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let title = fm_title
        .or_else(|| first_h1(&parsed.body))
        .unwrap_or_else(|| slug.rsplit('/').next().unwrap_or("").to_string());

    PreparedDoc { rel_path: raw.rel_path, slug, title, body_md: parsed.body }
}

/// Pass 2: build the slug set, run the wikilink pass + syntect highlight per doc,
/// assemble the link graph + search index. Input order preserved.
pub fn render_docs(prepared: Vec<PreparedDoc>) -> SiteBuild {
    let slugs: SlugSet = prepared.iter().map(|p| p.slug.clone()).collect();
    let doc_meta: Vec<(String, String)> =
        prepared.iter().map(|p| (p.slug.clone(), p.title.clone())).collect();
    let options = comrak_options();

    let mut docs = Vec::with_capacity(prepared.len());
    let mut outbound: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut search = Vec::with_capacity(prepared.len());

    for p in &prepared {
        // Search plaintext from the raw body (independent arena).
        search.push(SearchEntry {
            slug: p.slug.clone(),
            title: p.title.clone(),
            text: plaintext(&p.body_md),
        });

        // Wikilink AST pass + highlighted HTML.
        let arena = Arena::new();
        let root = parse_document(&arena, &p.body_md, &options);
        let pass = transform_wikilinks(root, &arena, &slugs);
        outbound.insert(p.slug.clone(), pass.resolved);
        let body_html = format_ast(root, &options);

        docs.push(Doc {
            rel_path: p.rel_path.clone(),
            slug: p.slug.clone(),
            title: p.title.clone(),
            body_html,
        });
    }

    let graph = build_link_graph(&doc_meta, &outbound);
    SiteBuild { docs, graph, search }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;

    fn raw(path: &str, body: &str) -> RawDoc {
        RawDoc { rel_path: path.into(), raw: body.into() }
    }

    #[test]
    fn prepare_keeps_raw_body_and_derives_meta() {
        let p = prepare(raw("guide/intro.md", "---\ntitle: Intro\n---\n# H\nbody [[index]]\n"));
        assert_eq!(p.slug, "guide/intro");
        assert_eq!(p.title, "Intro");
        assert!(p.body_md.contains("[[index]]"));
        assert!(!p.body_md.contains("title:")); // frontmatter stripped
    }

    #[test]
    fn render_docs_resolves_links_highlights_and_indexes() {
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw("guide/intro.md", "# Intro\n```rust\nfn x(){}\n```\nBack to [[index]] and [[ghost]].\n")),
        ];
        let site = render_docs(prepared);

        // Doc order preserved.
        assert_eq!(site.docs[0].slug, "index");
        assert_eq!(site.docs[1].slug, "guide/intro");

        // index links to guide/intro (resolved anchor).
        assert!(site.docs[0].body_html.contains(r#"href="/guide/intro""#));
        // intro has highlighted code + a resolved link + a broken span.
        assert!(site.docs[1].body_html.contains("style=\"color:"));
        assert!(site.docs[1].body_html.contains(r#"href="/index""#));
        assert!(site.docs[1].body_html.contains("docgen-wikilink--broken"));

        // Graph: index->guide/intro and guide/intro->index (ghost dropped).
        assert!(site.graph.edges.iter().any(|e| e.from == "index" && e.to == "guide/intro"));
        assert!(site.graph.edges.iter().any(|e| e.from == "guide/intro" && e.to == "index"));
        assert!(!site.graph.edges.iter().any(|e| e.to == "ghost"));

        // Backlinks: index is linked from guide/intro.
        assert_eq!(site.graph.backlinks.get("index").unwrap()[0].slug, "guide/intro");

        // Search index: one entry per doc, plaintext, no markup.
        assert_eq!(site.search.len(), 2);
        let home = site.search.iter().find(|e| e.slug == "index").unwrap();
        assert_eq!(home.title, "Home");
        assert!(home.text.contains("Go to"));
        assert!(!home.text.contains("[["));
    }
}
