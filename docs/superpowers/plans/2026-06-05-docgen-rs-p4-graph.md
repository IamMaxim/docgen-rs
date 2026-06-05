# docgen-rs P4 — Graph view (Rust force layout + SVG island + `/graph/` page)

**Date:** 2026-06-05
**Phase:** P4 (graph view)
**Branch:** `overnight/p1-p6` (local only — never push/PR)
**Status:** Plan approved, not yet implemented
**Spec:** `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md`
(sections: *Islands* → `HomeDocGraph` row, *Pipeline / data flow* step 3, *Decisions* → Asset embedding)
**Original behaviour reference:** `~/work/docgen/packages/docgen/src/lib/components/HomeDocGraph.svelte`
and its layout core `~/work/docgen/packages/docgen/src/lib/graph.ts`.

---

## 0. Scope, decisions, and what P4 inherits

### 0.1 Goal restated

Add a doc-graph view to the generated site. Two clusters, landed strictly in order:

- **Cluster A — Rust layout + `GraphData` + JSON (pure, deterministic).** A new
  `docgen-core::graphlayout` module that consumes the *already-built*
  `LinkGraph` (P1's wikilink edges + the per-doc title/slug metadata) and runs a
  **deterministic** force-directed spring layout in Rust to produce 2D node
  positions. It emits a serializable `GraphData { nodes:[{slug,title,x,y,degree}],
  edges:[{from,to}] }` plus a `graph_data_json(&GraphData) -> String` serializer.
  Fully unit-tested: determinism (byte-identical across two runs), bounds, degree,
  edge filtering, and the empty / single-node / disconnected / self-loop cases.

- **Cluster B — graph island JS + `/graph/` page + build wiring + e2e.** A new
  Alpine island (`docgenGraph`, registered through the `window.docgen.island`
  convention from P3's `bootstrap.js`, **no d3, no graph lib**) that reads the
  embedded `GraphData` JSON from a `<script type="application/json">` tag and
  renders nodes + edges as SVG with hover-highlight, click-to-navigate, and
  pan/zoom. A new `graph.html` template + `GraphContext` in `docgen-render`
  renders the `/graph/` page. `crates/docgen/src/build.rs` computes the layout,
  emits `dist/graph/index.html`, gates + emits the graph island asset, and the
  page template grows a nav link to `/graph/`. Pure-Rust tests assert the page
  markup, the embedded JSON, asset emission, and the nav link. Actual SVG
  interactivity (hover/click/pan/zoom) is validated in-browser by the architect.

No npm, no node, no bundler, no WASM, no vendored JS graph lib. Layout is **pure
Rust at build time**; the island is small hand-written classic JS.

### 0.2 FLAGGED DECISION — layout algorithm: **port the original `graph.ts` spring model to Rust, minus the site-specific anchoring. Confirmed.**

The original `buildGraph` (graph.ts) is a fixed-iteration spring layout:
per-iteration **repulsion** (all pairs, `min(3.8, 2800/distSq)`), **attraction**
along edges (target length 132, factor 0.018), and a pull toward a per-node
**anchor** seeded from `section` columns + a `sin` wave. It is already
**deterministic** (no RNG, no clock) — its only nondeterminism risk is `Map`/array
iteration order, which we eliminate in Rust by iterating index-ordered `Vec`s.

We port the **repulsion + attraction + clamping** faithfully (same constants), but
replace the original's *section-column* anchoring (docgen-rs has no `section`
frontmatter concept; the original hard-coded `['Getting started','Overview','Game
design','Developers']`) with a **deterministic seeded initial placement**: a
golden-angle spiral keyed by node index. This keeps the layout stable, seed-free
of any system RNG, and independent of doc taxonomy we don't have. The anchor pull
is kept (toward each node's *initial* spiral point) so disconnected components
don't drift to the clamp walls — same role the section anchor played originally.

> **Determinism contract (the testable core):** given the same `GraphData` inputs
> (same node order, same edges), `layout_graph` produces **byte-identical** output
> across runs and machines. Sources of nondeterminism are banned: no
> `rand`/`thread_rng`, no `SystemTime`, no `HashMap` iteration in the hot loop
> (use index-ordered `Vec`s + `BTreeMap` only for slug→index lookups built once).
> All float math is plain `f64`; iteration count is a fixed constant.

**Why not a crate (e.g. `forceatlas2`, `fdg`)?** Unnecessary dependency + harder
determinism guarantees across versions. The algorithm is ~40 lines; we own it and
test it. Rejected.

**Why anchor to the spiral, not section columns?** docgen-rs docs have `slug`,
`title`, `rel_path` — no `section`. Inventing one is out of P4 scope. The spiral
gives a stable, well-spread seed; the spring forces do the real layout.

### 0.3 FLAGGED DECISION — `/graph/` page is **default-on**, no config toggle in P4.

The spec lists HomeDocGraph as a parity feature with no toggle of its own; P6 owns
`docgen.toml` feature flags. P4 emits `/graph/` unconditionally (like history pages
emit when git history exists). The graph island ships **only on the `/graph/`
page** (gated exactly like the mermaid island is gated per-page via a
`PageContext`/`GraphContext` bool + an `EmitOptions.include_graph` flag). A
`docgen.toml` `[features] graph = false` toggle is explicitly **deferred to P6**;
P4 leaves the wiring (a single bool through `build.rs`) so P6 flips one value.

### 0.4 What P4 inherits and must not break

- **`docgen-core::graph::LinkGraph { edges: Vec<LinkEdge{from,to}>, backlinks:
  BTreeMap<String,Vec<Backlink>> }`** is already built per-site by
  `pipeline::render_docs` from wikilinks (P1). **P4 CONSUMES this graph; it does
  NOT recompute links.** Self-edges are already dropped by `build_link_graph`
  (`graph.rs`), so P4's self-loop handling is a defensive belt-and-braces filter,
  not a re-derivation.
- The per-doc `(slug, title)` metadata is available as `site.docs[i].{slug,title}`
  from `SiteBuild`. `layout_graph` takes node metadata + the `LinkGraph` and never
  reaches back into markdown.
- **Island convention (P3, `docgen-assets`):** `window.docgen.island(name, fn)`
  pushes a registrar; `bootstrap.js` runs all registrars on `alpine:init`, then
  Alpine starts once. `window.docgen.loadScript(src)` lazy-loads (unused by graph —
  no lazy lib). The graph island is a **classic script, NO ESM `import`** (the
  no-npm discipline), registered the same way `islands/mermaid.js` is.
- **Asset planner (P3, `docgen-assets`):** `EmitOptions { include_katex_runtime,
  include_mermaid }` + `assets_for(&EmitOptions) -> Vec<Asset>` + `emit`. P4 adds
  `include_graph: bool` and a `graph_assets()` slice, mirroring `mermaid_assets()`.
- **`crates/docgen/src/build.rs`** is the `build` subcommand (there is **no Cargo
  `build.rs` script** — the prompt's "build.rs" means this file). It already:
  emits per-doc pages + history pages + `search-index.json`, then calls
  `docgen_assets::emit(&assets_for(&opts), &dist_dir)`. P4 hooks the `/graph/` page
  + the graph asset emission here.
- **Page template** `crates/docgen-render/templates/page.html` gates the mermaid
  island via `{% if has_mermaid %}`. The graph page reuses the same nav/sidebar
  shell. P4 adds a **nav link to `/graph/`** in this template (shows on every page).
- **`docgen-render::Renderer`** owns a minijinja env with `page.html` + `history.html`
  registered under `.html` names (auto-escape on). P4 registers `graph.html` the same
  way and adds `render_graph(&GraphContext)`.
- **Test conventions:** temp dirs keyed by `std::process::id()`,
  `CARGO_BIN_EXE_docgen`, fixtures copied from `fixtures/`. Match exactly.
- **`serde` is already a dep** of `docgen-core` and `docgen-render`; `serde_json`
  is a dep of `docgen-core` (used by `search::index_json`). No new crates needed
  for Cluster A. (`serde_json` is a *dev*-dep of the `docgen` bin — fine, the bin
  serializes via `docgen-core`.)

### 0.5 Public API kept consistent across clusters (define once in A, reused in B)

```rust
// docgen-core::graphlayout — the contract Cluster B depends on.

/// One laid-out graph node: a doc plus its 2D position and link degree.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GraphNode {
    pub slug: String,
    pub title: String,
    pub x: f64,
    pub y: f64,
    pub degree: u32,
}

/// One graph edge (undirected for display; carries the directed slugs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct GraphDataEdge {
    pub from: String,
    pub to: String,
}

/// The serializable layout result embedded in /graph/ and consumed by the island.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphDataEdge>,
}

/// Fixed layout parameters. `LayoutParams::default()` is the canonical, tested seed.
#[derive(Debug, Clone, Copy)]
pub struct LayoutParams {
    pub width: f64,        // 1420.0  (matches original viewBox)
    pub height: f64,       // 760.0
    pub padding: f64,      // 74.0
    pub iterations: usize, // 180     (fixed; determinism)
}

/// Deterministic force-directed layout. `nodes_meta`: (slug, title) in a STABLE
/// order (doc input order). `graph`: the already-built LinkGraph. Returns GraphData
/// with positions clamped inside [padding, width-padding] × [padding, height-padding].
pub fn layout_graph(
    nodes_meta: &[(String, String)],
    graph: &docgen_core::graph::LinkGraph,
    params: LayoutParams,
) -> GraphData;

/// Serialize GraphData to compact JSON (stable key order via serde derive).
pub fn graph_data_json(data: &GraphData) -> String;
```

```rust
// docgen-assets — one new flag + one new slice, mirroring mermaid.
pub struct EmitOptions {
    pub include_katex_runtime: bool,
    pub include_mermaid: bool,
    pub include_graph: bool,   // NEW
}
pub fn graph_assets() -> Vec<Asset>;   // NEW: islands/graph.js (+ css already in docgen.css)

// docgen-render — one new context + render method, mirroring history.
pub struct GraphContext<'a> {
    pub tree: &'a [docgen_core::model::TreeNode],
    pub graph_json: &'a str,   // the embedded GraphData JSON
    pub node_count: usize,
    pub edge_count: usize,
}
impl Renderer { pub fn render_graph(&self, ctx: &GraphContext) -> Result<String, minijinja::Error>; }
```

**Island name:** `docgenGraph` (Alpine `x-data="docgenGraph"`), asset
`islands/graph.js`, dist path `/islands/graph.js`. Mirrors `docgenMermaid`.

---

## 1. Working agreement (per logical unit)

For **every** unit below:

1. **Write the test(s) first** (TDD). Run, watch them fail for the right reason.
2. Implement the minimum to pass.
3. `cargo test` (workspace) green + `cargo clippy --all-targets` clean (no new warnings).
4. Commit with exactly:
   ```
   git -c commit.gpgsign=false -c user.name="docgen-rs overnight" \
       -c user.email="g.maxim.stepanoff@gmail.com" commit -m "<msg>"
   ```
5. STAY on `overnight/p1-p6`. Never push, PR, branch, or touch a remote.

Commands used throughout (absolute-path safe; run from repo root
`/Users/maxim/work/docgen-rs`):

```
cargo test -p docgen-core graphlayout::      # Cluster A unit tests
cargo test -p docgen-assets                  # asset slice tests
cargo test -p docgen-render graph            # render tests
cargo test -p docgen --test build_cli        # e2e
cargo test                                   # full workspace
cargo clippy --all-targets
```

---

## CLUSTER A — Rust layout + `GraphData` + JSON (pure, deterministic)

New file: `crates/docgen-core/src/graphlayout.rs`. Register in `lib.rs`
(`pub mod graphlayout;` + re-export the public types). No new dependencies.

### A-1 — module skeleton + public types + `lib.rs` wiring

**Test (compile-only contract):** add to `graphlayout.rs` `#[cfg(test)]`:

```rust
#[test]
fn graphdata_serializes_with_stable_keys() {
    let d = GraphData {
        nodes: vec![GraphNode { slug: "a".into(), title: "A".into(), x: 1.0, y: 2.0, degree: 3 }],
        edges: vec![GraphDataEdge { from: "a".into(), to: "b".into() }],
    };
    let j = graph_data_json(&d);
    assert!(j.contains(r#""slug":"a""#));
    assert!(j.contains(r#""x":1.0"#));
    assert!(j.contains(r#""degree":3"#));
    assert!(j.contains(r#""from":"a""#));
    assert!(j.contains(r#""to":"b""#));
    // Compact (no pretty whitespace), parses back.
    assert!(!j.contains(",\n"));
    let _v: serde_json::Value = serde_json::from_str(&j).unwrap();
}
```

**Implement:** the four public types from §0.5 + `LayoutParams::default()`
(`width:1420.0, height:760.0, padding:74.0, iterations:180`) + a stub
`layout_graph` that returns `GraphData { nodes: vec![], edges: vec![] }` for now
(A-2 fills it) + `graph_data_json` = `serde_json::to_string(data).unwrap()`.

Add to `lib.rs`:
```rust
pub mod graphlayout;
pub use graphlayout::{graph_data_json, layout_graph, GraphData, GraphDataEdge, GraphNode, LayoutParams};
```

`cargo test -p docgen-core graphlayout::graphdata_serializes` → green.

**Commit:** `feat(core): GraphData types + JSON serializer for graph layout (P4 A-1)`

### A-2 — degree computation from `LinkGraph.edges` (undirected, self-loops excluded)

**Tests:**

```rust
use docgen_core::graph::LinkGraph;          // (within module: use crate::graph::LinkGraph;)
use crate::model::LinkEdge;

fn lg(edges: &[(&str, &str)]) -> LinkGraph {
    LinkGraph {
        edges: edges.iter().map(|(f, t)| LinkEdge { from: f.to_string(), to: t.to_string() }).collect(),
        backlinks: Default::default(),
    }
}

#[test]
fn degree_counts_each_incident_edge_undirected() {
    // a->b, a->c, b->a : node a touches 3 edges; b touches 2; c touches 1.
    let meta = vec![("a".into(),"A".into()), ("b".into(),"B".into()), ("c".into(),"C".into())];
    let g = lg(&[("a","b"), ("a","c"), ("b","a")]);
    let d = layout_graph(&meta, &g, LayoutParams::default());
    let deg = |s: &str| d.nodes.iter().find(|n| n.slug == s).unwrap().degree;
    assert_eq!(deg("a"), 3);
    assert_eq!(deg("b"), 2);
    assert_eq!(deg("c"), 1);
}

#[test]
fn self_loops_are_ignored_for_degree_and_edges() {
    let meta = vec![("a".into(),"A".into()), ("b".into(),"B".into())];
    // Defensive: even if a self-edge leaks in, it must not count or render.
    let g = lg(&[("a","a"), ("a","b")]);
    let d = layout_graph(&meta, &g, LayoutParams::default());
    assert_eq!(d.nodes.iter().find(|n| n.slug == "a").unwrap().degree, 1);
    assert!(!d.edges.iter().any(|e| e.from == e.to));
    assert_eq!(d.edges.len(), 1);
}

#[test]
fn edges_to_unknown_slugs_are_dropped() {
    // An edge to a slug not in nodes_meta (ghost) can't be drawn — drop it.
    let meta = vec![("a".into(),"A".into())];
    let g = lg(&[("a","ghost")]);
    let d = layout_graph(&meta, &g, LayoutParams::default());
    assert!(d.edges.is_empty());
    assert_eq!(d.nodes.iter().find(|n| n.slug == "a").unwrap().degree, 0);
}
```

**Implement** (inside `layout_graph`, before the force loop):
- Build `index_of: BTreeMap<&str, usize>` from `nodes_meta` (stable).
- Build the **filtered edge list** `edges: Vec<(usize, usize)>`: for each
  `LinkEdge`, look up both endpoints in `index_of`; skip if either missing or if
  `from == to`. Dedup undirected pairs? **No** — keep parity with the original
  which drew every edge; but degree must count each incident filtered edge. (The
  original counted source+target once per edge.) Store `edges` index-pairs.
- `degree[i]` = count of filtered edge-pairs incident to node `i` (increment both
  endpoints per pair).
- Build `GraphData.edges` as `GraphDataEdge { from, to }` in **filtered LinkGraph
  order** (already sorted by `build_link_graph`), slugs cloned from `nodes_meta`.
- (Positions still stubbed to the seed in A-3; degree + edges correct now. To make
  these tests pass standalone you may seed positions to `(0,0)` temporarily — A-3
  replaces with the real seed + force loop. Keep `x`/`y` out of these asserts.)

`cargo test -p docgen-core graphlayout::` → green.

**Commit:** `feat(core): graph layout degree + edge filtering (drop self/ghost) (P4 A-2)`

### A-3 — deterministic seed placement (golden-angle spiral)

**Tests:**

```rust
#[test]
fn seed_positions_are_deterministic_and_in_bounds() {
    let meta: Vec<_> = (0..7).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
    let g = lg(&[]); // no edges → layout == seed (after force loop is a no-op-ish)
    let p = LayoutParams::default();
    let d1 = layout_graph(&meta, &g, p);
    let d2 = layout_graph(&meta, &g, p);
    assert_eq!(d1, d2); // byte/value-identical
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
    // JSON of an empty graph is still valid + parseable.
    let _v: serde_json::Value = serde_json::from_str(&graph_data_json(&d)).unwrap();
}
```

**Implement** `seed_point(i, total, params) -> (f64, f64)`:
- Golden angle `GA = PI * (3.0 - 5.0_f64.sqrt())` (≈2.39996). For node `i`:
  `angle = i as f64 * GA`, `radius_frac = ((i as f64 + 0.5) / total.max(1) as f64).sqrt()`.
  `cx = width/2`, `cy = height/2`, `rmax = (min(width,height)/2 - padding)`.
  `x = cx + radius_frac * rmax * angle.cos()`, `y = cy + radius_frac * rmax * angle.sin()`.
- Clamp into `[padding, width-padding] × [padding, height-padding]` with a private
  `clamp(v,min,max)` helper (port of the original `clamp`).
- No system RNG, no time — purely a function of `i`, `total`, `params`. Deterministic.
- For `total == 0` return empty. For `total == 1`, the formula gives the centre-ish.

Force loop currently a no-op or absent → seed *is* the output, so these tests pin
the seed. A-4 adds the forces (which must keep determinism + bounds).

`cargo test -p docgen-core graphlayout::` → green.

**Commit:** `feat(core): deterministic golden-angle seed placement for graph (P4 A-3)`

### A-4 — force iterations (repulsion + attraction + anchor pull + clamp)

Port the original spring model with fixed iteration count. Positions live in two
parallel `Vec<f64>` (`xs`, `ys`) indexed by node; `seeds: Vec<(f64,f64)>` kept for
the anchor pull.

**Tests:**

```rust
#[test]
fn connected_pair_settles_near_target_edge_length() {
    // Two linked nodes should relax toward the spring's target distance (~132),
    // well separated, both in-bounds, finite, deterministic.
    let meta = vec![("a".into(),"A".into()), ("b".into(),"B".into())];
    let g = lg(&[("a","b")]);
    let p = LayoutParams::default();
    let d1 = layout_graph(&meta, &g, p);
    let d2 = layout_graph(&meta, &g, p);
    assert_eq!(d1, d2); // determinism survives the force loop
    let a = &d1.nodes[0]; let b = &d1.nodes[1];
    let dist = ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
    assert!(dist > 40.0 && dist < 320.0, "pair distance {dist} not in plausible band");
    for n in &d1.nodes { assert!(n.x.is_finite() && n.y.is_finite()); }
}

#[test]
fn all_positions_stay_in_bounds_after_forces() {
    let meta: Vec<_> = (0..20).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
    // a hub + spokes + a couple of disconnected nodes
    let mut e: Vec<(&str,&str)> = Vec::new();
    let owned: Vec<(String,String)> = (1..15).map(|i| ("n0".into(), format!("n{i}"))).collect();
    for (f,t) in &owned { e.push((f, t)); }
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
    // Two separate pairs: {a-b} and {c-d}. Anchor pull keeps them spread.
    let meta = vec![("a".into(),"A".into()),("b".into(),"B".into()),
                    ("c".into(),"C".into()),("d".into(),"D".into())];
    let g = lg(&[("a","b"),("c","d")]);
    let d = layout_graph(&meta, &g, LayoutParams::default());
    // No two nodes share an identical position.
    for i in 0..d.nodes.len() {
        for j in (i+1)..d.nodes.len() {
            let (p,q) = (&d.nodes[i], &d.nodes[j]);
            assert!((p.x - q.x).abs() > 0.01 || (p.y - q.y).abs() > 0.01, "coincident nodes");
        }
    }
}
```

**Implement** the loop (faithful port; constants from graph.ts):

```rust
for _ in 0..params.iterations {
    // 1) repulsion: every unordered pair
    for a in 0..n {
        for b in (a+1)..n {
            let dx = (xs[b] - xs[a]).abs().max(0.01).copysign(xs[b] - xs[a]); // dx || 0.01
            // (in practice: let mut dx = xs[b]-xs[a]; if dx==0.0 {dx=0.01;} same for dy)
            let dy = /* same guard */;
            let dist_sq = dx*dx + dy*dy;
            let dist = dist_sq.sqrt();
            let strength = (2800.0 / dist_sq).min(3.8);
            let (px, py) = (dx/dist*strength, dy/dist*strength);
            xs[a] -= px; ys[a] -= py; xs[b] += px; ys[b] += py;
        }
    }
    // 2) attraction along edges (target 132, factor 0.018)
    for &(s, t) in &edges {
        let dx = xs[t] - xs[s]; let dy = ys[t] - ys[s];
        let dist = (dx*dx + dy*dy).sqrt().max(1.0);
        let pull = (dist - 132.0) * 0.018;
        let (px, py) = (dx/dist*pull, dy/dist*pull);
        xs[s] += px; ys[s] += py; xs[t] -= px; ys[t] -= py;
    }
    // 3) anchor pull toward seed + clamp (original used 0.012 / 0.006)
    for i in 0..n {
        xs[i] += (seeds[i].0 - xs[i]) * 0.012;
        ys[i] += (seeds[i].1 - ys[i]) * 0.006;
        xs[i] = clamp(xs[i], params.padding, params.width - params.padding);
        ys[i] = clamp(ys[i], params.padding, params.height - params.padding);
    }
}
```

Notes:
- Iterate `edges` (the `Vec<(usize,usize)>` from A-2) in **index order** — never a
  `HashMap` — to keep determinism.
- The `dx || 0.01` guard: in Rust write `let mut dx = xs[b]-xs[a]; if dx == 0.0 { dx = 0.01; }`.
- After the loop, round positions to a fixed precision before writing into
  `GraphNode` to make JSON byte-identical across platforms and keep the file small:
  store `x = (xs[i] * 100.0).round() / 100.0` (2 decimals). **Add a test that the
  serialized JSON has no coordinate with > 2 decimal places** (regex
  `\.\d{3,}` absent) — pins file stability.

```rust
#[test]
fn coordinates_are_rounded_for_stable_json() {
    let meta: Vec<_> = (0..5).map(|i| (format!("n{i}"), format!("N{i}"))).collect();
    let j = graph_data_json(&layout_graph(&meta, &lg(&[("n0","n1")]), LayoutParams::default()));
    // No coordinate carries more than 2 decimal places.
    assert!(!regex_like_three_decimals(&j), "json has over-precise coords: {j}");
}
// helper: a tiny manual scan for `.` followed by 3+ digits in a number context,
// or just assert the float fields round-trip to themselves at 2dp. Prefer a
// no-dep manual scan over adding `regex`.
```

(Implement `regex_like_three_decimals` as a manual byte scan — **do not add the
`regex` crate**.)

`cargo test -p docgen-core graphlayout::` → all green. `cargo clippy --all-targets`.

**Commit:** `feat(core): deterministic force-directed graph layout (repulsion/attraction/anchor) (P4 A-4)`

### A-5 — `SiteBuild` convenience: build `GraphData` from the assembled site

So `build.rs` doesn't re-derive `(slug,title)` meta. Add a thin helper.

**Test** (in `pipeline.rs` tests, reusing `render_docs`):

```rust
#[test]
fn site_graph_data_matches_docs_and_links() {
    let prepared = vec![
        prepare(raw("index.md", "# Home\nGo to [[guide/intro]].\n")),
        prepare(raw("guide/intro.md", "# Intro\nBack to [[index]].\n")),
    ];
    let site = render_docs(prepared);
    let gd = site.graph_data(docgen_core::graphlayout::LayoutParams::default());
    // one node per doc, slugs + titles carried through
    assert_eq!(gd.nodes.len(), 2);
    assert!(gd.nodes.iter().any(|n| n.slug == "index" && n.title == "Home"));
    assert!(gd.nodes.iter().any(|n| n.slug == "guide/intro" && n.title == "Intro"));
    // edges mirror the link graph (index<->intro), no self/ghost
    assert!(gd.edges.iter().any(|e| e.from == "index" && e.to == "guide/intro"));
    assert!(gd.edges.iter().any(|e| e.from == "guide/intro" && e.to == "index"));
}
```

**Implement** on `SiteBuild` (in `pipeline.rs`):

```rust
impl SiteBuild {
    /// Build the deterministic GraphData for the /graph/ page from this site's
    /// docs (node order = doc order) and its already-built LinkGraph.
    pub fn graph_data(&self, params: crate::graphlayout::LayoutParams) -> crate::graphlayout::GraphData {
        let meta: Vec<(String, String)> =
            self.docs.iter().map(|d| (d.slug.clone(), d.title.clone())).collect();
        crate::graphlayout::layout_graph(&meta, &self.graph, params)
    }
}
```

`cargo test -p docgen-core` → green. `cargo clippy --all-targets`.

**Commit:** `feat(core): SiteBuild::graph_data builds layout from docs + link graph (P4 A-5)`

**Cluster A done-gate:** `cargo test -p docgen-core` green, `cargo clippy
--all-targets` clean, all of A-1..A-5 committed. Public API in §0.5 is final and
will not change in Cluster B.

---

## CLUSTER B — graph island JS + `/graph/` page + build wiring + e2e

### B-1 — graph island asset (`docgen-assets/assets/docgen/islands/graph.js`)

Authored classic JS, registered via the island convention, **no ESM, no d3**.
Reads `GraphData` JSON from `<script type="application/json" id="docgen-graph-data">`,
renders SVG, wires hover/click/pan/zoom.

**Test** (in `docgen-assets/src/lib.rs` `#[cfg(test)]`, mirrors the mermaid contract test):

```rust
#[test]
fn graph_island_registers_and_renders_without_esm_or_d3() {
    let js = std::str::from_utf8(
        ASSETS.get_file("docgen/islands/graph.js").unwrap().contents()
    ).unwrap();
    assert!(js.contains("docgen.island"));
    assert!(js.contains("docgenGraph"));
    assert!(js.contains("docgen-graph-data"));         // reads the embedded JSON
    assert!(js.contains("http://www.w3.org/2000/svg")); // builds SVG via createElementNS
    assert!(!js.contains("import "));                   // no ESM / npm
    assert!(!js.to_lowercase().contains("d3"));         // no vendored graph lib
}
```

**Implement** `islands/graph.js` (hand-written; key behaviours — the architect will
verify the actual interactivity in-browser):

```js
// Doc-graph island: renders an SVG force-graph from build-time GraphData JSON.
// No d3, no ESM — classic script, registered via the docgen.island convention.
window.docgen.island('docgenGraph', function (Alpine) {
  Alpine.data('docgenGraph', function () {
    return {
      pan: { x: 0, y: 0 },
      scale: 1,
      hovered: null,
      data: { nodes: [], edges: [] },
      adj: {},           // slug -> Set of neighbour slugs (for hover highlight)
      init() {
        const tag = document.getElementById('docgen-graph-data');
        if (!tag) return;
        try { this.data = JSON.parse(tag.textContent); } catch (e) { return; }
        this.buildAdjacency();
        this.draw();
        this.wirePanZoom();
      },
      buildAdjacency() { /* fill this.adj from this.data.edges (both directions) */ },
      draw() {
        // createElementNS('http://www.w3.org/2000/svg', ...) for <g>/<line>/<a>/<circle>.
        // r = clamp(4.5 + sqrt(degree)*1.2, 5, 12)  (port of original radius).
        // node <a href="/{slug}"> so click navigates; circle inside.
        // store node refs for hover highlight.
      },
      hover(slug) { /* set this.hovered; toggle .active/.dimmed classes on nodes+edges */ },
      clearHover() { this.hovered = null; /* reset classes */ },
      wirePanZoom() {
        // pointerdown/move/up → translate this.pan (clamped); wheel → zoom this.scale
        // (clamped ~0.4..2.5) about cursor; apply via transform on the root <g>.
      },
    };
  });
});
```

Implementation rules:
- Build SVG nodes with `document.createElementNS('http://www.w3.org/2000/svg', tag)`.
- **No innerHTML for SVG** (namespace correctness) — use `createElementNS`.
- Node radius port: `clamp(4.5 + Math.sqrt(degree)*1.2, 5, 12)`.
- Hover: on `pointerenter` of a node, add `active` to it + its adjacency, `dimmed`
  to the rest, and `active` to incident `<line>`s; clear on `pointerleave`.
- Click: nodes are `<a href="/{slug}">` so a plain click navigates (matches the
  original's `<a href={base+path}>`). Pan-drag must not trigger navigation — start
  pan only when the pointer target is the frame, not an `<a>` (port the original's
  `closest('a')` guard).
- Pan/zoom apply as a single `transform="translate(px,py) scale(s)"` on the root
  `<g>`. Clamp pan + scale.
- Classic IIFE-free style is fine (mermaid island is bare `window.docgen.island(...)`).
- Match the existing 2-space-indent, no-semicolon-free style of `mermaid.js`.

`cargo test -p docgen-assets graph_island` → green.

**Commit:** `feat(assets): docgenGraph SVG island (hover/click/pan/zoom, no d3) (P4 B-1)`

### B-2 — `graph_assets()` slice + `EmitOptions.include_graph` planner gate

**Tests** (in `docgen-assets/src/lib.rs`, mirroring the mermaid slice tests):

```rust
#[test]
fn graph_slice_has_island_and_is_gated() {
    let g = graph_assets();
    assert!(g.iter().any(|a| a.path == "islands/graph.js"));
    for a in &g { assert!(!a.bytes.is_empty(), "{} empty", a.path); }

    // off by default
    assert!(!assets_for(&EmitOptions::default()).iter().any(|a| a.path == "islands/graph.js"));
    // on when flag set
    let full = assets_for(&EmitOptions { include_graph: true, ..Default::default() });
    assert!(full.iter().any(|a| a.path == "islands/graph.js"));
}

#[test]
fn graph_island_is_js_kinded() {
    assert_eq!(graph_assets().iter().find(|a| a.path == "islands/graph.js").unwrap().kind, AssetKind::Js);
}
```

Update the existing `planner_gates_mermaid_and_katex_runtime` style default-off
assertions to also cover graph if convenient (optional).

**Implement:**
- Add `pub include_graph: bool` to `EmitOptions` (derive `Default` keeps it `false`).
- ```rust
  /// Graph island JS. Emitted only on builds that render the /graph/ page
  /// (gated by [`EmitOptions::include_graph`]). CSS lives in the shared
  /// docgen.css (see B-3); this slice is JS-only. No vendored graph lib.
  pub fn graph_assets() -> Vec<Asset> {
      vec![embed("docgen/islands/graph.js", "islands/graph.js", AssetKind::Js)]
  }
  ```
- In `assets_for`: `if opts.include_graph { out.extend(graph_assets()); }`.

`cargo test -p docgen-assets` → green.

**Commit:** `feat(assets): graph_assets slice + include_graph planner gate (P4 B-2)`

### B-3 — graph CSS in the shared `docgen.css`

Append graph styles (ported/condensed from `HomeDocGraph.svelte`'s `<style>`, but
using the existing docgen.css variable conventions) so nodes/edges/hover/frame look
right. Classes: `.docgen-graph` (frame), `.docgen-graph svg`, `.docgen-graph__links
line`, `.docgen-graph__links line.active`, `.docgen-graph__nodes circle`,
`.docgen-graph__nodes circle.active`, `.docgen-graph__nodes circle.dimmed`,
`.docgen-graph__meta` (the "N nodes · M links" caption), `.docgen-graph__frame`
cursor grab/grabbing.

**Test** (extend the existing css-content tests in `docgen-assets`):

```rust
#[test]
fn shared_css_has_graph_styles() {
    let s = std::str::from_utf8(
        core_assets().iter().find(|a| a.path == "docgen.css").unwrap().bytes
    ).unwrap();
    assert!(s.contains(".docgen-graph"));
    assert!(s.contains(".docgen-graph__nodes circle"));
    assert!(s.contains(".docgen-graph__links line"));
}
```

**Implement:** append the rules to
`crates/docgen-assets/assets/docgen/docgen.css`. Keep it dependency-free, reuse
existing CSS custom properties if the file defines any (check the file; if it has
no vars, use literal colours consistent with the existing palette).

`cargo test -p docgen-assets` → green.

**Commit:** `feat(assets): graph view styles in shared docgen.css (P4 B-3)`

### B-4 — `graph.html` template + `GraphContext` + `render_graph` in `docgen-render`

New template `crates/docgen-render/templates/graph.html`. Reuses the page shell
(sidebar macro + nav). Embeds the JSON in a `<script type="application/json"
id="docgen-graph-data">` and mounts the island on a frame `<div x-data="docgenGraph"
x-init="init()">`. Loads `/islands/graph.js` + bootstrap + Alpine (same order as
`page.html`).

**Tests** (new `graph`-tagged tests in `docgen-render/src/lib.rs`):

```rust
#[test]
fn renders_graph_page_with_embedded_json_and_island() {
    let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
    let json = r#"{"nodes":[{"slug":"a","title":"A","x":1.0,"y":2.0,"degree":0}],"edges":[]}"#;
    let html = r.render_graph(&GraphContext {
        tree: &[],
        graph_json: json,
        node_count: 1,
        edge_count: 0,
    }).unwrap();
    assert!(html.contains("<title>Graph</title>"));
    assert!(html.contains(r#"id="docgen-graph-data""#));
    assert!(html.contains(r#"type="application/json""#));
    assert!(html.contains(json));                       // JSON embedded verbatim, NOT escaped
    assert!(html.contains(r#"x-data="docgenGraph""#));
    assert!(html.contains(r#"src="/islands/graph.js""#));
    assert!(html.contains(r#"src="/bootstrap.js""#));
    assert!(html.contains(r#"src="/vendor/alpine/alpine.min.js""#));
    assert!(html.contains("1 nodes")); // meta caption
}

#[test]
fn graph_page_renders_sidebar_tree() {
    let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
    let tree = vec![docgen_core::model::TreeNode::Doc {
        name: "intro".into(), slug: "guide/intro".into(), title: "Intro".into(),
    }];
    let html = r.render_graph(&GraphContext {
        tree: &tree, graph_json: r#"{"nodes":[],"edges":[]}"#, node_count: 0, edge_count: 0,
    }).unwrap();
    assert!(html.contains(r#"href="/guide/intro""#));
}
```

**Critical:** the embedded JSON must be emitted **raw** (`| safe`), not
HTML-escaped, so the island's `JSON.parse` works. minijinja auto-escapes by
default under the `.html` name; mark `{{ graph_json | safe }}`. Because the JSON
goes inside `<script type="application/json">`, the only XSS vector is a literal
`</script>` substring in a title — guard by ensuring `graph_data_json` produces
JSON where `/` in `</script>` cannot appear unescaped. **Mitigation (do this in
A-1 serializer or B-4 template):** after serialization, replace `</` with `<\/`
in the embedded string (valid JSON, neutralises `</script>`). Add a test:

```rust
#[test]
fn embedded_json_neutralizes_script_close() {
    // A title containing </script> must not break out of the JSON <script> tag.
    let r = Renderer::new(DEFAULT_PAGE_TEMPLATE).unwrap();
    let json = r#"{"nodes":[{"slug":"x","title":"a</script>b","x":0.0,"y":0.0,"degree":0}],"edges":[]}"#;
    let html = r.render_graph(&GraphContext { tree: &[], graph_json: json, node_count: 1, edge_count: 0 }).unwrap();
    assert!(!html.contains("a</script>b"));       // raw close-tag must not survive
    assert!(html.contains(r#"a<\/script>b"#));    // escaped form present
}
```

Decision: **do the `</` → `<\/` escaping in the template via a minijinja filter or
in `graph_data_json`?** Put it in `graph_data_json` is wrong (it'd corrupt the JSON
for other consumers). Put the escaping in `render_graph` (replace on the
`graph_json` string before injecting). Keep `graph_data_json` pure JSON.

**Implement:**
- `GraphContext` struct (§0.5) in `docgen-render/src/lib.rs`.
- `pub const DEFAULT_GRAPH_TEMPLATE: &str = include_str!("../templates/graph.html");`
  and register it in `Renderer::new` (`env.add_template_owned("graph.html", ...)`).
- `render_graph`: escape `graph_json`'s `</` → `<\/`, then
  `tmpl.render(context!{ tree, graph_json => escaped, node_count, edge_count, search_enabled => true })`.
- `graph.html`: `<title>Graph</title>`, the sidebar macro (copy from page.html),
  a `<main>` with the meta caption `{{ node_count }} nodes · {{ edge_count }} links`,
  the frame `<div class="docgen-graph" x-data="docgenGraph" x-init="init()"><svg
  class="docgen-graph__svg" ...></svg></div>`, the JSON script tag `<script
  type="application/json" id="docgen-graph-data">{{ graph_json | safe }}</script>`,
  then `<script src="/bootstrap.js"></script><script src="/islands/graph.js"></script>
  <script src="/vendor/alpine/alpine.min.js" defer></script>`.

`cargo test -p docgen-render graph` → green. `cargo clippy --all-targets`.

**Commit:** `feat(render): /graph/ page template + GraphContext + render_graph (P4 B-4)`

### B-5 — nav link to `/graph/` in the page template (and history shell if shared)

**Tests** (extend `docgen-render` page tests):

```rust
#[test]
fn page_has_graph_nav_link() {
    let html = renderer().render_page(&PageContext {
        title: "X", slug: "x", body_html: "", tree: &[], backlinks: &[],
        has_history: false, has_mermaid: false, has_math: false,
    }).unwrap();
    assert!(html.contains(r#"href="/graph""#));
}
```

Also assert the `/graph/` page itself shows the same nav link (self-link is fine).

**Implement:** add a small nav element to `page.html` (and `graph.html`) — e.g. in
the sidebar header or a top nav: `<a class="docgen-nav-graph" href="/graph">Graph</a>`.
Keep it on every page so the graph is always reachable (parity with the original
home graph being a primary surface).

`cargo test -p docgen-render` → green.

**Commit:** `feat(render): site-wide nav link to /graph/ (P4 B-5)`

### B-6 — wire into `crates/docgen/src/build.rs` (emit page + asset + flag)

**Implement** in `build(project_root)` after Phase 2 (doc pages), before/with the
asset emit:

```rust
// Phase 3: the /graph/ page (default-on). Deterministic layout from the
// already-built link graph; never recomputes links.
let graph_data = site.graph_data(docgen_core::graphlayout::LayoutParams::default());
let graph_json = docgen_core::graphlayout::graph_data_json(&graph_data);
let graph_html = renderer.render_graph(&docgen_render::GraphContext {
    tree: &tree,
    graph_json: &graph_json,
    node_count: graph_data.nodes.len(),
    edge_count: graph_data.edges.len(),
})?;
let graph_dir = dist_dir.join("graph");
fs::create_dir_all(&graph_dir)?;
fs::write(graph_dir.join("index.html"), graph_html)?;
```

And flip the asset flag:

```rust
let emit_opts = docgen_assets::EmitOptions {
    include_katex_runtime: false,
    include_mermaid: site.any_mermaid,
    include_graph: true,   // /graph/ always emitted in P4
};
```

(`include_graph: true` is unconditional in P4 — see §0.3. The P6 toggle flips this
one bool.)

Update the build summary `println!` to mention the graph page if you like (optional;
no test asserts it).

No new unit test here — covered by the e2e in B-7. `cargo build -p docgen` must
compile. `cargo clippy --all-targets`.

**Commit:** `feat(docgen): emit /graph/ page + graph island in build (P4 B-6)`

### B-7 — end-to-end test (`crates/docgen/tests/build_cli.rs`)

**Test** (new test, mirroring `builds_mermaid_page_with_lazy_island`):

```rust
/// Graph view: the build emits /graph/ with embedded GraphData JSON, mounts the
/// docgenGraph island, ships islands/graph.js, and every page links to /graph/.
#[test]
fn builds_graph_page_with_island_and_nav_link() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent().unwrap().parent().unwrap();
    let fixture = workspace.join("fixtures/site-basic");

    let tmp = std::env::temp_dir().join(format!("docgen_build_cli_graph_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(tmp.join("docs/guide")).unwrap();
    fs::copy(fixture.join("docs/index.md"), tmp.join("docs/index.md")).unwrap();
    fs::copy(fixture.join("docs/guide/intro.md"), tmp.join("docs/guide/intro.md")).unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build").arg(&tmp).status().unwrap();
    assert!(status.success());

    // /graph/ page exists with the island + embedded data + meta.
    let graph = fs::read_to_string(tmp.join("dist/graph/index.html")).unwrap();
    assert!(graph.contains("<title>Graph</title>"));
    assert!(graph.contains(r#"x-data="docgenGraph""#));
    assert!(graph.contains(r#"id="docgen-graph-data""#));
    assert!(graph.contains(r#"src="/islands/graph.js""#));

    // Embedded JSON is real, parseable, and reflects the two docs + their links.
    let start = graph.find("docgen-graph-data").unwrap();
    let open = graph[start..].find('>').unwrap() + start + 1;
    let close = graph[open..].find("</script>").unwrap() + open;
    let json = &graph[open..close];
    let data: serde_json::Value = serde_json::from_str(json).unwrap();
    let nodes = data["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().any(|n| n["slug"] == "index"));
    assert!(nodes.iter().any(|n| n["slug"] == "guide/intro"));
    let edges = data["edges"].as_array().unwrap();
    assert!(edges.iter().any(|e| e["from"] == "index" && e["to"] == "guide/intro"));

    // Determinism at the integration level: positions are finite, in-bounds.
    for n in nodes {
        let (x, y) = (n["x"].as_f64().unwrap(), n["y"].as_f64().unwrap());
        assert!(x.is_finite() && y.is_finite());
        assert!((74.0..=1420.0 - 74.0).contains(&x));
        assert!((74.0..=760.0 - 74.0).contains(&y));
    }

    // The island JS is emitted.
    assert!(tmp.join("dist/islands/graph.js").is_file());
    // No d3 / graph lib vendored anywhere.
    let island = fs::read_to_string(tmp.join("dist/islands/graph.js")).unwrap();
    assert!(!island.to_lowercase().contains("d3"));

    // Every doc page links to /graph/.
    let home = fs::read_to_string(tmp.join("dist/index/index.html")).unwrap();
    assert!(home.contains(r#"href="/graph""#));

    let _ = fs::remove_dir_all(&tmp);
}
```

Also: extend the existing `builds_fixture_site` test (or add a one-liner) to assert
`dist/graph/index.html` exists and `dist/islands/graph.js` exists, so the default
fixture build always exercises the graph path. **Do not** weaken the existing
mermaid/math negative assertions — graph assets are additive and distinct paths.

**Implement:** nothing new (B-1..B-6 already do the work). Run:

```
cargo test -p docgen --test build_cli
cargo test            # full workspace
cargo clippy --all-targets
```

All green.

**Commit:** `test(docgen): e2e graph page emission + embedded data + nav (P4 B-7)`

### B-8 — (optional, nice-to-have) fixture doc that makes the graph non-trivial

If `fixtures/site-basic` is too sparse to eyeball in-browser, add a couple of
cross-linked docs (e.g. a small hub-and-spoke set) under a new
`fixtures/site-graph/` and a `#[ignore]`-free e2e that builds it. **Only if it adds
signal** — otherwise skip; B-7 already proves the path. If added, keep it tiny and
deterministic and assert node/edge counts.

**Commit (if done):** `test(docgen): site-graph fixture for in-browser graph verification (P4 B-8)`

**Cluster B done-gate:** `cargo test` (full workspace) green, `cargo clippy
--all-targets` clean, B-1..B-7 committed. Architect then opens
`dist/graph/index.html` in a browser to verify hover-highlight, click-navigate, and
pan/zoom behave (the SVG-interactivity claims this plan does not unit-test).

---

## 2. In-browser verification checklist (architect, post-Cluster-B)

Build a fixture site, serve `dist/`, open `/graph/`:

1. Nodes + edges render as SVG; node radius scales with degree.
2. Hovering a node highlights it + its neighbours + incident edges; others dim.
3. Clicking a node navigates to `/{slug}`.
4. Dragging the frame pans; scroll/wheel zooms; pan-drag does **not** trigger a
   node click; pan/zoom are clamped (no infinite drift).
5. The "N nodes · M links" caption matches the embedded JSON counts.
6. No console errors; no network request for any graph/d3 library (only the
   embedded JSON + `islands/graph.js` + alpine + bootstrap).
7. The nav link to `/graph/` appears on a normal doc page and round-trips back.

---

## 3. Determinism & no-npm guarantees (summary of the testable invariants)

- `layout_graph` is a pure function of `(nodes_meta, LinkGraph, LayoutParams)` —
  no RNG, no clock, index-ordered iteration, fixed `iterations`. Tested by
  `seed_positions_are_deterministic_and_in_bounds`,
  `connected_pair_settles_near_target_edge_length` (both run twice, assert equal).
- Coordinates rounded to 2dp → byte-stable JSON across platforms
  (`coordinates_are_rounded_for_stable_json`).
- Empty / single / disconnected / self-loop / ghost-edge cases all covered in A-2/A-3/A-4.
- No new crates; no ESM in `islands/graph.js`; no vendored graph lib (asserted by
  `graph_island_registers_and_renders_without_esm_or_d3` and the e2e `d3` scan).
- `/graph/` default-on; P6 flips the single `include_graph` / page-emit bool.

---

## 4. File manifest (everything P4 touches)

**New:**
- `crates/docgen-core/src/graphlayout.rs` (Cluster A; all of §0.5's core types + fns)
- `crates/docgen-assets/assets/docgen/islands/graph.js` (Cluster B)
- `crates/docgen-render/templates/graph.html` (Cluster B)
- `fixtures/site-graph/...` (optional, B-8)

**Modified:**
- `crates/docgen-core/src/lib.rs` (`pub mod graphlayout;` + re-exports)
- `crates/docgen-core/src/pipeline.rs` (`SiteBuild::graph_data`)
- `crates/docgen-assets/src/lib.rs` (`include_graph`, `graph_assets`, `assets_for`)
- `crates/docgen-assets/assets/docgen/docgen.css` (graph styles)
- `crates/docgen-render/src/lib.rs` (`GraphContext`, `DEFAULT_GRAPH_TEMPLATE`,
  `render_graph`, register `graph.html`)
- `crates/docgen-render/templates/page.html` (nav link to `/graph/`)
- `crates/docgen/src/build.rs` (emit `/graph/`, `include_graph: true`)
- `crates/docgen/tests/build_cli.rs` (B-7 e2e; extend `builds_fixture_site`)

**No new Cargo dependencies anywhere.**
