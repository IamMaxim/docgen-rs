//! Build-history orchestrator — the Rust analogue of `git-diff.server.ts`'s
//! `loadDocDiffTimelineReport` + `buildTimelinePoint` + `reportFromTimeline`,
//! restricted to one doc and build-history mode.
//!
//! Ties the `history` git walk (Cluster A) to the pure diff/grouping layer
//! (Cluster B): each revision becomes a `DocDiffTimelinePoint` carrying the
//! doc's line hunks and rendered block diff; the report selects the newest
//! point as its head.

use chrono::Local;
use docgen_core::markdown::render_markdown;

use crate::error::DiffError;
use crate::types::{
    DocDiffBlockKind, DocDiffFile, DocDiffLineKind, DocDiffReport, DocDiffTimelinePoint,
    DocDiffTimelinePointKind,
};
use crate::{block_diff, file_tree, git_refs, history, line_diff};

/// Build the build-history report for one doc (`doc_rel_path` is relative to
/// the repo root, e.g. `"docs/guide/intro.md"`). Returns `Ok(None)` when the
/// doc has no commit history (graceful no-op).
pub fn build_doc_diff_report(
    repo: &git2::Repository,
    doc_rel_path: &str,
    limit: usize,
) -> Result<Option<DocDiffReport>, DiffError> {
    build_doc_diff_report_inner(repo, doc_rel_path, limit, false)
}

/// Like [`build_doc_diff_report`] but attaching the rendered block diff to each
/// file. Used by consumers that actually render blocks; the CLI build/history
/// path does not (it projects only hunks), so it uses the cheaper default.
pub fn build_doc_diff_report_with_blocks(
    repo: &git2::Repository,
    doc_rel_path: &str,
    limit: usize,
) -> Result<Option<DocDiffReport>, DiffError> {
    build_doc_diff_report_inner(repo, doc_rel_path, limit, true)
}

fn build_doc_diff_report_inner(
    repo: &git2::Repository,
    doc_rel_path: &str,
    limit: usize,
    with_blocks: bool,
) -> Result<Option<DocDiffReport>, DiffError> {
    let revs = history::doc_revisions(repo, doc_rel_path, limit)?;
    if revs.is_empty() {
        return Ok(None);
    }

    let mut timeline: Vec<DocDiffTimelinePoint> = Vec::with_capacity(revs.len());

    for rev in revs {
        let hunks = line_diff::build_line_hunks_default(&rev.old_text, &rev.new_text);

        // Build the block grouping (cheap text grouping) to reproduce the TS
        // "no visible change -> return null" skip decision. Rendering each
        // block to HTML is only done when a consumer wants the blocks — it is
        // pure waste in the build/history path, which projects only hunks.
        let mut blocks = block_diff::build_block_diff(&rev.old_text, &rev.new_text);
        let all_context = blocks.iter().all(|b| b.kind == DocDiffBlockKind::Context);
        let blocks = if with_blocks {
            for block in &mut blocks {
                block.html = render_markdown(&block.raw);
            }
            Some(blocks)
        } else {
            None
        };

        // Skip the file when there is no visible change (parity with the TS
        // `return null`). In build-history each point is a single doc file.
        let files: Vec<DocDiffFile> = if hunks.is_empty() && all_context {
            vec![]
        } else {
            let added_lines = count_lines(&hunks, DocDiffLineKind::Added);
            let removed_lines = count_lines(&hunks, DocDiffLineKind::Removed);
            vec![DocDiffFile {
                path: rev.path.clone(),
                old_path: rev.old_path.clone(),
                status: rev.status,
                added_lines,
                removed_lines,
                hunks,
                blocks,
            }]
        };

        let file_tree = file_tree::build_file_tree(&files);
        let total_added_lines = files.iter().map(|f| f.added_lines).sum();
        let total_removed_lines = files.iter().map(|f| f.removed_lines).sum();
        let base_ref = git_refs::base_ref_for_parents(&rev.meta.parents);
        let head_ref = rev.meta.hash.clone();

        timeline.push(DocDiffTimelinePoint {
            id: rev.meta.hash.clone(),
            kind: DocDiffTimelinePointKind::Commit,
            hash: Some(rev.meta.hash.clone()),
            short_hash: rev.meta.short_hash.clone(),
            subject: rev.meta.subject.clone(),
            author: rev.meta.author.clone(),
            date: rev.meta.date.clone(),
            base_ref,
            head_ref,
            files,
            file_tree,
            total_added_lines,
            total_removed_lines,
            warnings: vec![],
        });
    }

    Ok(Some(report_from_timeline(timeline)))
}

