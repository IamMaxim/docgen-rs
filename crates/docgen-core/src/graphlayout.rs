//! Deterministic force-directed layout for the `/graph/` view.
//!
//! Consumes the already-built [`crate::graph::LinkGraph`] (wikilink edges) plus
//! per-doc `(slug, title)` metadata and runs a fixed-iteration spring layout in
//! pure Rust to produce stable 2D node positions. No RNG, no clock, no
//! `HashMap` iteration in the hot loop: index-ordered `Vec`s + a `BTreeMap` for
//! slug lookups built once. Given the same inputs, [`layout_graph`] produces
//! byte-identical output across runs and machines.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::graph::LinkGraph;

/// One laid-out graph node: a doc plus its 2D position and link degree.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphNode {
    pub slug: String,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub degree: u32,
}

/// One graph edge (undirected for display; carries the directed slugs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphDataEdge {
    pub from: String,
    pub to: String,
}

/// The serializable layout result embedded in `/graph/` and consumed by the island.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphDataEdge>,
}

/// Fixed layout parameters. [`LayoutParams::default`] is the canonical, tested seed.
#[derive(Debug, Clone, Copy)]
pub struct LayoutParams {
    /// Layout width (matches the original viewBox).
    pub width: f64,
    /// Layout height.
    pub height: f64,
    /// Inset kept clear of the clamp walls.
    pub padding: f64,
    /// Fixed iteration count (determinism: never time- or convergence-gated).
    pub iterations: usize,
}

impl Default for LayoutParams {
    fn default() -> Self {
        LayoutParams { width: 1420.0, height: 760.0, padding: 74.0, iterations: 180 }
    }
}

/// Clamp `v` into `[min, max]`. Port of the original `clamp` helper.
fn clamp(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min
    } else if v > max {
        max
    } else {
        v
    }
}

/// Deterministic golden-angle spiral seed for node `i` of `total`, spread within
/// the padded layout box. Purely a function of `(i, total, params)`.
fn seed_point(i: usize, total: usize, params: &LayoutParams) -> (f64, f64) {
    // Golden angle ~= 2.39996 rad.
    let ga = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    let angle = i as f64 * ga;
    let radius_frac = ((i as f64 + 0.5) / total.max(1) as f64).sqrt();
    let cx = params.width / 2.0;
    let cy = params.height / 2.0;
    let rmax = params.width.min(params.height) / 2.0 - params.padding;
    let x = cx + radius_frac * rmax * angle.cos();
    let y = cy + radius_frac * rmax * angle.sin();
    (
        clamp(x, params.padding, params.width - params.padding),
        clamp(y, params.padding, params.height - params.padding),
    )
}

