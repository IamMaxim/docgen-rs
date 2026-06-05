//! Payload summarization — a faithful port of `payloads.ts`.
//!
//! Produces the lightweight JSON the history page ships as data: heavy `hunks`
//! and rendered `blocks` are stripped while file-tree and metadata are kept.

use crate::types::{DocDiffFile, DocDiffReport, DocDiffTimelinePoint};

/// Clone a file with its heavy `hunks` cleared and `blocks` dropped.
pub fn summarize_file(file: &DocDiffFile) -> DocDiffFile {
    DocDiffFile {
        hunks: vec![],
        blocks: None,
        ..file.clone()
    }
}

/// Clone a timeline point with each file summarized.
pub fn summarize_timeline_point(point: &DocDiffTimelinePoint) -> DocDiffTimelinePoint {
    DocDiffTimelinePoint {
        files: point.files.iter().map(summarize_file).collect(),
        ..point.clone()
    }
}

/// Clone a report with both its timeline points and top-level files summarized.
pub fn summarize_report(report: &DocDiffReport) -> DocDiffReport {
    DocDiffReport {
        timeline: report
            .timeline
            .iter()
            .map(summarize_timeline_point)
            .collect(),
        files: report.files.iter().map(summarize_file).collect(),
        ..report.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        DocDiffBlock, DocDiffBlockKind, DocDiffFileStatus, DocDiffFileTreeNode, DocDiffHunk,
        DocDiffLine, DocDiffLineKind, DocDiffTimelinePointKind,
    };

    fn full_point() -> DocDiffTimelinePoint {
        DocDiffTimelinePoint {
            id: "abc123".into(),
            kind: DocDiffTimelinePointKind::Commit,
            hash: Some("abc123".into()),
            short_hash: "abc123".into(),
            subject: "docs".into(),
            author: Some("author".into()),
            date: Some("2026-05-13T00:00:00.000Z".into()),
            base_ref: "abc122".into(),
            head_ref: "abc123".into(),
            files: vec![DocDiffFile {
                path: "docs/a.md".into(),
                old_path: None,
                status: DocDiffFileStatus::Modified,
                added_lines: 1,
                removed_lines: 1,
                hunks: vec![DocDiffHunk {
                    old_start: 1,
                    old_lines: 1,
                    new_start: 1,
                    new_lines: 1,
                    lines: vec![DocDiffLine {
                        kind: DocDiffLineKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        text: "new".into(),
                    }],
                }],
                blocks: Some(vec![DocDiffBlock {
                    id: "block".into(),
                    kind: DocDiffBlockKind::Added,
                    raw: "new".into(),
                    html: "<p>new</p>".into(),
                    old_index: None,
                    new_index: Some(1),
                }]),
            }],
            file_tree: vec![DocDiffFileTreeNode::Group {
                id: "docs".into(),
                label: "docs".into(),
                children: vec![DocDiffFileTreeNode::File {
                    id: "docs/a.md".into(),
                    label: "a.md".into(),
                    path: "docs/a.md".into(),
                    old_path: None,
                    status: DocDiffFileStatus::Modified,
                    added_lines: 1,
                    removed_lines: 1,
                }],
            }],
            total_added_lines: 1,
            total_removed_lines: 1,
            warnings: vec![],
        }
    }

    #[test]
    fn summarize_strips_hunks_and_blocks() {
        let summary = summarize_timeline_point(&full_point());
        assert_eq!(summary.files.len(), 1);
        assert!(summary.files[0].hunks.is_empty());
        assert!(summary.files[0].blocks.is_none());
        assert!(matches!(
            summary.file_tree[0],
            DocDiffFileTreeNode::Group { .. }
        ));
    }
}
