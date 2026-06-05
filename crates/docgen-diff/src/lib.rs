//! `docgen-diff` — the doc-diff timeline port.
//!
//! A pure-logic crate that owns the port of the original SvelteKit/TypeScript
//! doc-diff timeline (`~/work/docgen/packages/docgen/src/lib/diff`). It keeps
//! `libgit2` out of `docgen-core` (whose pipeline tests stay fast and
//! dependency-light) and mirrors the original's split between *pure diff
//! algorithms* (no git) and the *git-driven orchestrator*.

pub mod error;
pub mod types;

pub use error::DiffError;
pub use types::*;