/// Deterministic force-directed layout. `nodes_meta`: `(slug, title)` in a STABLE
/// order (doc input order). `graph`: the already-built [`LinkGraph`]. Returns a
/// [`GraphData`] with positions clamped inside
/// `[padding, width-padding] x [padding, height-padding]`.
pub fn layout_graph(
    nodes_meta: &[(String, String)],
    graph: &LinkGraph,
    params: LayoutParams,
) -> GraphData {
    let n = nodes_meta.len();
    if n == 0 {
        return GraphData { nodes: Vec::new(), edges: Vec::new() };
    }

    // Slug -> index, built once. BTreeMap for stable, no-hash lookup.
    let index_of: BTreeMap<&str, usize> =
        nodes_meta.iter().enumerate().map(|(i, (s, _))| (s.as_str(), i)).collect();

    // Filtered edge list as index pairs: drop self-loops and edges touching a
    // slug not in `nodes_meta` (ghosts can't be drawn). Index order preserved
    // (the LinkGraph edges are already sorted by `build_link_graph`).
    let mut edges_idx: Vec<(usize, usize)> = Vec::with_capacity(graph.edges.len());
    let mut graph_edges: Vec<GraphDataEdge> = Vec::with_capacity(graph.edges.len());
    let mut degree: Vec<u32> = vec![0; n];
    for e in &graph.edges {
        if e.from == e.to {
            continue;
        }
        let (Some(&s), Some(&t)) =
            (index_of.get(e.from.as_str()), index_of.get(e.to.as_str()))
        else {
            continue;
        };
        edges_idx.push((s, t));
        graph_edges.push(GraphDataEdge { from: e.from.clone(), to: e.to.clone() });
        degree[s] += 1;
        degree[t] += 1;
    }

    // Seed positions (deterministic spiral); keep seeds for the anchor pull.
    let seeds: Vec<(f64, f64)> = (0..n).map(|i| seed_point(i, n, &params)).collect();
    let mut xs: Vec<f64> = seeds.iter().map(|s| s.0).collect();
    let mut ys: Vec<f64> = seeds.iter().map(|s| s.1).collect();

    // Fixed-iteration spring relaxation (faithful port of the original graph.ts
    // constants), index-ordered throughout for determinism.
    for _ in 0..params.iterations {
        // 1) repulsion across every unordered pair.
        for a in 0..n {
            for b in (a + 1)..n {
                let mut dx = xs[b] - xs[a];
                let mut dy = ys[b] - ys[a];
                if dx == 0.0 {
                    dx = 0.01;
                }
                if dy == 0.0 {
                    dy = 0.01;
                }
                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt();
                let strength = (2800.0 / dist_sq).min(3.8);
                let px = dx / dist * strength;
                let py = dy / dist * strength;
                xs[a] -= px;
                ys[a] -= py;
                xs[b] += px;
                ys[b] += py;
            }
        }
        // 2) attraction along edges (target length 132, factor 0.018).
        for &(s, t) in &edges_idx {
            let dx = xs[t] - xs[s];
            let dy = ys[t] - ys[s];
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);
            let pull = (dist - 132.0) * 0.018;
            let px = dx / dist * pull;
            let py = dy / dist * pull;
            xs[s] += px;
            ys[s] += py;
            xs[t] -= px;
            ys[t] -= py;
        }
        // 3) anchor pull toward the spiral seed + clamp into bounds.
        for i in 0..n {
            xs[i] += (seeds[i].0 - xs[i]) * 0.012;
            ys[i] += (seeds[i].1 - ys[i]) * 0.006;
            xs[i] = clamp(xs[i], params.padding, params.width - params.padding);
            ys[i] = clamp(ys[i], params.padding, params.height - params.padding);
        }
    }

    // Round to 2dp for byte-stable JSON across platforms + smaller files.
    let round2 = |v: f64| (v * 100.0).round() / 100.0;
    let nodes: Vec<GraphNode> = (0..n)
        .map(|i| GraphNode {
            slug: nodes_meta[i].0.clone(),
            title: nodes_meta[i].1.clone(),
            x: round2(xs[i]),
            y: round2(ys[i]),
            degree: degree[i],
        })
        .collect();

    GraphData { nodes, edges: graph_edges }
}

