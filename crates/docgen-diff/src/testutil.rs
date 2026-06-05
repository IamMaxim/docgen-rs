//! Hermetic temp-git-repo builder shared by every git-layer test.
//!
//! Fully self-contained: configures a local committer identity so commits do
//! not depend on the host's global git config, and cleans up on drop.

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use git2::{Repository, Signature};

pub struct TempRepo {
    pub dir: PathBuf,
    pub repo: Repository,
}

impl TempRepo {
    /// Fresh empty repo in a unique temp dir with a local committer identity.
    pub fn init() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "docgen_diff_{}_{}",
            std::process::id(),
            unique_counter()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();
        // Local identity so commits don't need global git config (hermetic).
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "docgen test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        TempRepo { dir, repo }
    }

    /// Write `content` to `rel` (relative to repo root, creating parent dirs),
    /// stage everything, commit with `subject`. Returns the commit oid hex.
    pub fn commit_file(&self, rel: &str, content: &str, subject: &str) -> String {
        let abs = self.dir.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, content).unwrap();
        self.commit_all(subject)
    }

    /// Stage all changes (adds, modifies, deletes) and commit.
    pub fn commit_all(&self, subject: &str) -> String {
        let mut index = self.repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        // Pick up deletions.
        index.update_all(["*"].iter(), None).unwrap();
        index.write().unwrap();
        let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("docgen test", "test@example.com").unwrap();
        let parents = match self.repo.head().ok().and_then(|h| h.target()) {
            Some(oid) => vec![self.repo.find_commit(oid).unwrap()],
            None => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        let oid = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, subject, &tree, &parent_refs)
            .unwrap();
        oid.to_string()
    }

    /// git mv: rename a tracked file on disk (commit separately).
    pub fn rename_file(&self, from: &str, to: &str) {
        let to_abs = self.dir.join(to);
        std::fs::create_dir_all(to_abs.parent().unwrap()).unwrap();
        std::fs::rename(self.dir.join(from), to_abs).unwrap();
    }

    #[allow(dead_code)]
    pub fn delete_file(&self, rel: &str) {
        std::fs::remove_file(self.dir.join(rel)).unwrap();
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn unique_counter() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}
