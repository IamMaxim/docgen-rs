//! `docgen-diff` — the doc-diff timeline port.
//!
//! A pure-logic crate that owns the port of the original SvelteKit/TypeScript
//! doc-diff timeline (`~/work/docgen/packages/docgen/src/lib/diff`). It keeps
//! `libgit2` out of `docgen-core` (whose pipeline tests stay fast and
//! dependency-light) and mirrors the original's split between *pure diff
//! algorithms* (no git) and the *git-driven orchestrator*.

pub mod block_diff;
pub mod error;
pub mod file_tree;
pub mod git_parsing;
pub mod git_refs;
pub mod history;
pub mod line_diff;
pub mod payloads;
pub mod report;
pub mod timeline_groups;
pub mod types;

#[cfg(test)]
mod testutil;

pub use block_diff::{build_block_diff, split_markdown_blocks, strip_invisible_document_parts};
pub use error::DiffError;
pub use file_tree::build_file_tree;
pub use git_parsing::{parse_name_status, parse_untracked_docs, NameStatusEntry};
pub use git_refs::{base_ref_for_commit_parents, base_ref_for_parents, EMPTY_TREE_REF};
pub use history::{discover_repo, doc_revisions, CommitMeta, RevisionContent};
pub use line_diff::{build_line_hunks, build_line_hunks_default};
pub use payloads::{summarize_file, summarize_report, summarize_timeline_point};
pub use report::{build_doc_diff_report, build_doc_diff_report_with_blocks};
pub use timeline_groups::{bucket_label, format_date, group_timeline, ymd, TimelineBucket};
pub use types::*;
