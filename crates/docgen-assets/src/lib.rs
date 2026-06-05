//! `docgen-assets` owns every vendored and authored frontend file used by the
//! generated doc site. Files are embedded at compile time via `include_dir!`,
//! and exposed through a typed enumerate/emit API. The build subcommand drives
//! emission via [`assets_for`] + [`emit`].

use include_dir::{include_dir, Dir};
use std::path::Path;

/// Every vendored + authored frontend file, embedded at compile time.
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// Embedded built-in component sources (`components/<name>/...`). Loaded through
/// the SAME raw parts a project component is, so built-ins dogfood the mechanism.
static COMPONENTS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/components");

/// Raw parts of one embedded built-in component. Returned as borrowed `'static`
/// strings so this crate stays dependency-free of `docgen-components`; the build
/// assembles these into `docgen_components::Component`s.
pub struct BuiltinComponent {
    pub name: &'static str,
    pub template: &'static str,
    pub island_js: Option<&'static str>,
    pub style_css: Option<&'static str>,
}

/// `(name, template, island_js?, style_css?)` for each embedded built-in
/// component, name-sorted for deterministic output.
pub fn builtin_components() -> Vec<BuiltinComponent> {
    fn text(name: &str, file: &str) -> Option<&'static str> {
        COMPONENTS
            .get_file(format!("{name}/{file}"))
            .and_then(|f| f.contents_utf8())
    }
    let mut subs: Vec<&Dir> = COMPONENTS.dirs().collect();
    subs.sort_by_key(|d| d.path().file_name().map(|n| n.to_owned()));
    let mut out = Vec::new();
    for sub in subs {
        let name = sub
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .expect("component dir has a utf-8 name");
        let template =
            text(name, "template.html").expect("builtin component needs template.html");
        out.push(BuiltinComponent {
            name,
            template,
            island_js: text(name, "island.js"),
            style_css: text(name, "style.css"),
        });
    }
    out
}

/// Coarse classification of an emitted asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Css,
    Js,
    Font,
    Json,
    Other,
}

/// One embedded frontend file, ready to write into `dist/`.
#[derive(Debug, Clone, Copy)]
pub struct Asset {
    /// dist-relative path, e.g. `"docgen.css"`, `"vendor/katex/katex.min.css"`.
    pub path: &'static str,
    /// Embedded file contents.
    pub bytes: &'static [u8],
    pub kind: AssetKind,
}

/// Read an embedded file (`src_path`) into an [`Asset`] written at `dist_path`.
fn embed(src_path: &'static str, dist_path: &'static str, kind: AssetKind) -> Asset {
    let bytes = ASSETS
        .get_file(src_path)
        .unwrap_or_else(|| panic!("embedded asset missing: {src_path}"))
        .contents();
    Asset {
        path: dist_path,
        bytes,
        kind,
    }
}

/// Embed a file whose dist path equals its source path (the common case). Use
/// [`embed`] directly only for the few `docgen/...` -> root rewrites.
fn same(path: &'static str, kind: AssetKind) -> Asset {
    embed(path, path, kind)
}

/// Assets emitted on every build: Alpine, bootstrap, shared css, search client.
pub fn core_assets() -> Vec<Asset> {
    vec![
        same("vendor/alpine/alpine.min.js", AssetKind::Js),
        embed("docgen/bootstrap.js", "bootstrap.js", AssetKind::Js),
        embed("docgen/docgen.css", "docgen.css", AssetKind::Css),
        embed("docgen/search.js", "search.js", AssetKind::Js),
        embed(
            "docgen/islands/theme-toggle.js",
            "islands/theme-toggle.js",
            AssetKind::Js,
        ),
    ]
}

