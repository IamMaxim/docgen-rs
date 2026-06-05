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

    pub fn delete_file(&self, rel: &str) {
        std::fs::remove_file(self.dir.join(rel)).unwrap();
    }

    /// Create a branch at HEAD and check it out (working tree follows HEAD).
    pub fn checkout_new_branch(&self, name: &str) {
        let head = self.repo.head().unwrap().target().unwrap();
        let commit = self.repo.find_commit(head).unwrap();
        self.repo.branch(name, &commit, false).unwrap();
        self.checkout_branch(name);
    }

    /// Check out an existing branch, updating HEAD and the working tree.
    pub fn checkout_branch(&self, name: &str) {
        let refname = format!("refs/heads/{name}");
        let obj = self.repo.revparse_single(&refname).unwrap();
        let mut opts = git2::build::CheckoutBuilder::new();
        opts.force();
        self.repo.checkout_tree(&obj, Some(&mut opts)).unwrap();
        self.repo.set_head(&refname).unwrap();
    }

    /// Merge `branch` into the current HEAD with a `--no-ff` style merge commit
    /// (two parents: current HEAD then `branch`). Assumes a clean, conflict-free
    /// merge of trees; uses the branch tip's tree as the merge result, which is
    /// sufficient for hermetic doc-history tests.
    pub fn merge_no_ff(&self, branch: &str, subject: &str) -> String {
        let head_oid = self.repo.head().unwrap().target().unwrap();
        let head_commit = self.repo.find_commit(head_oid).unwrap();
        let branch_obj = self
            .repo
            .revparse_single(&format!("refs/heads/{branch}"))
            .unwrap();
        let branch_commit = branch_obj.peel_to_commit().unwrap();

        let mut idx = self
            .repo
            .merge_commits(&head_commit, &branch_commit, None)
            .unwrap();
        let tree_oid = idx.write_tree_to(&self.repo).unwrap();
        let tree = self.repo.find_tree(tree_oid).unwrap();
        let sig = Signature::now("docgen test", "test@example.com").unwrap();
        let oid = self
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                subject,
                &tree,
                &[&head_commit, &branch_commit],
            )
            .unwrap();
        // Sync the working tree/index to the new HEAD.
        let obj = self.repo.find_object(oid, None).unwrap();
        let mut opts = git2::build::CheckoutBuilder::new();
        opts.force();
        self.repo.checkout_tree(&obj, Some(&mut opts)).unwrap();
        oid.to_string()
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