/// Build the global build-history report across all docs under `docs_prefix`
/// (repo-relative, e.g. `"docs"`). Each timeline point is a commit carrying
/// *every* doc file it changed — the analogue of the original global
/// `/docs/diff` report. Returns `Ok(None)` when no commit touched the docs.
/// With `with_blocks`, each file additionally carries its rendered block diff.
pub fn build_global_doc_diff_report(
    repo: &git2::Repository,
    docs_prefix: &str,
    limit: usize,
    with_blocks: bool,
) -> Result<Option<DocDiffReport>, DiffError> {
    let revs = history::global_doc_revisions(repo, docs_prefix, limit)?;
    if revs.is_empty() {
        return Ok(None);
    }

    let mut timeline: Vec<DocDiffTimelinePoint> = Vec::with_capacity(revs.len());

    for rev in revs {
        let mut files: Vec<DocDiffFile> = Vec::new();
        for change in &rev.files {
            let hunks = line_diff::build_line_hunks_default(&change.old_text, &change.new_text);
            let mut blocks = block_diff::build_block_diff(&change.old_text, &change.new_text);
            let all_context = blocks.iter().all(|b| b.kind == DocDiffBlockKind::Context);

            // Parity with the TS `return null`: a file whose only change is
            // invisible (no hunks, all-context blocks) is dropped.
            if hunks.is_empty() && all_context {
                continue;
            }

            let blocks = if with_blocks {
                for block in &mut blocks {
                    block.html = render_markdown(&block.raw);
                }
                Some(blocks)
            } else {
                None
            };

            files.push(DocDiffFile {
                path: change.path.clone(),
                old_path: change.old_path.clone(),
                status: change.status,
                added_lines: count_lines(&hunks, DocDiffLineKind::Added),
                removed_lines: count_lines(&hunks, DocDiffLineKind::Removed),
                hunks,
                blocks,
            });
        }

        // A commit whose docs changes were all invisible yields no files — skip.
        if files.is_empty() {
            continue;
        }

        let file_tree = file_tree::build_file_tree(&files);
        let total_added_lines = files.iter().map(|f| f.added_lines).sum();
        let total_removed_lines = files.iter().map(|f| f.removed_lines).sum();
        let base_ref = git_refs::base_ref_for_parents(&rev.meta.parents);
        let head_ref = rev.meta.hash.clone();

        timeline.push(DocDiffTimelinePoint {
            id: rev.meta.hash.clone(),
            kind: DocDiffTimelinePointKind::Commit,
            hash: Some(rev.meta.hash.clone()),
            short_hash: rev.meta.short_hash.clone(),
            subject: rev.meta.subject.clone(),
            author: rev.meta.author.clone(),
            date: rev.meta.date.clone(),
            base_ref,
            head_ref,
            files,
            file_tree,
            total_added_lines,
            total_removed_lines,
            warnings: vec![],
        });
    }

    if timeline.is_empty() {
        return Ok(None);
    }

    Ok(Some(report_from_timeline(timeline)))
}

fn count_lines(hunks: &[crate::types::DocDiffHunk], kind: DocDiffLineKind) -> u32 {
    hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.kind == kind)
        .count() as u32
}