/// KaTeX stylesheet + fonts, always emitted (build-time-rendered HTML needs them).
///
/// Font dist paths stay siblings of the css under `vendor/katex/` so the css's
/// relative `url(fonts/...)` references resolve. Paths are `&'static str`
/// literals (no runtime formatting), so each `Asset` carries embedded bytes
/// directly.
pub fn katex_css_assets() -> Vec<Asset> {
    macro_rules! font {
        ($name:literal) => {
            same(
                concat!("vendor/katex/fonts/KaTeX_", $name, ".woff2"),
                AssetKind::Font,
            )
        };
    }
    vec![
        same("vendor/katex/katex.min.css", AssetKind::Css),
        font!("AMS-Regular"),
        font!("Caligraphic-Bold"),
        font!("Caligraphic-Regular"),
        font!("Fraktur-Bold"),
        font!("Fraktur-Regular"),
        font!("Main-Bold"),
        font!("Main-BoldItalic"),
        font!("Main-Italic"),
        font!("Main-Regular"),
        font!("Math-BoldItalic"),
        font!("Math-Italic"),
        font!("SansSerif-Bold"),
        font!("SansSerif-Italic"),
        font!("SansSerif-Regular"),
        font!("Script-Regular"),
        font!("Typewriter-Regular"),
    ]
}

/// Runtime KaTeX (`katex.min.js` + `auto-render.min.js`). Emitted ONLY when
/// [`EmitOptions::include_katex_runtime`] is set — the documented fallback if
/// the build-time `katex` crate ever cannot compile on a host. The default
/// build renders math at build time and ships **zero** runtime JS for math.
pub fn katex_runtime_assets() -> Vec<Asset> {
    vec![
        same("vendor/katex/katex.min.js", AssetKind::Js),
        same("vendor/katex/auto-render.min.js", AssetKind::Js),
    ]
}

/// Mermaid library + island glue. Emitted only when a page used a diagram
/// (gated by [`EmitOptions::include_mermaid`]). The island lazy-loads
/// `mermaid.min.js` at runtime, so it ships only on pages with a diagram.
pub fn mermaid_assets() -> Vec<Asset> {
    vec![
        same("vendor/mermaid/mermaid.min.js", AssetKind::Js),
        embed(
            "docgen/islands/mermaid.js",
            "islands/mermaid.js",
            AssetKind::Js,
        ),
    ]
}

/// Graph island JS. Emitted only on builds that render the `/graph/` page
/// (gated by [`EmitOptions::include_graph`]). The graph styles live in the
/// shared `docgen.css`, so this slice is JS-only. No vendored graph lib — the
/// island reads build-time `GraphData` JSON and draws SVG by hand.
pub fn graph_assets() -> Vec<Asset> {
    vec![embed(
        "docgen/islands/graph.js",
        "islands/graph.js",
        AssetKind::Js,
    )]
}

/// DEV-ONLY assets, served by `docgen dev` ONLY. NEVER returned by [`assets_for`]
/// and NEVER emitted by `docgen build`. Dist paths are namespaced under
/// `__docgen/` and `__codemirror/` so they cannot collide with doc slugs.
///
/// Contents: the vendored CodeMirror 5 UMD (`codemirror.js` + css), its markdown
/// mode and the `xml` mode + `overlay` addon that markdown mode depends on, the
/// editor island JS + css, and the live-reload client. All loadable as plain
/// `<script>`/`<link>` tags with no bundler/import resolution.
pub fn dev_assets() -> Vec<Asset> {
    vec![
        embed(
            "vendor/codemirror/codemirror.js",
            "__codemirror/codemirror.js",
            AssetKind::Js,
        ),
        embed(
            "vendor/codemirror/codemirror.css",
            "__codemirror/codemirror.css",
            AssetKind::Css,
        ),
        embed(
            "vendor/codemirror/markdown.js",
            "__codemirror/markdown.js",
            AssetKind::Js,
        ),
        embed(
            "vendor/codemirror/xml.js",
            "__codemirror/xml.js",
            AssetKind::Js,
        ),
        embed(
            "vendor/codemirror/overlay.js",
            "__codemirror/overlay.js",
            AssetKind::Js,
        ),
        embed("docgen/dev/editor.js", "__docgen/editor.js", AssetKind::Js),
        embed(
            "docgen/dev/editor.css",
            "__docgen/editor.css",
            AssetKind::Css,
        ),
        embed(
            "docgen/dev/livereload.js",
            "__docgen/livereload.js",
            AssetKind::Js,
        ),
    ]
}

