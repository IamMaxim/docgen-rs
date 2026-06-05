//! `docgen-assets` owns every vendored and authored frontend file used by the
//! generated doc site. Files are embedded at compile time via `include_dir!`,
//! and exposed through a typed enumerate/emit API. The build subcommand drives
//! emission via [`assets_for`] + [`emit`].

use include_dir::{include_dir, Dir};
use std::path::Path;

/// Every vendored + authored frontend file, embedded at compile time.
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

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

/// Flags driving which conditional asset slices a build emits.
///
/// Both fields default to `false`: the default build takes the build-time KaTeX
/// path (no runtime JS) and emits mermaid only when a page used a diagram.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmitOptions {
    /// Ship runtime `katex.min.js` + `auto-render.min.js` (fallback path). Default false.
    pub include_katex_runtime: bool,
    /// Ship `mermaid.min.js` + the mermaid island (only when a page used a diagram).
    pub include_mermaid: bool,
    /// Ship the graph island (`islands/graph.js`) — emitted when the `/graph/`
    /// page is rendered. Default false.
    pub include_graph: bool,
}

/// The full asset set to emit for this build: core + katex css (always, for math
/// output) + conditional runtime/mermaid. Stable ordering for deterministic tests.
pub fn assets_for(opts: &EmitOptions) -> Vec<Asset> {
    let mut out = core_assets();
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
        });
    }
}
