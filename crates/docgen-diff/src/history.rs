//! Git history walk + blob content (git2 layer).
//!
//! Given a repo and a doc path (relative to repo root), list every commit that
//! touched it — newest-first — paired with the file content at that revision and
//! at its first parent. Rename-aware via diff `find_similar`, first-commit-safe,
//! no-history-safe.
//!
//! History selection mirrors `git log -- <path>`: the walk is over all
//! reachable commits but a merge commit that is TREESAME (same blob at the
//! tracked path) to one of its parents is simplified out, so a side-branch
//! change is not duplicated under the merge subject. Rename following uses
//! git2's `find_similar` with an explicit 50% similarity threshold (git's
//! default); a rename that also rewrites the body below that threshold is seen
//! as add+delete rather than a rename, so — like git without `--follow` — the
//! pre-rename history is not stitched across in that case. Copy detection is
//! off (parity with the original, which relied on `git log`'s default rename
//! following).

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

        // History simplification (parity with `git log -- <path>`): a merge
        // commit whose blob at the tracked path is identical (TREESAME) to one
        // of its parents brought no change of its own on this path — the real
        // authoring commit lives on that parent's history and is walked
        // separately. Emitting the merge too would duplicate that change.
        // Skip it (but still follow the rest of the walk).
        if commit.parent_count() > 1
            && merge_is_treesame_to_a_parent(&commit, &commit_tree, &current_path)?
        {
            continue;
        }

        // Full tree diff with rename detection — we cannot pre-restrict the
        // pathspec because a rename changes the path.
        let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)?;
        let mut find_opts = DiffFindOptions::new();
        find_opts.renames(true);
        // Pin git's default 50% similarity threshold so rename detection does
        // not drift with git2's internal default (documented behavior above).
        find_opts.rename_threshold(50);
        diff.find_similar(Some(&mut find_opts))?;

        // Find a delta touching the currently-tracked path (as new or old side).
        let mut matched: Option<(DocDiffFileStatus, String, Option<String>)> = None;
        for delta in diff.deltas() {
            let new_path = delta.new_file().path().and_then(|p| p.to_str());
            let old_path = delta.old_file().path().and_then(|p| p.to_str());

            let touches = new_path == Some(current_path.as_str());
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
            meta: commit_meta(&commit)?,
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

/// One changed doc file within a single commit (the `new` side, plus its
/// `old` content from the first parent). The global analogue of the per-file
/// [`RevisionContent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFileChange {
    pub status: DocDiffFileStatus,
    /// Repo-relative path of the doc at this revision (the `new` side).
    pub path: String,
    /// Set when this file was renamed (the `old` side path).
    pub old_path: Option<String>,
    pub old_text: String,
    pub new_text: String,
}

/// One commit in the global doc timeline: its metadata plus every doc file it
/// changed (under `docs_prefix`), diffed against its first parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRevision {
    pub meta: CommitMeta,
    pub files: Vec<GlobalFileChange>,
}

/// True when `path` is `prefix` itself or lives beneath it (`prefix/...`).
fn under_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// All commits (newest-first, capped at `limit` doc-touching commits) that
/// changed any file under `docs_prefix` (repo-relative, e.g. `"docs"`), each
/// paired with the per-file old/new content from its first parent. The global
/// analogue of [`doc_revisions`] — mirrors `git log -- <docsPath>` plus the
/// per-commit `git diff` in the original `git-diff.server.ts`.
pub fn global_doc_revisions(
    repo: &Repository,
    docs_prefix: &str,
    limit: usize,
) -> Result<Vec<GlobalRevision>, DiffError> {
    let mut revwalk = repo.revwalk()?;
    revwalk.set_sorting(Sort::TIME | Sort::TOPOLOGICAL)?;
    if revwalk.push_head().is_err() {
        return Ok(Vec::new());
    }

    let mut out: Vec<GlobalRevision> = Vec::new();

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

        // Merge history simplification (parity with `git log -- <docsPath>`): a
        // merge whose docs subtree is identical to a parent's brought no change
        // of its own under the docs prefix — its authoring commits are walked
        // separately. Skip it to avoid duplicating those changes.
        if commit.parent_count() > 1
            && merge_docs_treesame_to_a_parent(&commit, &commit_tree, docs_prefix)?
        {
            continue;
        }

        let mut diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&commit_tree), None)?;
        let mut find_opts = DiffFindOptions::new();
        find_opts.renames(true);
        find_opts.rename_threshold(50);
        diff.find_similar(Some(&mut find_opts))?;

        let mut files: Vec<GlobalFileChange> = Vec::new();
        for delta in diff.deltas() {
            let new_path = delta.new_file().path().and_then(|p| p.to_str());
            let old_path = delta.old_file().path().and_then(|p| p.to_str());
            let status = classify(delta.status());

            // Resolve the doc path on the side that exists, then filter to docs.
            let touch_path = if status == DocDiffFileStatus::Deleted {
                old_path
            } else {
                new_path
            };
            let Some(touch_path) = touch_path else { continue };
            if !under_prefix(touch_path, docs_prefix) {
                continue;
            }

            let resolved_new = new_path.unwrap_or(touch_path).to_string();
            let resolved_old = old_path.map(|s| s.to_string());

            let new_text = if status == DocDiffFileStatus::Deleted {
                String::new()
            } else {
                blob_text(repo, &commit_tree, &resolved_new)
            };
            let old_text = match (&parent_tree, status) {
                (_, DocDiffFileStatus::Added) => String::new(),
                (Some(pt), _) => {
                    let read_path = resolved_old.as_deref().unwrap_or(resolved_new.as_str());
                    blob_text(repo, pt, read_path)
                }
                (None, _) => String::new(),
            };
            let rename_old_path = if status == DocDiffFileStatus::Renamed {
                resolved_old.clone()
            } else {
                None
            };

            files.push(GlobalFileChange {
                status,
                path: resolved_new,
                old_path: rename_old_path,
                old_text,
                new_text,
            });
        }

        if files.is_empty() {
            // Commit touched no docs files — skip (parity with the TS
            // `entries.length === 0 -> continue`).
            continue;
        }

        // Stable, deterministic order by new-side path.
        files.sort_by(|a, b| a.path.cmp(&b.path));

        out.push(GlobalRevision {
            meta: commit_meta(&commit)?,
            files,
        });
    }

    Ok(out)
}

