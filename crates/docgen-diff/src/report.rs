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
    let revs = history::doc_revisions(repo, doc_rel_path, limit)?;
    if revs.is_empty() {
        return Ok(None);
    }

    let mut timeline: Vec<DocDiffTimelinePoint> = Vec::with_capacity(revs.len());

    for rev in revs {
        let hunks = line_diff::build_line_hunks_default(&rev.old_text, &rev.new_text);

        let mut blocks = block_diff::build_block_diff(&rev.old_text, &rev.new_text);
        for block in &mut blocks {
            block.html = render_markdown(&block.raw);
        }

        // Skip the file when there is no visible change (parity with the TS
        // `return null`). In build-history each point is a single doc file.
        let all_context = blocks.iter().all(|b| b.kind == DocDiffBlockKind::Context);
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
                blocks: Some(blocks),
            }]
        };

        let file_tree = file_tree::build_file_tree(&files);
        let total_added_lines = files.iter().map(|f| f.added_lines).sum();
        let total_removed_lines = files.iter().map(|f| f.removed_lines).sum();
        let base_ref =
            git_refs::base_ref_for_commit_parents(&rev.meta.hash, &rev.meta.parents.join(" "));
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

        let report = build_doc_diff_report(&r.repo, "docs/a.md", 50)
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
    fn report_is_none_when_doc_has_no_history() {
        let r = TempRepo::init();
        r.commit_file("docs/a.md", "x\n", "a");
        assert!(build_doc_diff_report(&r.repo, "docs/ghost.md", 50)
            .unwrap()
            .is_none());
    }
}