/// Flags driving which conditional asset slices a build emits.
///
/// Both fields default to `false`: the default build takes the build-time KaTeX
/// path (no runtime JS) and emits mermaid only when a page used a diagram.
#[derive(Debug, Clone, Copy)]
pub struct EmitOptions {
    /// Ship runtime `katex.min.js` + `auto-render.min.js` (fallback path). Default false.
    pub include_katex_runtime: bool,
    /// Ship `mermaid.min.js` + the mermaid island (only when a page used a diagram).
    pub include_mermaid: bool,
    /// Ship the graph island (`islands/graph.js`) — emitted when the `/graph/`
    /// page is rendered. Default false.
    pub include_graph: bool,
    /// Emit `components.css` (set when any component had a `style.css`). The
    /// authored bytes are concatenated by the build and written via
    /// [`emit_component_bundle`], not [`assets_for`]. Default false.
    pub include_component_css: bool,
    /// Emit `components.js` (set when any *used* component had an `island.js`).
    /// Written via [`emit_component_bundle`], not [`assets_for`]. Default false.
    pub include_component_js: bool,
    /// Ship the search client (`search.js`). Gated by `[features] search`. The
    /// page template only links it when search is enabled, so dropping it here
    /// keeps the dist free of an orphan asset. Default `true` (search on).
    pub include_search: bool,
}

impl Default for EmitOptions {
    /// The default build: build-time KaTeX (no runtime JS), mermaid/graph only
    /// when used, component bundles written elsewhere — and search **on**, the
    /// pre-P6 behaviour every feature-toggle test relies on.
    fn default() -> Self {
        Self {
            include_katex_runtime: false,
            include_mermaid: false,
            include_graph: false,
            include_component_css: false,
            include_component_js: false,
            include_search: true,
        }
    }
}

/// The full asset set to emit for this build: core + katex css (always, for math
/// output) + conditional runtime/mermaid. Stable ordering for deterministic tests.
pub fn assets_for(opts: &EmitOptions) -> Vec<Asset> {
    let mut out = core_assets();
    if !opts.include_search {
        out.retain(|a| a.path != "search.js");
    }
    out.extend(katex_css_assets());
    if opts.include_katex_runtime {
        out.extend(katex_runtime_assets());
    }
    if opts.include_mermaid {
        out.extend(mermaid_assets());
    }
    if opts.include_graph {
        out.extend(graph_assets());
    }
    out
}

/// Write every asset under `dist`, creating parent dirs. `dist` is wiped per
/// build, so there is no staleness concern; this always overwrites.
pub fn emit(assets: &[Asset], dist: &Path) -> std::io::Result<()> {
    for a in assets {
        let out = dist.join(a.path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, a.bytes)?;
    }
    Ok(())
}