/// True when the merge `commit`'s tree entry at `docs_prefix` is identical to
/// that of at least one parent (TREESAME on the docs subtree).
fn merge_docs_treesame_to_a_parent(
    commit: &git2::Commit,
    commit_tree: &Tree,
    docs_prefix: &str,
) -> Result<bool, DiffError> {
    let merge_oid = blob_oid_at(commit_tree, docs_prefix);
    for parent in commit.parents() {
        let parent_tree = parent.tree()?;
        if blob_oid_at(&parent_tree, docs_prefix) == merge_oid {
            return Ok(true);
        }
    }
    Ok(false)
}

/// True when the merge `commit`'s blob at `path` is identical to that path's
/// blob in at least one parent (TREESAME). Mirrors git-log's merge history
/// simplification: such a merge contributed no change of its own on `path`.
fn merge_is_treesame_to_a_parent(
    commit: &git2::Commit,
    commit_tree: &Tree,
    path: &str,
) -> Result<bool, DiffError> {
    let merge_oid = blob_oid_at(commit_tree, path);
    for parent in commit.parents() {
        let parent_tree = parent.tree()?;
        if blob_oid_at(&parent_tree, path) == merge_oid {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The blob oid at `path` within `tree`, or `None` when the path is absent or
/// is not a blob. Two trees are TREESAME on `path` iff these match.
fn blob_oid_at(tree: &Tree, path: &str) -> Option<git2::Oid> {
    tree.get_path(Path::new(path)).ok().map(|e| e.id())
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

fn commit_meta(commit: &git2::Commit) -> Result<CommitMeta, DiffError> {
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
    fn doc_revisions_rename_with_heavy_edit_is_add_delete_not_followed() {
        // A rename that also rewrites the body below the 50% threshold is seen
        // as Added(new) rather than Renamed; the pre-rename history is not
        // stitched across (documented non-`--follow` behavior). This pins the
        // explicit threshold so the classification cannot silently drift.
        let r = TempRepo::init();
        r.commit_file(
            "docs/old.md",
            "alpha beta gamma\ndelta epsilon zeta\neta theta iota\n",
            "create old",
        );
        r.rename_file("docs/old.md", "docs/new.md");
        // Replace the body entirely so similarity is far below 50%.
        std::fs::write(
            r.dir.join("docs/new.md"),
            "completely different content here\nnothing in common at all\nbrand new words only\n",
        )
        .unwrap();
        r.commit_all("rename and rewrite");

        let revs = doc_revisions(&r.repo, "docs/new.md", 50).unwrap();
        // Not classified as a rename, so old.md history is not followed.
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].status, DocDiffFileStatus::Added);
        assert_eq!(revs[0].old_path, None);
        assert_eq!(revs[0].old_text, "");
    }

    #[test]
    fn doc_revisions_empty_for_untouched_path() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "x\n", "a");
        assert!(doc_revisions(&r.repo, "docs/ghost.md", 50)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn doc_revisions_empty_for_initialized_repo_with_no_commits() {
        // Initialized but no HEAD: exercises the push_head().is_err() branch.
        let r = TempRepo::init();
        assert!(doc_revisions(&r.repo, "docs/a.md", 50).unwrap().is_empty());
    }

    #[test]
    fn doc_revisions_records_deletion() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\nbody\n", "add a");
        r.delete_file("docs/a.md");
        r.commit_all("remove a");

        let revs = doc_revisions(&r.repo, "docs/a.md", 50).unwrap();
        // Newest revision is the deletion.
        assert_eq!(revs[0].meta.subject, "remove a");
        assert_eq!(revs[0].status, DocDiffFileStatus::Deleted);
        assert_eq!(revs[0].new_text, "");
        assert_eq!(revs[0].old_text, "# A\nbody\n");
    }

    #[test]
    fn doc_revisions_respects_limit() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "1\n", "edit 1");
        r.commit_file("docs/a.md", "2\n", "edit 2");
        r.commit_file("docs/a.md", "3\n", "edit 3");

        let revs = doc_revisions(&r.repo, "docs/a.md", 2).unwrap();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].meta.subject, "edit 3");
        assert_eq!(revs[1].meta.subject, "edit 2");
    }

    #[test]
    fn doc_revisions_skips_merge_commit_treesame_to_a_parent() {
        // Reproduces `git log -- docs/a.md` history simplification: a merge that
        // brings a side-branch change in (TREESAME to the side parent) must NOT
        // appear as a duplicate revision alongside the real authoring commit.
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\nl1\n", "base");

        r.checkout_new_branch("feature");
        r.commit_file("docs/a.md", "# A\nl1\nfeatureline\n", "feature edit");

        r.checkout_branch("master");
        r.commit_file("docs/b.txt", "b\n", "unrelated b");
        r.merge_no_ff("feature", "merge feature");

        let revs = doc_revisions(&r.repo, "docs/a.md", 50).unwrap();
        let subjects: Vec<&str> = revs.iter().map(|x| x.meta.subject.as_str()).collect();
        // Only the real authoring commit + base — the merge is simplified out.
        assert_eq!(subjects, vec!["feature edit", "base"]);
    }
}
