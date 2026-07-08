//! Incremental rebuilds for the dev server.
//!
//! A full [`build_site`](crate::build_site) is O(n²) in the doc count — the
//! force-directed graph layout and the per-page nav-tree render both dominate at
//! scale (a 2.5k-doc corpus takes ~10s). That cost is fine for `docgen build`
//! and acceptable for the dev server's *initial* build, but rebuilding the whole
//! site on every keystroke-save makes large workspaces painful to edit.
//!
//! [`DevState`] seeds itself from the initial full build's in-memory artifacts
//! (no second pass) and, on each subsequent change, attempts a **fast path**: it
//! re-discovers the docs, re-renders only the doc(s) whose body actually changed,
//! and rewrites only those pages — reusing the cached nav tree, graph layout,
//! diff workspace, and assets untouched. The fast path is taken ONLY when it is
//! provably equivalent to a full rebuild: the set/order of slugs, every title and
//! description, the partial set, the link graph (edges + backlinks), and the used
//! component-island set must all be unchanged. Any structural change falls back
//! to a full rebuild, which re-seeds the cache. The result is byte-identical to a
//! full build for the pages it touches, and leaves every other file exactly as the
//! last full build wrote it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use docgen_core::graph::{build_link_graph, LinkGraph};
use docgen_core::model::{Doc, SearchEntry, TreeNode};
use docgen_core::pipeline::{partition_partials, prepare, render_doc, Partials, PreparedDoc};
use docgen_core::wikilink::SlugSet;
use docgen_render::{HomeRecent, HomeSection, Renderer, DEFAULT_PAGE_TEMPLATE};

use crate::{
    build_site_inner, compute_home_rows, render_one_page, BuildMode, BuildOptions, PageShared,
    HOME_SLUG,
};

/// In-memory artifacts captured from a full build, enough to (a) detect whether a
/// later change is structural and (b) re-render any single page without touching
/// the rest of the site. Produced by [`build_site_inner`] when `capture` is set.
pub(crate) struct CapturedBuild {
    pub config: docgen_config::SiteConfig,
    pub registry: docgen_components::Registry,
    pub partials: Partials,
    pub prepared: Vec<PreparedDoc>,
    pub docs: Vec<Doc>,
    pub outbound: BTreeMap<String, Vec<String>>,
    pub graph: LinkGraph,
    pub tree: Vec<TreeNode>,
    pub graph_payload: Option<(String, usize, usize)>,
    pub island_components: BTreeSet<String>,
    pub has_components_css: bool,
    pub commit_hash: String,
    pub built_stamp: String,
    pub has_diff: bool,
    pub search: Vec<SearchEntry>,
}

/// Which kind of rebuild [`DevState::rebuild`] performed. `Full` wipes and
/// repopulates `out_dir` (via the atomic staging swap), so the dev server must
/// re-emit its dev-only assets afterward; `Incremental` writes only the changed
/// pages in place and leaves everything else (including dev assets) intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebuildKind {
    Full,
    Incremental,
}

/// Outcome of a dev rebuild: the kind taken plus the page count.
#[derive(Debug, Clone)]
pub struct Rebuilt {
    pub kind: RebuildKind,
    pub page_count: usize,
}

/// The dev server's persistent incremental build engine. Holds the renderer and
/// the last full build's artifacts; [`rebuild`](DevState::rebuild) takes the fast
/// path when it can prove equivalence and falls back to a full build otherwise.
pub struct DevState {
    project_root: PathBuf,
    out_dir: PathBuf,
    renderer: Renderer,
    cap: CapturedBuild,
}

impl DevState {
    /// Run the initial full build (Dev mode) and seed the engine from it.
    pub fn initial(project_root: &Path, out_dir: &Path) -> Result<(Self, Rebuilt)> {
        let (outcome, cap) = build_site_inner(
            &BuildOptions {
                project_root,
                out_dir,
                mode: BuildMode::Dev,
            },
            true,
        )?;
        let cap = cap.expect("capture requested → CapturedBuild present");
        let renderer = Renderer::new(DEFAULT_PAGE_TEMPLATE)?;
        Ok((
            Self {
                project_root: project_root.to_path_buf(),
                out_dir: out_dir.to_path_buf(),
                renderer,
                cap,
            },
            Rebuilt {
                kind: RebuildKind::Full,
                page_count: outcome.page_count,
            },
        ))
    }

