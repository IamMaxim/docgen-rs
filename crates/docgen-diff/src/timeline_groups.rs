//! Timeline date bucketing — a faithful port of `timeline-groups.ts`.
//!
//! Adapts the JS `Date`-based logic to `chrono::Local`. `ymd` takes 1-based
//! year/month/day ints (the JS `ymd(new Date(...))` used 0-based months; our
//! API is 1-based and documented). `format_date` parses an RFC3339 string to
//! local time and formats it as `YYYY-MM-DD`, returning `""` on null/invalid.

use chrono::{DateTime, Datelike, Local, TimeDelta};

use crate::types::{DocDiffTimelinePoint, DocDiffTimelinePointKind};

/// Zero-padded `YYYY-MM-DD` from 1-based year/month/day.
pub fn ymd(y: i32, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// Parse an RFC3339 string, convert to local time, format as `YYYY-MM-DD`.
/// Returns `""` for `None` or an unparseable value (parity with the JS
/// `new Date(value)` + `ymd`, which is local-timezone).
pub fn format_date(value: Option<&str>) -> String {
    let Some(s) = value else {
        return String::new();
    };
    match DateTime::parse_from_rfc3339(s) {
        Ok(dt) => {
            let local = dt.with_timezone(&Local);
            ymd(local.year(), local.month(), local.day())
        }
        Err(_) => String::new(),
    }
}

/// A timeline bucket: a label plus the points that fall under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineBucket {
    pub label: String,
    pub points: Vec<DocDiffTimelinePoint>,
}

/// Bucket label for a single point relative to `now`: `"Working tree"` for
/// worktree points, else `"Today"` / `"Yesterday"` / `"Earlier"`.
pub fn bucket_label(point: &DocDiffTimelinePoint, now: DateTime<Local>) -> String {
    if point.kind == DocDiffTimelinePointKind::Worktree {
        return "Working tree".into();
    }
    let today = ymd(now.year(), now.month(), now.day());
    let prev = now - TimeDelta::days(1);
    let yesterday = ymd(prev.year(), prev.month(), prev.day());
    let day = format_date(point.date.as_deref());
    if day == today {
        "Today".into()
    } else if day == yesterday {
        "Yesterday".into()
    } else {
        "Earlier".into()
    }
}

/// Group points into ordered buckets by label, preserving first-seen order.
pub fn group_timeline(
    points: Vec<DocDiffTimelinePoint>,
    now: DateTime<Local>,
) -> Vec<TimelineBucket> {
    let mut buckets: Vec<TimelineBucket> = Vec::new();
    for point in points {
        let label = bucket_label(&point, now);
        if let Some(existing) = buckets.iter_mut().find(|b| b.label == label) {
            existing.points.push(point);
        } else {
            buckets.push(TimelineBucket {
                label,
                points: vec![point],
            });
        }
    }
    buckets
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn point_dated(date: &str) -> DocDiffTimelinePoint {
        point(
            DocDiffTimelinePointKind::Commit,
            Some(date.to_string()),
            "p",
        )
    }

    fn point(
        kind: DocDiffTimelinePointKind,
        date: Option<String>,
        id: &str,
    ) -> DocDiffTimelinePoint {
        DocDiffTimelinePoint {
            id: id.into(),
            kind,
            hash: Some("abc".into()),
            short_hash: "abc".into(),
            subject: "s".into(),
            author: None,
            date,
            base_ref: String::new(),
            head_ref: String::new(),
            files: vec![],
            file_tree: vec![],
            total_added_lines: 0,
            total_removed_lines: 0,
            warnings: vec![],
        }
    }

    #[test]
    fn ymd_formats_year_month_day_with_zero_padding() {
        assert_eq!(ymd(2026, 1, 5), "2026-01-05");
        assert_eq!(ymd(2026, 12, 31), "2026-12-31");
    }

    #[test]
    fn format_date_returns_empty_for_null_or_invalid() {
        assert_eq!(format_date(None), "");
        assert_eq!(format_date(Some("not-a-date")), "");
    }

    #[test]
    fn format_date_parses_iso_strings() {
        let result = format_date(Some("2026-03-15T12:00:00Z"));
        // Timezone-dependent but YYYY-MM-DD format, day is 14 or 15.
        assert_eq!(result.len(), 10);
        assert!(result.starts_with("2026-03-1"));
        assert!(result == "2026-03-14" || result == "2026-03-15");
    }

    #[test]
    fn bucket_label_returns_working_tree_for_worktree_kind() {
        let now = Local.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap();
        assert_eq!(
            bucket_label(&point(DocDiffTimelinePointKind::Worktree, None, "wt"), now),
            "Working tree"
        );
    }

    #[test]
    fn buckets_today_yesterday_earlier() {
        let now = Local.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap();
        let today = Local
            .with_ymd_and_hms(2026, 5, 15, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let yesterday = Local
            .with_ymd_and_hms(2026, 5, 14, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let earlier = Local
            .with_ymd_and_hms(2026, 5, 1, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        assert_eq!(bucket_label(&point_dated(&today), now), "Today");
        assert_eq!(bucket_label(&point_dated(&yesterday), now), "Yesterday");
        assert_eq!(bucket_label(&point_dated(&earlier), now), "Earlier");
    }

    #[test]
    fn group_timeline_preserves_order_and_groups_by_label() {
        let now = Local.with_ymd_and_hms(2026, 5, 15, 12, 0, 0).unwrap();
        let today = Local
            .with_ymd_and_hms(2026, 5, 15, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let earlier = Local
            .with_ymd_and_hms(2026, 5, 1, 8, 0, 0)
            .unwrap()
            .to_rfc3339();
        let buckets = group_timeline(
            vec![
                point(DocDiffTimelinePointKind::Worktree, None, "wt"),
                point(DocDiffTimelinePointKind::Commit, Some(today.clone()), "a"),
                point(DocDiffTimelinePointKind::Commit, Some(today), "b"),
                point(DocDiffTimelinePointKind::Commit, Some(earlier), "c"),
            ],
            now,
        );
        let proj: Vec<(String, Vec<String>)> = buckets
            .iter()
            .map(|b| {
                (
                    b.label.clone(),
                    b.points.iter().map(|p| p.id.clone()).collect(),
                )
            })
            .collect();
        assert_eq!(
            proj,
            vec![
                ("Working tree".to_string(), vec!["wt".to_string()]),
                ("Today".to_string(), vec!["a".to_string(), "b".to_string()]),
                ("Earlier".to_string(), vec!["c".to_string()]),
            ]
        );
    }
}
