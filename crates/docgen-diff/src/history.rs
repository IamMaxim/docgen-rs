//! Git history walk + blob content (git2 layer).
//!
//! Given a repo and a doc path (relative to repo root), list every commit that
//! touched it — newest-first — paired with the file content at that revision and
//! at its first parent. Rename-aware via diff `find_similar`, first-commit-safe,
//! no-history-safe. This reproduces `git log --follow -- <path>` semantics:
//! only first-parent rename chains are followed; copy detection is off (parity
//! with the original, which relied on `git log`'s default rename following).

use std::path::Path;

use git2::{Delta, DiffFindOptions, ErrorCode, Repository, Sort, Tree};

use crate::error::DiffError;
use crate::types::DocDiffFileStatus;

/// Commit metadata captured for a revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitMeta {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: Option<String>,
    /// RFC3339 string, parity with the TS ISO date.
    pub date: Option<String>,
    pub subject: String,
}

/// One revision of a doc: its commit metadata plus the file content at this
/// revision (`new_text`) and at its first parent (`old_text`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionContent {
    pub meta: CommitMeta,
    pub old_text: String,
    pub new_text: String,
    pub status: DocDiffFileStatus,
    /// Path of the doc at this revision (the `new` side).
    pub path: String,
    /// Set when this revision renamed the doc (the `old` side path).
    pub old_path: Option<String>,
}

/// Open the repo that contains `path` (walking up). Returns `Ok(None)` when
/// `path` is not inside any git repo — the graceful "not a git repo" path.
pub fn discover_repo(path: &Path) -> Result<Option<Repository>, DiffError> {
    match Repository::discover(path) {
        Ok(repo) => Ok(Some(repo)),
        Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// All commits (newest-first, capped at `limit`) that touched `doc_rel_path`,
/// each paired with the file content at that revision and at its first parent.
pub fn doc_revisions(
    repo: &Repository,
    doc_rel_path: &str,
    limit: usize,
) -> Result<Vec<RevisionContent>, DiffError> {
    let mut revwalk = repo.revwalk()?;
    // Newest-first, parity with `git log` default for `recentDocCommits`.
    revwalk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)?;
    if revwalk.push_head().is_err() {
        // Empty repo (no HEAD): no history.
        return Ok(Vec::new());
    }

    // The path moves backward in time as we follow renames.
    let mut current_path = doc_rel_path.to_string();
    let mut out = Vec::new();

    for oid in revwalk {
        if out.len() >= limit {
            break;
        }
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let commit_tree = commit.tree()?;
        let parent = commit.parents().next();
        let parent_tree: Option<Tree> = match &parent {
            Some(p) => Some(p.tree()?),
            None => None,
        };

        // Full tree diff with rename detection — we cannot pre-restrict the
        // pathspec because a rename changes the path.
        let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)?;
        let mut find_opts = DiffFindOptions::new();
        find_opts.renames(true);
        diff.find_similar(Some(&mut find_opts))?;

        // Find a delta touching the currently-tracked path (as new or old side).
        let mut matched: Option<(DocDiffFileStatus, String, Option<String>)> = None;
        for delta in diff.deltas() {
            let new_path = delta.new_file().path().and_then(|p| p.to_str());
            let old_path = delta.old_file().path().and_then(|p| p.to_str());

            let touches = new_path == Some(current_path.as_str())
                || (delta.status() == Delta::Renamed && new_path == Some(current_path.as_str()));
            // For deletions the path lives on the old side.
            let touches_deleted =
                delta.status() == Delta::Deleted && old_path == Some(current_path.as_str());

            if touches || touches_deleted {
                let status = classify(delta.status());
                let resolved_new = new_path.unwrap_or(current_path.as_str()).to_string();
                let resolved_old = old_path.map(|s| s.to_string());
                matched = Some((status, resolved_new, resolved_old));
                break;
            }
        }

        let (status, new_path, old_path) = match matched {
            Some(m) => m,
            // This commit did not touch the tracked path — skip (parity with
            // the TS `entries.length === 0 -> continue`).
            None => continue,
        };

        // Read content. `new_text` from this commit's tree; `old_text` from the
        // parent tree (empty when parentless or Added).
        let new_text = if status == DocDiffFileStatus::Deleted {
            String::new()
        } else {
            blob_text(repo, &commit_tree, &new_path)
        };
        let old_text = match (&parent_tree, status) {
            (_, DocDiffFileStatus::Added) => String::new(),
            (Some(pt), _) => {
                let read_path = old_path.as_deref().unwrap_or(new_path.as_str());
                blob_text(repo, pt, read_path)
            }
            (None, _) => String::new(),
        };

        let rename_old_path = if status == DocDiffFileStatus::Renamed {
            old_path.clone()
        } else {
            None
        };

        out.push(RevisionContent {
            meta: commit_meta(repo, &commit)?,
            old_text,
            new_text,
            status,
            path: new_path,
            old_path: rename_old_path,
        });

        // Follow the rename backward: older commits know this doc by its old path.
        if status == DocDiffFileStatus::Renamed {
            if let Some(op) = old_path {
                current_path = op;
            }
        }
    }

    Ok(out)
}