fn report_from_timeline(timeline: Vec<DocDiffTimelinePoint>) -> DocDiffReport {
    let selected_point = timeline.first();
    let selected_file = selected_point.and_then(|p| p.files.first());

    let base_ref = selected_point
        .map(|p| p.base_ref.clone())
        .unwrap_or_else(|| git_refs::EMPTY_TREE_REF.to_string());
    let head_ref = selected_point
        .map(|p| p.head_ref.clone())
        .unwrap_or_else(|| "HEAD".to_string());
    let selected_point_id = selected_point.map(|p| p.id.clone());
    let selected_file_path = selected_file.map(|f| f.path.clone());
    let files = selected_point.map(|p| p.files.clone()).unwrap_or_default();
    let total_added_lines = selected_point.map(|p| p.total_added_lines).unwrap_or(0);
    let total_removed_lines = selected_point.map(|p| p.total_removed_lines).unwrap_or(0);

    DocDiffReport {
        mode: "build-history".into(),
        base_ref,
        head_ref,
        generated_at: Local::now().to_rfc3339(),
        timeline,
        selected_point_id,
        selected_file_path,
        files,
        total_added_lines,
        total_removed_lines,
        warnings: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempRepo;
    use crate::types::DocDiffFileStatus;

    #[test]
    fn report_builds_timeline_for_a_doc_with_hunks_and_blocks() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\n\nfirst paragraph.\n", "add a");
        r.commit_file("docs/a.md", "# A\n\nsecond paragraph.\n", "edit a");

        let report = build_doc_diff_report_with_blocks(&r.repo, "docs/a.md", 50)
            .unwrap()
            .unwrap();
        assert_eq!(report.mode, "build-history");
        assert_eq!(report.timeline.len(), 2);

        let head = &report.timeline[0];
        assert_eq!(head.subject, "edit a");
        assert_eq!(head.kind, DocDiffTimelinePointKind::Commit);

        let file = &head.files[0];
        assert_eq!(file.path, "docs/a.md");
        assert!(!file.hunks.is_empty());

        let blocks = file.blocks.as_ref().unwrap();
        assert!(blocks
            .iter()
            .any(|b| b.kind == DocDiffBlockKind::Removed && b.raw == "first paragraph."));
        assert!(blocks
            .iter()
            .any(|b| b.kind == DocDiffBlockKind::Added && b.raw == "second paragraph."));
        // Block html populated via docgen-core markdown.
        assert!(blocks.iter().any(|b| b.html.contains("<p>")));

        assert!(head.total_added_lines >= 1 && head.total_removed_lines >= 1);
        assert!(!head.file_tree.is_empty());

        // First commit (oldest) is an Added file with empty old side.
        assert_eq!(report.timeline[1].files[0].status, DocDiffFileStatus::Added);

        // Report head selection.
        assert_eq!(report.selected_point_id.as_deref(), Some(head.id.as_str()));
        assert_eq!(report.selected_file_path.as_deref(), Some("docs/a.md"));
    }

    #[test]
    fn report_without_blocks_skips_block_rendering() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\n\nfirst paragraph.\n", "add a");
        r.commit_file("docs/a.md", "# A\n\nsecond paragraph.\n", "edit a");

        let report = build_doc_diff_report(&r.repo, "docs/a.md", 50)
            .unwrap()
            .unwrap();
        // Hunks/line stats are still computed and the change is not skipped.
        let head = &report.timeline[0];
        let file = &head.files[0];
        assert!(!file.hunks.is_empty());
        assert!(head.total_added_lines >= 1 && head.total_removed_lines >= 1);
        // Blocks are not built/rendered in the build path — nothing consumes them.
        assert!(file.blocks.is_none());
    }

    #[test]
    fn global_report_groups_all_changed_docs_per_commit() {
        let r = TempRepo::init();
        // Commit 1: two docs added.
        r.commit_file("docs/a.md", "# A\n\nfirst.\n", "init");
        std::fs::write(r.dir.join("docs/b.md"), "# B\n\nbee.\n").unwrap();
        r.commit_all("add b");
        // Commit 2: edit a + add nested c in ONE commit.
        std::fs::write(r.dir.join("docs/a.md"), "# A\n\nsecond.\n").unwrap();
        std::fs::create_dir_all(r.dir.join("docs/sub")).unwrap();
        std::fs::write(r.dir.join("docs/sub/c.md"), "# C\n\ncee.\n").unwrap();
        r.commit_all("edit a + add c");

        let report = build_global_doc_diff_report(&r.repo, "docs", 50, true)
            .unwrap()
            .unwrap();
        assert_eq!(report.mode, "build-history");

        // Head commit changed a.md (edit) and c.md (add) -> both present.
        let head = &report.timeline[0];
        let paths: Vec<&str> = head.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"docs/a.md"));
        assert!(paths.contains(&"docs/sub/c.md"));
        // Block html rendered for the detail report.
        assert!(head
            .files
            .iter()
            .any(|f| f.blocks.as_ref().is_some_and(|b| b.iter().any(|x| x.html.contains("<p>")))));
        // File tree nests docs/sub.
        assert!(!head.file_tree.is_empty());

        // Selection points at the head commit + its first file.
        assert_eq!(report.selected_point_id.as_deref(), Some(head.id.as_str()));
    }

    #[test]
    fn global_report_summary_strips_blocks() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "# A\n\none.\n", "init");
        r.commit_file("docs/a.md", "# A\n\ntwo.\n", "edit");
        let report = build_global_doc_diff_report(&r.repo, "docs", 50, false)
            .unwrap()
            .unwrap();
        assert!(report.timeline[0].files[0].blocks.is_none());
        assert!(!report.timeline[0].files[0].hunks.is_empty());
    }

    #[test]
    fn global_report_is_none_without_docs_commits() {
        let r = TempRepo::init();
        r.commit_file("notes/a.md", "x\n", "non-docs");
        assert!(build_global_doc_diff_report(&r.repo, "docs", 50, true)
            .unwrap()
            .is_none());
    }

    #[test]
    fn report_is_none_when_doc_has_no_history() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "x\n", "a");
        assert!(build_doc_diff_report(&r.repo, "docs/ghost.md", 50)
            .unwrap()
            .is_none());
    }
}
