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
    /// True if any doc contains a mermaid diagram. Lets the build subcommand flip
    /// `EmitOptions.include_mermaid` once for the whole site.
    pub any_mermaid: bool,
}

impl SiteBuild {
    /// Build the deterministic `GraphData` for the `/graph/` page from this
    /// site's docs (node order = doc order) and its already-built `LinkGraph`.
    /// Never recomputes links.
    pub fn graph_data(
        &self,
        params: crate::graphlayout::LayoutParams,
    ) -> crate::graphlayout::GraphData {
        let meta: Vec<(String, String)> =
            self.docs.iter().map(|d| (d.slug.clone(), d.title.clone())).collect();
        crate::graphlayout::layout_graph(&meta, &self.graph, params)
    }
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
        // Parse the body once. Extract search plaintext from the pristine AST
        // *before* the wikilink pass rewrites `[[...]]` Text nodes into anchors.
        let arena = Arena::new();
        let root = parse_document(&arena, &p.body_md, &options);

        search.push(SearchEntry {
            slug: p.slug.clone(),
            title: p.title.clone(),
            text: plaintext(root),
        });

        // Wikilink AST pass (mutates `root`) + highlighted HTML.
        let pass = transform_wikilinks(root, &arena, &slugs);
        outbound.insert(p.slug.clone(), pass.resolved);
        // Build-time math: replace math nodes with KaTeX HTML before formatting.
        let math_count = crate::mathpass::transform_math(root);
        // Mermaid: replace ```mermaid fences with island containers before formatting.
        let mermaid_count = crate::mermaidpass::transform_mermaid(root);
        let body_html = format_ast(root, &options);

        docs.push(Doc {
            rel_path: p.rel_path.clone(),
            slug: p.slug.clone(),
            title: p.title.clone(),
            body_html,
            has_math: math_count > 0,
            has_mermaid: mermaid_count > 0,
        });
    }

    let graph = build_link_graph(&doc_meta, &outbound);
    let any_mermaid = docs.iter().any(|d| d.has_mermaid);
    SiteBuild { docs, graph, search, any_mermaid }
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

    #[test]
    fn render_docs_renders_math_at_build_time() {
        let prepared = vec![prepare(raw("m.md", "# M\nmass: $E=mc^2$\n"))];
        let site = render_docs(prepared);
        assert!(site.docs[0].body_html.contains("katex"));
        assert!(site.docs[0].has_math);
        assert!(!site.docs[0].body_html.contains("$E=mc^2$"));
    }

    #[test]
    fn render_docs_marks_mermaid_pages_and_site() {
        let prepared = vec![
            prepare(raw("d.md", "# D\n```mermaid\ngraph TD;A-->B;\n```\n")),
            prepare(raw("p.md", "# P\nplain\n")),
        ];
        let site = render_docs(prepared);
        assert!(site.docs[0].has_mermaid && site.docs[0].body_html.contains("docgen-mermaid"));
        assert!(!site.docs[1].has_mermaid);
        assert!(site.any_mermaid);
    }

    #[test]
    fn site_graph_data_matches_docs_and_links() {
        let prepared = vec![
            prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
            prepare(raw("guide/intro.md", "# Intro\nBack to [[index]].\n")),
        ];
        let site = render_docs(prepared);
        let gd = site.graph_data(crate::graphlayout::LayoutParams::default());
        assert_eq!(gd.nodes.len(), 2);
        assert!(gd.nodes.iter().any(|n| n.slug == "index" && n.title == "Home"));
        assert!(gd.nodes.iter().any(|n| n.slug == "guide/intro" && n.title == "Intro"));
        // Reciprocal [[..]] pair collapses to a single undirected edge.
        let is_pair = |e: &crate::graphlayout::GraphDataEdge| {
            (e.from == "index" && e.to == "guide/intro")
                || (e.from == "guide/intro" && e.to == "index")
        };
        assert_eq!(gd.edges.iter().filter(|e| is_pair(e)).count(), 1);
        assert_eq!(gd.edges.len(), 1);
    }

    #[test]
    fn render_docs_without_mermaid_clears_site_flag() {
        let prepared = vec![prepare(raw("p.md", "# P\nplain\n"))];
        let site = render_docs(prepared);
        assert!(!site.any_mermaid);
    }

    #[test]
    fn self_link_renders_anchor_but_no_self_backlink() {
        // A doc that links to its own slug renders a resolved anchor, but the
        // self-edge is dropped from the graph (no self-backlink).
        let prepared = vec![prepare(raw("index.md", "# Home\nBack to [[index]].\n"))];
        let site = render_docs(prepared);

        assert!(site.docs[0].body_html.contains(r#"href="/index""#));
        assert!(!site.graph.edges.iter().any(|e| e.from == "index" && e.to == "index"));
        assert!(!site.graph.backlinks.contains_key("index"));
    }
}
