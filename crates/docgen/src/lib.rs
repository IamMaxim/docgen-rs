//! # docgen — a Cargo-only static documentation-site generator
//!
//! **No npm, no Node.** `docgen` turns a `docs/` tree of Markdown files into a
//! fast, fully static site: server-side syntax highlighting, `[[wikilinks]]`
//! with backlinks, a zero-JS-build search index, an interactive git-history
//! timeline, a knowledge graph, and Obsidian-style `.base` views — all rendered
//! ahead of time.
//!
//! Most users want the **command-line tool**, not this library:
//!
//! ```sh
//! cargo install docgen-rs      # installs the `docgen` binary
//! docgen init my-docs
//! docgen dev my-docs           # live-reload preview
//! docgen build my-docs         # static site in ./dist
//! ```
//!
//! The full user guide lives at <https://iammaxim.github.io/docgen-rs/>.
//!
//! ## Using docgen as a library
//!
//! `docgen-rs` is a thin CLI over a Cargo workspace of focused library crates.
//! This crate re-exports them so the whole public API is browsable from one
//! place; depend on the individual crates directly for a leaner build.
//!
//! | Re-export | Crate | Responsibility |
//! |-----------|-------|----------------|
//! | [`core`] | `docgen-core` | Document model, discovery, sidebar tree, pipeline |
//! | [`render`] | `docgen-render` | HTML rendering (pages, graph, history, diff) |
//! | [`build`] | `docgen-build` | The `build` orchestration that ties it together |
//! | [`server`] | `docgen-server` | The `dev` live-reload server |
//! | [`diff`] | `docgen-diff` | Git-history diffing (the `/diff/` timeline) |
//! | [`assets`] | `docgen-assets` | Vendored CSS/JS and static assets |
//! | [`init`] | `docgen-init` | Site scaffolding (`docgen init`) |
//! | [`lint`] | `docgen-lint` | Pre-publish link/asset/frontmatter checks |
//! | [`plantuml`] | `docgen-plantuml` | Build-time PlantUML diagram rendering |

/// Vendored CSS/JS and static assets. See [`docgen_assets`].
pub use docgen_assets as assets;
/// Build orchestration (`docgen build`). See [`docgen_build`].
pub use docgen_build as build;
/// Document model, discovery, sidebar tree, and pipeline. See [`docgen_core`].
pub use docgen_core as core;
/// Git-history diffing behind the `/diff/` timeline. See [`docgen_diff`].
pub use docgen_diff as diff;
/// Site scaffolding (`docgen init`). See [`docgen_init`].
pub use docgen_init as init;
/// Pre-publish link/asset/frontmatter checks (`docgen lint`). See [`docgen_lint`].
pub use docgen_lint as lint;
/// Build-time PlantUML diagram rendering. See [`docgen_plantuml`].
pub use docgen_plantuml as plantuml;
/// HTML rendering of pages, graph, history, and diff. See [`docgen_render`].
pub use docgen_render as render;
/// The `dev` live-reload server. See [`docgen_server`].
pub use docgen_server as server;
