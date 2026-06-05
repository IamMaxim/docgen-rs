//! `docgen-diff` — the doc-diff timeline port.
//!
//! A pure-logic crate that owns the port of the original SvelteKit/TypeScript
//! doc-diff timeline (`~/work/docgen/packages/docgen/src/lib/diff`). It keeps
//! `libgit2` out of `docgen-core` (whose pipeline tests stay fast and
//! dependency-light) and mirrors the original's split between *pure diff
//! algorithms* (no git) and the *git-driven orchestrator*.

pub mod error;
pub mod git_parsing;
pub mod git_refs;
pub mod history;
pub mod line_diff;
pub mod types;

#[cfg(test)]
mod testutil;

pub use error::DiffError;
pub use git_parsing::{parse_name_status, parse_untracked_docs, NameStatusEntry};
pub use git_refs::{base_ref_for_commit_parents, EMPTY_TREE_REF};
pub use history::{discover_repo, doc_revisions, CommitMeta, RevisionContent};
pub use line_diff::{build_line_hunks, build_line_hunks_default};
pub use types::*;