    /// Rebuild after a filesystem change. Re-discovers the docs and takes the fast
    /// path (re-render only changed pages) when the change is provably non-
    /// structural; otherwise falls back to a full build and re-seeds the cache.
    pub fn rebuild(&mut self) -> Result<Rebuilt> {
        match self.try_incremental()? {
            Some(rebuilt) => Ok(rebuilt),
            None => self.full(),
        }
    }

    /// Run a full build and replace the cached artifacts.
    fn full(&mut self) -> Result<Rebuilt> {
        let (outcome, cap) = build_site_inner(
            &BuildOptions {
                project_root: &self.project_root,
                out_dir: &self.out_dir,
                mode: BuildMode::Dev,
            },
            true,
        )?;
        self.cap = cap.expect("capture requested → CapturedBuild present");
        Ok(Rebuilt {
            kind: RebuildKind::Full,
            page_count: outcome.page_count,
        })
    }

    /// Attempt the fast path. Returns `Ok(Some(_))` when it succeeded, `Ok(None)`
    /// when the change is structural and the caller must fall back to a full
    /// build, or `Err` on a hard I/O/discovery failure.
    fn try_incremental(&mut self) -> Result<Option<Rebuilt>> {
        let docs_dir = self.project_root.join("docs");
        let raws = match docgen_core::discover::discover_docs(&docs_dir) {
            Ok(r) => r,
            // A discovery failure is a hard error → let the full path surface it.
            Err(_) => return Ok(None),
        };
        let (pages, partials_new) = partition_partials(raws);
        let prepared_new: Vec<PreparedDoc> = pages.into_iter().map(prepare).collect();

        // Partials feed `:include` transclusions whose dependents we don't track;
        // any partial change forces a full rebuild.
        if partials_new != self.cap.partials {
            return Ok(None);
        }
        // The doc set + order must match exactly: an add/remove/rename/reorder
        // changes the tree, sections, recent list, and graph node order.
        if prepared_new.len() != self.cap.prepared.len() {
            return Ok(None);
        }
        let mut changed: Vec<usize> = Vec::new();
        for (i, (new, old)) in prepared_new.iter().zip(&self.cap.prepared).enumerate() {
            if new.slug != old.slug {
                return Ok(None);
            }
            // Title/description feed the sidebar tree, backlink cards on other
            // pages, and the home sections/recent — all cross-page. Defer to full.
            if new.title != old.title || new.description != old.description {
                return Ok(None);
            }
            if new.body_md != old.body_md {
                changed.push(i);
            }
        }

        // Nothing actually changed (e.g. a touch / metadata-only fs event): no
        // pages to rewrite, but report a successful incremental so the caller
        // still fires a reload.
        if changed.is_empty() {
            return Ok(Some(Rebuilt {
                kind: RebuildKind::Incremental,
                page_count: self.cap.docs.len(),
            }));
        }

        // Re-render only the changed docs against the (unchanged) site slug set.
        let slugs: SlugSet = self.cap.prepared.iter().map(|p| p.slug.clone()).collect();
        let mut rerendered: Vec<(usize, docgen_core::pipeline::RenderedDoc)> =
            Vec::with_capacity(changed.len());
        for &i in &changed {
            let rd = render_doc(
                &prepared_new[i],
                &self.cap.config,
                &self.cap.registry,
                &slugs,
                &partials_new,
            );
            rerendered.push((i, rd));
        }

        // Rebuild the link graph from the cached outbound map with the changed
        // docs' entries swapped in. If the topology (edges) or backlinks differ,
        // the layout and other pages' backlink rails are affected → full rebuild.
        let mut outbound_new = self.cap.outbound.clone();
        for (i, rd) in &rerendered {
            outbound_new.insert(
                self.cap.prepared[*i].slug.clone(),
                rd.resolved_links.clone(),
            );
        }
        let doc_meta: Vec<(String, String, Option<String>)> = self
            .cap
            .docs
            .iter()
            .map(|d| (d.slug.clone(), d.title.clone(), d.description.clone()))
            .collect();
        let graph_new = build_link_graph(&doc_meta, &outbound_new);
        if graph_new.edges != self.cap.graph.edges
            || graph_new.backlinks != self.cap.graph.backlinks
        {
            return Ok(None);
        }

        // The used component-island set drives the shared components.js bundle and
        // every page's island link gating; if it changed, the bundle + other pages
        // are affected → full rebuild. (Compute the prospective set from the new
        // docs and compare to the cached one.)
        let island_new = self.island_set_after(&rerendered);
        if island_new != self.cap.island_components {
            return Ok(None);
        }

        // ---- Fast path committed: every gate proved equivalence. ----
        // Patch the cache with the re-rendered docs.
        for (i, rd) in rerendered {
            self.cap.search[i] = SearchEntry {
                slug: self.cap.docs[i].slug.clone(),
                title: self.cap.docs[i].title.clone(),
                text: rd.search_text,
            };
            self.cap.docs[i] = rd.doc;
        }
        self.cap.outbound = outbound_new;
        self.cap.prepared = prepared_new;
        self.cap.partials = partials_new;

        // Re-render + write only the changed pages, reusing the cached tree,
        // graph layout, home rows, and per-page chrome.
        let (section_rows, recent_rows) = compute_home_rows(&self.cap.docs);
        let home_sections: Vec<HomeSection> = section_rows
            .iter()
            .map(|(label, slug, count)| HomeSection {
                label,
                slug,
                count: *count,
            })
            .collect();
        let home_recent: Vec<HomeRecent> = recent_rows
            .iter()
            .map(|(title, slug, section)| HomeRecent {
                title,
                slug,
                section,
            })
            .collect();
        let shared = PageShared {
            tree: &self.cap.tree,
            graph: &self.cap.graph,
            commit: &self.cap.commit_hash,
            built: &self.cap.built_stamp,
            base: &self.cap.config.base,
            site_title: self.cap.config.title.as_deref().unwrap_or(""),
            search_enabled: self.cap.config.features.search,
            has_diff: self.cap.has_diff,
            has_components_css: self.cap.has_components_css,
            island_components: &self.cap.island_components,
            graph_payload: &self.cap.graph_payload,
            home_sections: &home_sections,
            home_recent: &home_recent,
            pages_count: self.cap.docs.len(),
            total_links: self.cap.graph.edges.len(),
        };

        for &i in &changed {
            let doc = &self.cap.docs[i];
            let html = render_one_page(&self.renderer, &shared, doc)?;
            let dir = self.out_dir.join(&doc.slug);
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("index.html"), &html)?;
            // The home doc is also served at the site root.
            if doc.slug == HOME_SLUG {
                std::fs::write(self.out_dir.join("index.html"), &html)?;
            }
        }

