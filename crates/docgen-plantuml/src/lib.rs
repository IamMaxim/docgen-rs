//! Build-time PlantUML rendering for docgen: the networked implementation of
//! `docgen-core`'s [`docgen_core::PlantumlRenderer`] trait (encoding + HTTP +
//! on-disk cache), plus the `docgen plantuml` ephemeral-server container command.
//!
//! docgen-core owns the trait and the pure directive glue; this crate owns
//! everything that touches the network, the filesystem cache, or a container
//! runtime — mirroring how `docgen-s3` isolates S3 I/O from the pure crates.

pub mod container;
pub mod encode;
pub mod render;

pub use container::{run_container, PLANTUML_IMAGE};
pub use encode::encode;
pub use render::HttpRenderer;