/// Serialize [`GraphData`] to compact JSON (stable key order via serde derive).
pub fn graph_data_json(data: &GraphData) -> String {
    serde_json::to_string(data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinkEdge;

    fn lg(edges: &[(&str, &str)]) -> LinkGraph {
        LinkGraph {
            edges: edges
                .iter()
                .map(|(f, t)| LinkEdge { from: f.to_string(), to: t.to_string() })
                .collect(),
            backlinks: Default::default(),
        }
    }

    // --- A-1 ---

    #[test]
    fn graphdata_serializes_with_stable_keys() {
        let d = GraphData {
            nodes: vec![GraphNode {
                slug: "a".into(),
                title: "A".into(),
                x: 1.0,
                y: 2.0,
                degree: 3,
            }],
            edges: vec![GraphDataEdge { from: "a".into(), to: "b".into() }],
        };
        let j = graph_data_json(&d);
        assert!(j.contains(r#""slug":"a""#));
        assert!(j.contains(r#""x":1.0"#));
        assert!(j.contains(r#""degree":3"#));
        assert!(j.contains(r#""from":"a""#));
        assert!(j.contains(r#""to":"b""#));
        assert!(!j.contains(",\n"));
        let _v: serde_json::Value = serde_json::from_str(&j).unwrap();
    }

    // --- A-2 ---

    #[test]
    fn degree_counts_each_incident_edge_undirected() {
        let meta = vec![
            ("a".into(), "A".into()),
            ("b".into(), "B".into()),
            ("c".into(), "C".into()),
        ];
        let g = lg(&[("a", "b"), ("a", "c"), ("b", "a")]);
        let d = layout_graph(&meta, &g, LayoutParams::default());
        let deg = |s: &str| d.nodes.iter().find(|n| n.slug == s).unwrap().degree;
        assert_eq!(deg("a"), 3);
        assert_eq!(deg("b"), 2);
        assert_eq!(deg("c"), 1);
    }

    #[test]
    fn self_loops_are_ignored_for_degree_and_edges() {
        let meta = vec![("a".into(), "A".into()), ("b".into(), "B".into())];
        let g = lg(&[("a", "a"), ("a", "b")]);
        let d = layout_graph(&meta, &g, LayoutParams::default());
        assert_eq!(d.nodes.iter().find(|n| n.slug == "a").unwrap().degree, 1);
        assert!(!d.edges.iter().any(|e| e.from == e.to));
        assert_eq!(d.edges.len(), 1);
    }

    #[test]
    fn edges_to_unknown_slugs_are_dropped() {
        let meta = vec![("a".into(), "A".into())];
        let g = lg(&[("a", "ghost")]);
        let d = layout_graph(&meta, &g, LayoutParams::default());
        assert!(d.edges.is_empty());
        assert_eq!(d.nodes.iter().find(|n| n.slug == "a").unwrap().degree, 0);
    }

    // --- A-3 ---

    #[test]
    fn seed_positions_are_deterministic_and_in_bounds() {
        let meta: Vec<_> = (0..7).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
        let g = lg(&[]);
        let p = LayoutParams::default();
        let d1 = layout_graph(&meta, &g, p);
        let d2 = layout_graph(&meta, &g, p);
        assert_eq!(d1, d2);
        for n in &d1.nodes {
            assert!(n.x >= p.padding && n.x <= p.width - p.padding, "x oob: {}", n.x);
            assert!(n.y >= p.padding && n.y <= p.height - p.padding, "y oob: {}", n.y);
        }
    }

    #[test]
    fn single_node_sits_inside_bounds_without_nan() {
        let meta = vec![("only".into(), "Only".into())];
        let d = layout_graph(&meta, &lg(&[]), LayoutParams::default());
        assert_eq!(d.nodes.len(), 1);
        let n = &d.nodes[0];
        assert!(n.x.is_finite() && n.y.is_finite());
    }

    #[test]
    fn empty_graph_yields_empty_graphdata() {
        let d = layout_graph(&[], &lg(&[]), LayoutParams::default());
        assert!(d.nodes.is_empty() && d.edges.is_empty());
        let _v: serde_json::Value = serde_json::from_str(&graph_data_json(&d)).unwrap();
    }

    // --- A-4 ---

    #[test]
    fn connected_pair_settles_near_target_edge_length() {
        let meta = vec![("a".into(), "A".into()), ("b".into(), "B".into())];
        let g = lg(&[("a", "b")]);
        let p = LayoutParams::default();
        let d1 = layout_graph(&meta, &g, p);
        let d2 = layout_graph(&meta, &g, p);
        assert_eq!(d1, d2);
        let a = &d1.nodes[0];
        let b = &d1.nodes[1];
        let dist = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        assert!(dist > 40.0 && dist < 320.0, "pair distance {dist} not in plausible band");
        for n in &d1.nodes {
            assert!(n.x.is_finite() && n.y.is_finite());
        }
    }

    #[test]
    fn all_positions_stay_in_bounds_after_forces() {
        let meta: Vec<_> = (0..20).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
        let owned: Vec<(String, String)> =
            (1..15).map(|i| ("n0".into(), format!("n{i}"))).collect();
        let mut e: Vec<(&str, &str)> = Vec::new();
        for (f, t) in &owned {
            e.push((f, t));
        }
        let g = lg(&e);
        let p = LayoutParams::default();
        let d = layout_graph(&meta, &g, p);
        for n in &d.nodes {
            assert!(n.x >= p.padding - 0.01 && n.x <= p.width - p.padding + 0.01);
            assert!(n.y >= p.padding - 0.01 && n.y <= p.height - p.padding + 0.01);
        }
    }

    #[test]
    fn disconnected_components_do_not_collapse_to_a_point() {
        let meta = vec![
            ("a".into(), "A".into()),
            ("b".into(), "B".into()),
            ("c".into(), "C".into()),
            ("d".into(), "D".into()),
        ];
        let g = lg(&[("a", "b"), ("c", "d")]);
        let d = layout_graph(&meta, &g, LayoutParams::default());
        for i in 0..d.nodes.len() {
            for j in (i + 1)..d.nodes.len() {
                let (p, q) = (&d.nodes[i], &d.nodes[j]);
                assert!(
                    (p.x - q.x).abs() > 0.01 || (p.y - q.y).abs() > 0.01,
                    "coincident nodes"
                );
            }
        }
    }

    /// Manual byte scan: any `.` followed by 3+ ASCII digits (over-precise coord).
    fn has_three_decimals(j: &str) -> bool {
        let b = j.as_bytes();
        for i in 0..b.len() {
            if b[i] == b'.' {
                let mut k = i + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k - (i + 1) >= 3 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn coordinates_are_rounded_for_stable_json() {
        let meta: Vec<_> = (0..5).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
        let j = graph_data_json(&layout_graph(
            &meta,
            &lg(&[("n0", "n1")]),
            LayoutParams::default(),
        ));
        assert!(!has_three_decimals(&j), "json has over-precise coords: {j}");
    }
}