fn classify(delta: Delta) -> DocDiffFileStatus {
    match delta {
        Delta::Added => DocDiffFileStatus::Added,
        Delta::Deleted => DocDiffFileStatus::Deleted,
        Delta::Renamed => DocDiffFileStatus::Renamed,
        // Modified, Copied, Typechange, etc. → Modified (parity default).
        _ => DocDiffFileStatus::Modified,
    }
}

/// Read a blob at `path` within `tree`, lossily decoding to UTF-8. Returns ""
/// when the path is absent or is not a blob.
fn blob_text(repo: &Repository, tree: &Tree, path: &str) -> String {
    let entry = match tree.get_path(Path::new(path)) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    let object = match entry.to_object(repo) {
        Ok(o) => o,
        Err(_) => return String::new(),
    };
    match object.as_blob() {
        Some(blob) => String::from_utf8_lossy(blob.content()).into_owned(),
        None => String::new(),
    }
}

fn commit_meta(repo: &Repository, commit: &git2::Commit) -> Result<CommitMeta, DiffError> {
    let hash = commit.id().to_string();
    let short_hash = commit
        .as_object()
        .short_id()
        .ok()
        .and_then(|buf| buf.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| hash.chars().take(7).collect());
    let parents = commit.parent_ids().map(|oid| oid.to_string()).collect();
    let author = commit.author().name().and_then(|n| {
        if n.is_empty() {
            None
        } else {
            Some(n.to_string())
        }
    });
    let date = rfc3339(commit.time());
    let subject = commit
        .message()
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    let _ = repo;
    Ok(CommitMeta {
        hash,
        short_hash,
        parents,
        author,
        date,
        subject,
    })
}

/// Convert a git2 commit time (epoch seconds + tz offset minutes) to an RFC3339
/// string. Returns `None` if the time is out of range.
fn rfc3339(time: git2::Time) -> Option<String> {
    use chrono::{FixedOffset, TimeZone};
    let offset = FixedOffset::east_opt(time.offset_minutes() * 60)?;
    offset
        .timestamp_opt(time.seconds(), 0)
        .single()
        .map(|dt| dt.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;

    #[test]
    fn discover_repo_returns_none_outside_git() {
        let dir = std::env::temp_dir().join(format!("docgen_nogit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(discover_repo(&dir).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_repo_finds_temp_repo() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "x\n", "a");
        assert!(discover_repo(&r.dir).unwrap().is_some());
    }

    #[test]
    fn doc_revisions_lists_commits_newest_first_with_content() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\nfirst\n", "add a");
        r.commit_file("docs/a.md", "# A\nsecond\n", "edit a");
        r.commit_file("docs/other.md", "x\n", "unrelated");

        let revs = doc_revisions(&r.repo, "docs/a.md", 50).unwrap();
        assert_eq!(revs.len(), 2);
        // Newest-first.
        assert_eq!(revs[0].meta.subject, "edit a");
        assert_eq!(revs[1].meta.subject, "add a");
        // Content at each revision + its parent.
        assert_eq!(revs[0].new_text, "# A\nsecond\n");
        assert_eq!(revs[0].old_text, "# A\nfirst\n");
        assert_eq!(revs[0].status, DocDiffFileStatus::Modified);
        // First commit: parentless -> empty old_text, status Added.
        assert_eq!(revs[1].old_text, "");
        assert_eq!(revs[1].new_text, "# A\nfirst\n");
        assert_eq!(revs[1].status, DocDiffFileStatus::Added);
        // short_hash is a prefix of hash; parents recorded.
        assert!(revs[0].meta.hash.starts_with(&revs[0].meta.short_hash));
        assert_eq!(revs[1].meta.parents.len(), 0);
        assert_eq!(revs[0].meta.parents.len(), 1);
        // Metadata populated.
        assert_eq!(revs[0].meta.author.as_deref(), Some("docgen test"));
        assert!(revs[0].meta.date.is_some());
    }

    #[test]
    fn doc_revisions_follows_a_rename() {
        let r = TempRepo::init();
        r.commit_file("docs/old.md", "# Doc\nbody line\nmore\n", "create");
        r.rename_file("docs/old.md", "docs/new.md");
        r.commit_all("rename");

        let revs = doc_revisions(&r.repo, "docs/new.md", 50).unwrap();
        assert!(revs
            .iter()
            .any(|rev| rev.status == DocDiffFileStatus::Renamed
                && rev.old_path.as_deref() == Some("docs/old.md")));
        // The create commit is reachable through the rename (old_path history).
        assert!(revs.iter().any(|rev| rev.meta.subject == "create"));
    }

    #[test]
    fn doc_revisions_empty_for_untouched_path() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "x\n", "a");
        assert!(doc_revisions(&r.repo, "docs/ghost.md", 50)
            .unwrap()
            .is_empty());
    }
}