        // The search index aggregates every doc's text, so a single changed doc
        // means rewriting it — cheap relative to a full O(n²) rebuild.
        if self.cap.config.features.search {
            std::fs::write(
                self.out_dir.join("search-index.json"),
                docgen_core::search::index_json(&self.cap.search),
            )?;
        }

        Ok(Some(Rebuilt {
            kind: RebuildKind::Incremental,
            page_count: self.cap.docs.len(),
        }))
    }

    /// The used component-island set the site would have after applying the
    /// re-rendered docs: every doc's `components_used` ∩ the registry's islands.
    /// Mirrors the `island_components` set [`build_site_inner`] computes.
    fn island_set_after(
        &self,
        rerendered: &[(usize, docgen_core::pipeline::RenderedDoc)],
    ) -> BTreeSet<String> {
        let islands: BTreeSet<&str> = self
            .cap
            .registry
            .islands()
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        let mut used: BTreeSet<String> = BTreeSet::new();
        for (i, doc) in self.cap.docs.iter().enumerate() {
            // Use the re-rendered components for changed docs, the cached ones else.
            let components = rerendered
                .iter()
                .find(|(j, _)| *j == i)
                .map(|(_, rd)| &rd.doc.components_used)
                .unwrap_or(&doc.components_used);
            for c in components {
                if islands.contains(c.as_str()) {
                    used.insert(c.clone());
                }
            }
        }
        used
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Write a small multi-doc corpus into `<root>/docs` and return the root.
    fn corpus(dir: &Path) {
        let docs = dir.join("docs");
        fs::create_dir_all(docs.join("guide")).unwrap();
        fs::write(
            docs.join("index.md"),
            "# Home\n\nWelcome. See [[guide/a]].\n",
        )
        .unwrap();
        fs::write(
            docs.join("guide/a.md"),
            "# Alpha\n\nAlpha body. Link to [[guide/b]].\n",
        )
        .unwrap();
        fs::write(
            docs.join("guide/b.md"),
            "# Beta\n\nBeta body. Link to [[guide/a]].\n",
        )
        .unwrap();
    }

    /// The "Built" timestamp is wall-clock and the only field that legitimately
    /// varies between two builds, so mask it before comparing for equivalence.
    fn mask_built(html: &str, stamp: &str) -> String {
        if stamp.is_empty() {
            return html.to_string();
        }
        html.replace(stamp, "BUILT")
    }

    #[test]
    fn incremental_body_edit_matches_full_rebuild_and_leaves_others_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        corpus(root);
        let out = root.join("out");

        let (mut state, first) = DevState::initial(root, &out).unwrap();
        assert_eq!(first.kind, RebuildKind::Full);
        let init_stamp = state.cap.built_stamp.clone();

        // Record the bytes of the pages we expect NOT to change.
        let index_before = fs::read(out.join("index.html")).unwrap();
        let b_before = fs::read(out.join("guide/b/index.html")).unwrap();

        // Edit ONLY doc A's body (same title, same outbound links).
        fs::write(
            root.join("docs/guide/a.md"),
            "# Alpha\n\nAlpha body REVISED with new prose. Link to [[guide/b]].\n",
        )
        .unwrap();

        let r = state.rebuild().unwrap();
        assert_eq!(
            r.kind,
            RebuildKind::Incremental,
            "body-only edit must be incremental"
        );

        let a_incremental = fs::read_to_string(out.join("guide/a/index.html")).unwrap();
        assert!(
            a_incremental.contains("REVISED with new prose"),
            "incremental page reflects the edit"
        );

        // The unrelated pages are byte-for-byte untouched.
        assert_eq!(
            fs::read(out.join("index.html")).unwrap(),
            index_before,
            "home page must not be rewritten by a body edit elsewhere"
        );
        assert_eq!(
            fs::read(out.join("guide/b/index.html")).unwrap(),
            b_before,
            "sibling page must not be rewritten"
        );

        // Equivalence: a full rebuild of the edited corpus produces the same A page
        // (modulo the wall-clock Built stamp).
        let ref_out = root.join("ref");
        let (_outcome, refcap) = build_site_inner(
            &BuildOptions {
                project_root: root,
                out_dir: &ref_out,
                mode: BuildMode::Dev,
            },
            true,
        )
        .unwrap();
        let refcap = refcap.unwrap();
        let a_full = fs::read_to_string(ref_out.join("guide/a/index.html")).unwrap();
        assert_eq!(
            mask_built(&a_incremental, &init_stamp),
            mask_built(&a_full, &refcap.built_stamp),
            "incremental page is byte-identical to a full rebuild's page"
        );
    }

    #[test]
    fn title_change_falls_back_to_full() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        corpus(root);
        let out = root.join("out");
        let (mut state, _) = DevState::initial(root, &out).unwrap();

        // Changing the H1 changes the derived title → sidebar + cross-page → full.
        fs::write(
            root.join("docs/guide/a.md"),
            "# Alpha Renamed\n\nAlpha body. Link to [[guide/b]].\n",
        )
        .unwrap();
        assert_eq!(state.rebuild().unwrap().kind, RebuildKind::Full);
    }

    #[test]
    fn adding_a_link_falls_back_to_full() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        corpus(root);
        let out = root.join("out");
        let (mut state, _) = DevState::initial(root, &out).unwrap();

        // Adding an outbound wikilink changes graph topology + a backlink → full.
        fs::write(
            root.join("docs/guide/a.md"),
            "# Alpha\n\nAlpha body. Link to [[guide/b]] and now [[index]].\n",
        )
        .unwrap();
        assert_eq!(state.rebuild().unwrap().kind, RebuildKind::Full);
    }

    #[test]
    fn adding_a_new_doc_falls_back_to_full() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        corpus(root);
        let out = root.join("out");
        let (mut state, _) = DevState::initial(root, &out).unwrap();

        fs::write(root.join("docs/guide/c.md"), "# Gamma\n\nNew page.\n").unwrap();
        assert_eq!(state.rebuild().unwrap().kind, RebuildKind::Full);
    }

    #[test]
    fn no_op_change_is_incremental() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        corpus(root);
        let out = root.join("out");
        let (mut state, _) = DevState::initial(root, &out).unwrap();

        // Rewrite identical bytes (a bare `touch`-like save): no changed docs.
        fs::write(
            root.join("docs/guide/a.md"),
            "# Alpha\n\nAlpha body. Link to [[guide/b]].\n",
        )
        .unwrap();
        assert_eq!(state.rebuild().unwrap().kind, RebuildKind::Incremental);
    }
}