/// Write the authored component CSS/JS the build concatenated from the registry.
/// These bytes are dynamic (authored content, not `&'static` embedded files), so
/// they flow through here rather than [`assets_for`]/[`emit`]. An empty string
/// skips its file (per-page gating decides whether a page *links* them). `css` is
/// the concatenation of every component `style.css`; `js` is the concatenation of
/// the `island.js` of every *used* island component.
pub fn emit_component_bundle(dist: &Path, css: &str, js: &str) -> std::io::Result<()> {
    if !css.is_empty() {
        std::fs::write(dist.join("components.css"), css)?;
    }
    if !js.is_empty() {
        std::fs::write(dist.join("components.js"), js)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_vendored_tree() {
        let alpine = ASSETS
            .get_file("vendor/alpine/alpine.min.js")
            .expect("alpine embedded");
        assert!(!alpine.contents().is_empty());
    }

    #[test]
    fn ships_builtin_callout_component() {
        let comps = builtin_components();
        let c = comps
            .iter()
            .find(|c| c.name == "callout")
            .expect("callout builtin");
        assert!(c.template.contains("docgen-callout"));
        assert!(c.style_css.is_some());
        assert!(c.island_js.is_none()); // pure build-time
    }

    #[test]
    fn embeds_authored_docgen_files() {
        for p in ["docgen/docgen.css", "docgen/search.js", "docgen/bootstrap.js"] {
            assert!(ASSETS.get_file(p).is_some(), "missing embedded {p}");
        }
    }

    // ---- A-4: typed API + core_assets() ----

    #[test]
    fn core_assets_cover_alpine_bootstrap_css_search() {
        let paths: Vec<_> = core_assets().iter().map(|a| a.path).collect();
        assert!(paths.contains(&"vendor/alpine/alpine.min.js"));
        assert!(paths.contains(&"bootstrap.js"));
        assert!(paths.contains(&"docgen.css"));
        assert!(paths.contains(&"search.js"));
        assert!(paths.contains(&"islands/theme-toggle.js"));
    }

    #[test]
    fn core_assets_are_nonempty_and_kinded() {
        for a in core_assets() {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }
        assert_eq!(
            core_assets()
                .iter()
                .find(|a| a.path == "docgen.css")
                .unwrap()
                .kind,
            AssetKind::Css
        );
    }

    #[test]
    fn shared_css_retains_p1_classes() {
        let css = core_assets()
            .iter()
            .find(|a| a.path == "docgen.css")
            .unwrap()
            .bytes;
        let s = std::str::from_utf8(css).unwrap();
        assert!(s.contains("docgen-search-modal"));
        assert!(s.contains("docgen-diff-line--added"));
        assert!(s.contains("docgen-diff-line--removed"));
    }

    // ---- P7 Cluster A: theme-toggle island + design tokens + app shell css ----

    fn shared_css() -> &'static str {
        std::str::from_utf8(
            core_assets()
                .iter()
                .find(|a| a.path == "docgen.css")
                .unwrap()
                .bytes,
        )
        .unwrap()
    }

    #[test]
    fn core_assets_include_theme_toggle_island() {
        let a = core_assets()
            .into_iter()
            .find(|a| a.path == "islands/theme-toggle.js")
            .expect("theme-toggle island in core_assets");
        assert_eq!(a.kind, AssetKind::Js);
        assert!(!a.bytes.is_empty());
    }

    #[test]
    fn theme_toggle_island_registers_without_esm() {
        let js = std::str::from_utf8(
            ASSETS
                .get_file("docgen/islands/theme-toggle.js")
                .unwrap()
                .contents(),
        )
        .unwrap();
        assert!(js.contains("docgen.island"));
        assert!(js.contains("docgenThemeToggle"));
        assert!(js.contains("localStorage"));
        assert!(js.contains("setAttribute('data-theme'"));
        assert!(!js.contains("import ")); // no ESM / npm
    }

    #[test]
    fn shared_css_defines_theme_tokens() {
        let s = shared_css();
        assert!(s.contains(r#":root[data-theme="dark"]"#));
        for tok in ["--accent", "--bg", "--text", "--surface"] {
            assert!(s.contains(tok), "missing token {tok}");
        }
    }

    #[test]
    fn shared_css_has_layout_and_topbar() {
        let s = shared_css();
        for cls in [
            ".docgen-topbar",
            ".docgen-layout",
            ".docgen-sidebar",
            ".docgen-tree",
            ".docgen-content",
        ] {
            assert!(s.contains(cls), "css missing {cls}");
        }
    }

    #[test]
    fn shared_css_code_block_surface_is_theme_stable() {
        let s = shared_css();
        assert!(s.contains("--code-surface"));
        assert!(s.contains(".docgen-doc-content pre"));
    }

    #[test]
    fn shared_css_retains_legacy_selectors() {
        let s = shared_css();
        for sel in [
            ".docgen-search-modal",
            ".docgen-diff-line--added",
            ".docgen-diff-line--removed",
            ".katex-display",
            ".docgen-mermaid",
            ".docgen-graph",
            ".docgen-graph__nodes circle",
            ".docgen-graph__links line",
            ".docgen-wikilink--broken",
        ] {
            assert!(s.contains(sel), "css dropped legacy selector {sel}");
        }
    }

    #[test]
    fn shared_css_styles_search_modal_and_results() {
        let s = shared_css();
        for sel in [".docgen-search-box", ".docgen-search-result", ".is-selected"] {
            assert!(s.contains(sel), "search css missing {sel}");
        }
    }

    #[test]
    fn shared_css_styles_directive_error_surface() {
        // docgen core always emits `.docgen-directive-error`; it must read as an
        // error (tokenized), not inherit plain prose color.
        let s = shared_css();
        assert!(s.contains(".docgen-directive-error"));
    }

    #[test]
    fn shared_css_has_a11y_affordances() {
        let s = shared_css();
        // skip-to-content link, Alpine cloak (ships, not dev-only), and a
        // reduced-motion guard.
        assert!(s.contains(".docgen-skip-link"), "missing skip link style");
        assert!(s.contains("[x-cloak]"), "x-cloak must ship to avoid flashes");
        assert!(
            s.contains("prefers-reduced-motion"),
            "must honor reduced-motion"
        );
    }

    #[test]
    fn shared_css_mobile_tables_scroll_in_place() {
        // Wide tables must not force whole-page horizontal overflow on mobile.
        let s = shared_css();
        assert!(s.contains(".docgen-doc-content table"));
        assert!(s.contains("overflow-x: auto"));
    }

    #[test]
    fn shared_css_responsive_has_mobile_drawer() {
        let s = shared_css();
        assert!(s.contains("@media"));
        assert!(s.contains("max-width: 768px"));
        // the off-canvas drawer + open state
        assert!(s.contains(".docgen-sidebar.is-open"));
    }

    #[test]
    fn callout_css_uses_tokens_not_hardcoded_dark() {
        let c = builtin_components()
            .into_iter()
            .find(|c| c.name == "callout")
            .expect("callout builtin");
        let css = c.style_css.expect("callout has style.css");
        assert!(css.contains("var(--"), "callout css must use design tokens");
        assert!(
            !css.contains("#0b1220"),
            "callout css must not keep hardcoded dark surface"
        );
        // semantic variants stay tokenized
        for v in [".docgen-callout--note", ".docgen-callout--warning"] {
            assert!(css.contains(v), "callout css missing {v}");
        }
    }

    #[test]
    fn dev_editor_css_is_tokenized() {
        let css = std::str::from_utf8(
            ASSETS
                .get_file("docgen/dev/editor.css")
                .expect("editor.css embedded")
                .contents(),
        )
        .unwrap();
        assert!(css.contains("var(--"), "dev editor css must use tokens");
        // the production toggle/editor/save surfaces stay themed
        for sel in [".docgen-edit-toggle", "#docgen-editor", ".docgen-edit-save"] {
            assert!(css.contains(sel), "editor css missing {sel}");
        }
    }

    #[test]
    fn shared_css_has_katex_display_spacing() {
        let s = std::str::from_utf8(
            core_assets()
                .iter()
                .find(|a| a.path == "docgen.css")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert!(s.contains(".katex-display"));
    }

    #[test]
    fn shared_css_has_mermaid_container_styles() {
        let s = std::str::from_utf8(
            core_assets()
                .iter()
                .find(|a| a.path == "docgen.css")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert!(s.contains(".docgen-mermaid"));
    }

    // ---- A-5: bootstrap registry contract ----

    #[test]
    fn bootstrap_defines_registry_and_lazy_loader_without_esm() {
        let js = std::str::from_utf8(
            core_assets()
                .iter()
                .find(|a| a.path == "bootstrap.js")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert!(js.contains("docgen.island"));
        assert!(js.contains("docgen.loadScript"));
        assert!(js.contains("alpine:init"));
        assert!(!js.contains("import ")); // no ESM / npm
    }

    // ---- C-3: mermaid island contract ----

    #[test]
    fn mermaid_island_registers_and_lazy_loads_without_esm() {
        let js = std::str::from_utf8(
            ASSETS
                .get_file("docgen/islands/mermaid.js")
                .unwrap()
                .contents(),
        )
        .unwrap();
        assert!(js.contains("docgen.island"));
        assert!(js.contains("docgenMermaid"));
        assert!(js.contains("loadScript"));
        assert!(js.contains("/vendor/mermaid/mermaid.min.js"));
        assert!(!js.contains("import ")); // no ESM / npm
    }

    // ---- B-1: graph island contract ----

    #[test]
    fn graph_island_registers_and_renders_without_esm_or_d3() {
        let js = std::str::from_utf8(
            ASSETS
                .get_file("docgen/islands/graph.js")
                .unwrap()
                .contents(),
        )
        .unwrap();
        assert!(js.contains("docgen.island"));
        assert!(js.contains("docgenGraph"));
        assert!(js.contains("docgen-graph-data")); // reads the embedded JSON
        assert!(js.contains("http://www.w3.org/2000/svg")); // builds SVG via createElementNS
        assert!(!js.contains("import ")); // no ESM / npm
        assert!(!js.to_lowercase().contains("d3")); // no vendored graph lib
    }

    // ---- B-2: graph_assets() slice + include_graph gate ----

    #[test]
    fn graph_slice_has_island_and_is_gated() {
        let g = graph_assets();
        assert!(g.iter().any(|a| a.path == "islands/graph.js"));
        for a in &g {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }

        // off by default
        assert!(!assets_for(&EmitOptions::default())
            .iter()
            .any(|a| a.path == "islands/graph.js"));
        // on when flag set
        let full = assets_for(&EmitOptions {
            include_graph: true,
            ..Default::default()
        });
        assert!(full.iter().any(|a| a.path == "islands/graph.js"));
    }

    #[test]
    fn search_js_gated_by_include_search() {
        // On by default (pre-P6 behaviour).
        assert!(assets_for(&EmitOptions::default())
            .iter()
            .any(|a| a.path == "search.js"));
        // Dropped when search is disabled — no orphan asset in the dist.
        let off = assets_for(&EmitOptions {
            include_search: false,
            ..Default::default()
        });
        assert!(!off.iter().any(|a| a.path == "search.js"));
    }

    #[test]
    fn graph_island_is_js_kinded() {
        assert_eq!(
            graph_assets()
                .iter()
                .find(|a| a.path == "islands/graph.js")
                .unwrap()
                .kind,
            AssetKind::Js
        );
    }

    // ---- B-3: graph styles in shared css ----

    #[test]
    fn shared_css_has_graph_styles() {
        let s = std::str::from_utf8(
            core_assets()
                .iter()
                .find(|a| a.path == "docgen.css")
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert!(s.contains(".docgen-graph"));
        assert!(s.contains(".docgen-graph__nodes circle"));
        assert!(s.contains(".docgen-graph__links line"));
    }

    // ---- P5 A-6/B-1: dev_assets() slice, gated out of static emit ----

    #[test]
    fn dev_livereload_connects_to_sse_endpoint() {
        let reload = dev_assets()
            .into_iter()
            .find(|a| a.path == "__docgen/livereload.js")
            .expect("livereload client present");
        let js = std::str::from_utf8(reload.bytes).unwrap();
        assert!(js.contains("EventSource('/__docgen/livereload')"));
        assert!(js.contains("location.reload"));
    }

    #[test]
    fn dev_assets_has_codemirror_and_editor_and_reload() {
        let paths: Vec<&str> = dev_assets().iter().map(|a| a.path).collect();
        for p in [
            "__codemirror/codemirror.js",
            "__codemirror/codemirror.css",
            "__codemirror/markdown.js",
            "__codemirror/xml.js",
            "__codemirror/overlay.js",
            "__docgen/editor.js",
            "__docgen/editor.css",
            "__docgen/livereload.js",
        ] {
            assert!(paths.contains(&p), "dev_assets missing {p}");
        }
        for a in dev_assets() {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }
    }

    #[test]
    fn codemirror_is_umd_not_esm() {
        // Locks the no-bundler invariant: CM5 is classic UMD, never bare-ESM.
        let cm = dev_assets()
            .into_iter()
            .find(|a| a.path == "__codemirror/codemirror.js")
            .expect("codemirror.js present");
        let js = std::str::from_utf8(cm.bytes).unwrap();
        for line in js.lines() {
            assert!(
                !line.trim_start().starts_with("import "),
                "codemirror.js has a bare ESM import (needs a bundler): {line}"
            );
        }
    }

    #[test]
    fn editor_island_registers_without_esm() {
        let ed = dev_assets()
            .into_iter()
            .find(|a| a.path == "__docgen/editor.js")
            .expect("editor.js present");
        let js = std::str::from_utf8(ed.bytes).unwrap();
        assert!(js.contains("docgen.island"));
        assert!(js.contains("docgenEditor"));
        assert!(js.contains("/__docgen/source"));
        assert!(js.contains("window.CodeMirror"));
        assert!(!js.contains("import ")); // no ESM / npm
    }

    #[test]
    fn dev_assets_are_nonempty() {
        let d = dev_assets();
        assert!(!d.is_empty());
        for a in &d {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }
    }

    #[test]
    fn assets_for_never_includes_dev_assets() {
        let dev_paths: Vec<&str> = dev_assets().iter().map(|a| a.path).collect();
        // Every combination of the three EmitOptions flags.
        for k in [false, true] {
            for m in [false, true] {
                for g in [false, true] {
                    let opts = EmitOptions {
                        include_katex_runtime: k,
                        include_mermaid: m,
                        include_graph: g,
                        // Component bundles flow through emit_component_bundle, not
                        // assets_for; both flags are inert here but iterated so the
                        // exhaustive matrix stays honest.
                        include_component_css: k,
                        include_component_js: m,
                        include_search: g,
                    };
                    for a in assets_for(&opts) {
                        assert!(
                            !dev_paths.contains(&a.path),
                            "dev asset {} leaked into assets_for({opts:?})",
                            a.path
                        );
                        assert!(
                            !a.path.starts_with("__docgen/")
                                && !a.path.starts_with("__codemirror/"),
                            "dev-namespaced path {} leaked into assets_for",
                            a.path
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn emit_component_bundle_writes_when_nonempty_and_skips_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        emit_component_bundle(
            tmp.path(),
            ".docgen-callout{}",
            "Alpine.data('x',()=>({}))",
        )
        .unwrap();
        assert!(tmp.path().join("components.css").is_file());
        assert!(tmp.path().join("components.js").is_file());
        let tmp2 = tempfile::tempdir().unwrap();
        emit_component_bundle(tmp2.path(), "", "").unwrap();
        assert!(!tmp2.path().join("components.css").exists());
        assert!(!tmp2.path().join("components.js").exists());
    }

    // ---- A-6: emit() + assets_for() planner ----

    #[test]
    fn emit_writes_core_assets_to_disk() {
        let tmp =
            std::env::temp_dir().join(format!("docgen_assets_emit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        emit(&assets_for(&EmitOptions::default()), &tmp).unwrap();
        assert!(tmp.join("vendor/alpine/alpine.min.js").is_file());
        assert!(tmp.join("bootstrap.js").is_file());
        assert!(tmp.join("docgen.css").is_file());
        assert!(tmp.join("search.js").is_file());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ---- B-5: katex_css_assets() ----

    #[test]
    fn katex_css_and_all_16_fonts_present() {
        let a = katex_css_assets();
        assert!(a.iter().any(|x| x.path == "vendor/katex/katex.min.css"));
        let fonts = a.iter().filter(|x| x.kind == AssetKind::Font).count();
        assert_eq!(fonts, 16);
        for x in &a {
            assert!(!x.bytes.is_empty(), "{} empty", x.path);
        }
    }

    #[test]
    fn katex_css_url_paths_are_relative_to_fonts_dir() {
        let css = katex_css_assets()
            .into_iter()
            .find(|x| x.path.ends_with(".css"))
            .unwrap();
        let s = std::str::from_utf8(css.bytes).unwrap();
        assert!(s.contains("fonts/KaTeX_Main-Regular.woff2"));
    }

    #[test]
    fn default_build_always_emits_katex_css() {
        // Build-time math HTML always needs the css + fonts, even with no flags.
        let base = assets_for(&EmitOptions::default());
        assert!(base
            .iter()
            .any(|a| a.path == "vendor/katex/katex.min.css"));
        assert_eq!(
            base.iter().filter(|a| a.kind == AssetKind::Font).count(),
            16
        );
    }

    // ---- B-8: katex_runtime_assets() fallback ----

    #[test]
    fn katex_runtime_slice_has_both_scripts_but_is_off_by_default() {
        let rt = katex_runtime_assets();
        assert!(rt.iter().any(|a| a.path.ends_with("katex.min.js")));
        assert!(rt.iter().any(|a| a.path.ends_with("auto-render.min.js")));
        for a in &rt {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }
        assert!(!assets_for(&EmitOptions::default())
            .iter()
            .any(|a| a.path.ends_with("katex.min.js")));
    }

    #[test]
    fn katex_runtime_emitted_when_flag_set() {
        let full = assets_for(&EmitOptions {
            include_katex_runtime: true,
            ..Default::default()
        });
        assert!(full.iter().any(|a| a.path.ends_with("katex.min.js")));
        assert!(full.iter().any(|a| a.path.ends_with("auto-render.min.js")));
    }

    #[test]
    fn mermaid_slice_has_lib_and_island_and_is_gated() {
        let m = mermaid_assets();
        assert!(m.iter().any(|a| a.path.ends_with("mermaid.min.js")));
        assert!(m.iter().any(|a| a.path == "islands/mermaid.js"));
        for a in &m {
            assert!(!a.bytes.is_empty(), "{} empty", a.path);
        }
        let full = assets_for(&EmitOptions {
            include_mermaid: true,
            ..Default::default()
        });
        assert!(full.iter().any(|a| a.path == "islands/mermaid.js"));
        assert!(full.iter().any(|a| a.path.ends_with("mermaid.min.js")));
        assert!(!assets_for(&EmitOptions::default())
            .iter()
            .any(|a| a.path.contains("mermaid")));
    }

    #[test]
    fn planner_gates_mermaid_and_katex_runtime() {
        let base = assets_for(&EmitOptions::default());
        assert!(!base.iter().any(|a| a.path.contains("mermaid")));
        assert!(!base.iter().any(|a| a.path.ends_with("katex.min.js")));
        // With both flags on, the planner still must not include the stubbed
        // slices' files until B/C fill them — but it must not panic either.
        let _full = assets_for(&EmitOptions {
            include_katex_runtime: true,
            include_mermaid: true,
            include_graph: true,
            ..Default::default()
        });
    }
}
