//! Error type for the git-driven layer.

/// Errors raised while walking git history or reading blob content.
#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("git error: {0}")]
    Git(#[from] git2::Error),
}
