use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Backlink, LinkEdge};

/// The full directed link graph plus the inverted backlinks map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct LinkGraph {
    pub edges: Vec<LinkEdge>,
    pub backlinks: BTreeMap<String, Vec<Backlink>>,
}

/// Build a LinkGraph from per-doc resolved outbound targets.
/// `docs`: (slug, title) for every doc. `outbound`: slug -> resolved target slugs.
/// Self-links are dropped. Edges are sorted (from, to); backlink lists sorted by
/// linking slug. Deterministic.
pub fn build_link_graph(
    docs: &[(String, String)],
    outbound: &BTreeMap<String, Vec<String>>,
) -> LinkGraph {
    let title_of: BTreeMap<&str, &str> =
        docs.iter().map(|(s, t)| (s.as_str(), t.as_str())).collect();

    let mut edges: Vec<LinkEdge> = Vec::new();
    let mut backlinks: BTreeMap<String, Vec<Backlink>> = BTreeMap::new();

    for (from, targets) in outbound {
        for to in targets {
            if to == from {
                continue;
            }
            edges.push(LinkEdge { from: from.clone(), to: to.clone() });
            let title = title_of.get(from.as_str()).copied().unwrap_or(from.as_str());
            backlinks
                .entry(to.clone())
                .or_default()
                .push(Backlink { slug: from.clone(), title: title.to_string() });
        }
    }

    edges.sort();
    edges.dedup();
    for list in backlinks.values_mut() {
        list.sort();
        list.dedup();
    }

    LinkGraph { edges, backlinks }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn builds_edges_and_inverted_backlinks() {
        let docs = vec![
            ("index".to_string(), "Home".to_string()),
            ("a".to_string(), "Page A".to_string()),
            ("b".to_string(), "Page B".to_string()),
        ];
        let mut outbound: BTreeMap<String, Vec<String>> = BTreeMap::new();
        outbound.insert("a".into(), vec!["index".into(), "b".into()]);
        outbound.insert("b".into(), vec!["index".into()]);

        let g = build_link_graph(&docs, &outbound);

        assert_eq!(
            g.edges,
            vec![
                LinkEdge { from: "a".into(), to: "b".into() },
                LinkEdge { from: "a".into(), to: "index".into() },
                LinkEdge { from: "b".into(), to: "index".into() },
            ]
        );
        // index is linked from a and b (sorted by linking slug).
        assert_eq!(
            g.backlinks.get("index").unwrap(),
            &vec![
                Backlink { slug: "a".into(), title: "Page A".into() },
                Backlink { slug: "b".into(), title: "Page B".into() },
            ]
        );
        assert_eq!(
            g.backlinks.get("b").unwrap(),
            &vec![Backlink { slug: "a".into(), title: "Page A".into() }]
        );
        assert!(!g.backlinks.contains_key("a"));
    }

    #[test]
    fn backlink_title_falls_back_to_slug_when_meta_missing() {
        // `from` slug "orphan" has no entry in `docs`, so the title-of lookup
        // misses and the backlink title falls back to the slug itself.
        let docs = vec![("index".to_string(), "Home".to_string())];
        let mut outbound = BTreeMap::new();
        outbound.insert("orphan".to_string(), vec!["index".to_string()]);
        let g = build_link_graph(&docs, &outbound);
        assert_eq!(
            g.backlinks.get("index").unwrap(),
            &vec![Backlink { slug: "orphan".into(), title: "orphan".into() }]
        );
    }

    #[test]
    fn self_links_are_dropped() {
        let docs = vec![("a".to_string(), "A".to_string())];
        let mut outbound = BTreeMap::new();
        outbound.insert("a".to_string(), vec!["a".to_string()]);
        let g = build_link_graph(&docs, &outbound);
        assert!(g.edges.is_empty());
        assert!(g.backlinks.is_empty());
    }
}
