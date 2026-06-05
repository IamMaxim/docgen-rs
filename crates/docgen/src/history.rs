//! Map a `docgen_diff::DocDiffReport` into `docgen-render`'s view model.
//!
//! This is the pure (git-free) bridge between the diff domain types and the
//! history-page template. Kept here so `docgen-render` stays free of the
//! `docgen-diff` domain types while the CLI owns the projection.

use chrono::{DateTime, Local};
use docgen_diff::{
    format_date, group_timeline, DocDiffFile, DocDiffFileStatus, DocDiffHunk, DocDiffLine,
    DocDiffLineKind, DocDiffReport, DocDiffTimelinePoint,
};
use docgen_render::{FileView, HunkView, LineView, TimelineBucketView, TimelinePointView};

fn line_kind_str(kind: DocDiffLineKind) -> &'static str {
    match kind {
        DocDiffLineKind::Context => "context",
        DocDiffLineKind::Added => "added",
        DocDiffLineKind::Removed => "removed",
    }
}

fn status_str(status: DocDiffFileStatus) -> &'static str {
    match status {
        DocDiffFileStatus::Added => "added",
        DocDiffFileStatus::Modified => "modified",
        DocDiffFileStatus::Deleted => "deleted",
        DocDiffFileStatus::Renamed => "renamed",
    }
}

fn line_view(line: &DocDiffLine) -> LineView {
    LineView {
        kind: line_kind_str(line.kind).to_string(),
        text: line.text.clone(),
        old_line: line.old_line,
        new_line: line.new_line,
    }
}

fn hunk_view(hunk: &DocDiffHunk) -> HunkView {
    HunkView {
        lines: hunk.lines.iter().map(line_view).collect(),
    }
}

fn file_view(file: &DocDiffFile) -> FileView {
    FileView {
        path: file.path.clone(),
        status: status_str(file.status).to_string(),
        hunks: file.hunks.iter().map(hunk_view).collect(),
    }
}

fn point_view(point: &DocDiffTimelinePoint) -> TimelinePointView {
    TimelinePointView {
        short_hash: point.short_hash.clone(),
        subject: point.subject.clone(),
        author: point.author.clone(),
        date: match point.date.as_deref() {
            Some(d) => {
                let formatted = format_date(Some(d));
                if formatted.is_empty() {
                    None
                } else {
                    Some(formatted)
                }
            }
            None => None,
        },
        added_lines: point.total_added_lines,
        removed_lines: point.total_removed_lines,
        files: point.files.iter().map(file_view).collect(),
    }
}

/// Group a report's timeline into date buckets and project each point into the
/// render-friendly view model. `now` drives the Today/Yesterday/Earlier labels.
pub fn report_to_buckets(report: &DocDiffReport, now: DateTime<Local>) -> Vec<TimelineBucketView> {
    group_timeline(report.timeline.clone(), now)
        .into_iter()
        .map(|bucket| TimelineBucketView {
            label: bucket.label,
            points: bucket.points.iter().map(point_view).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use docgen_diff::{
        DocDiffBlock, DocDiffBlockKind, DocDiffFileTreeNode, DocDiffTimelinePointKind,
    };

    fn point_at(date: &str, subject: &str) -> DocDiffTimelinePoint {
        DocDiffTimelinePoint {
            id: "h".into(),
            kind: DocDiffTimelinePointKind::Commit,
            hash: Some("hash".into()),
            short_hash: "abc1234".into(),
            subject: subject.into(),
            author: Some("docgen test".into()),
            date: Some(date.into()),
            base_ref: "parent".into(),
            head_ref: "hash".into(),
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
                    lines: vec![
                        DocDiffLine {
                            kind: DocDiffLineKind::Removed,
                            old_line: Some(1),
                            new_line: None,
                            text: "first".into(),
                        },
                        DocDiffLine {
                            kind: DocDiffLineKind::Added,
                            old_line: None,
                            new_line: Some(1),
                            text: "second".into(),
                        },
                    ],
                }],
                blocks: Some(vec![DocDiffBlock {
                    id: "block-0".into(),
                    kind: DocDiffBlockKind::Added,
                    raw: "second".into(),
                    html: "<p>second</p>".into(),
                    old_index: None,
                    new_index: Some(0),
                }]),
            }],
            file_tree: vec![DocDiffFileTreeNode::Group {
                id: "docs".into(),
                label: "docs".into(),
                children: vec![],
            }],
            total_added_lines: 1,
            total_removed_lines: 1,
            warnings: vec![],
        }
    }

    fn report_with(points: Vec<DocDiffTimelinePoint>) -> DocDiffReport {
        DocDiffReport {
            mode: "build-history".into(),
            base_ref: "parent".into(),
            head_ref: "hash".into(),
            generated_at: "2026-05-15T12:00:00Z".into(),
            timeline: points,
            selected_point_id: None,
            selected_file_path: None,
            files: vec![],
            total_added_lines: 1,
            total_removed_lines: 1,
            warnings: vec![],
        }
    }

    #[test]
    fn maps_report_into_view_buckets() {
        let now = Local.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap();
        let today = Local
            .with_ymd_and_hms(2026, 5, 15, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let earlier = Local
            .with_ymd_and_hms(2026, 5, 1, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let report = report_with(vec![
            point_at(&today, "edit a"),
            point_at(&earlier, "add a"),
        ]);

        let buckets = report_to_buckets(&report, now);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].label, "Today");
        assert_eq!(buckets[1].label, "Earlier");

        let p = &buckets[0].points[0];
        assert_eq!(p.subject, "edit a");
        assert_eq!(p.short_hash, "abc1234");
        assert_eq!(p.added_lines, 1);
        assert_eq!(p.removed_lines, 1);
        // date stringified to local YYYY-MM-DD
        assert_eq!(p.date.as_deref(), Some("2026-05-15"));

        let f = &p.files[0];
        assert_eq!(f.path, "docs/a.md");
        assert_eq!(f.status, "modified");
        let lines = &f.hunks[0].lines;
        assert_eq!(lines[0].kind, "removed");
        assert_eq!(lines[0].text, "first");
        assert_eq!(lines[1].kind, "added");
        assert_eq!(lines[1].text, "second");
    }
}
