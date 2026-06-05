//! Line-level hunk diff — a faithful port of `line-diff.ts`.
//!
//! The originals roll their own LCS with a load-bearing tie-break
//! (`lcs[i+1][j] >= lcs[i][j+1]` ⇒ prefer *removed*); we port it directly so
//! the emitted op stream matches byte-for-byte. Hunks expand `context_lines`
//! around each change and merge when adjacent.

use crate::types::{DocDiffHunk, DocDiffLine, DocDiffLineKind};

/// A diff op carrying the cursor positions before it was emitted, used to
/// compute hunk start lines when a hunk begins on a one-sided change.
#[derive(Debug, Clone)]
struct DiffOp {
    kind: DocDiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    text: String,
    old_before: usize,
    new_before: usize,
}

/// Build line-level hunks between two texts, with `context_lines` of context
/// around each change. Returns an empty vec when the texts are identical.
pub fn build_line_hunks(old_text: &str, new_text: &str, context_lines: usize) -> Vec<DocDiffHunk> {
    if old_text == new_text {
        return vec![];
    }

    let old_lines = split_lines(old_text);
    let new_lines = split_lines(new_text);
    let ops = build_diff_ops(&old_lines, &new_lines);
    let ranges = build_hunk_ranges(&ops, context_lines);

    ranges
        .into_iter()
        .map(|(start, end)| build_hunk(&ops[start..=end]))
        .collect()
}

/// Convenience matching the TS default of 3 context lines.
pub fn build_line_hunks_default(old_text: &str, new_text: &str) -> Vec<DocDiffHunk> {
    build_line_hunks(old_text, new_text, 3)
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }

    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect();
    if lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }

    lines
}

fn build_diff_ops(old_lines: &[String], new_lines: &[String]) -> Vec<DiffOp> {
    let lcs = build_lcs_table(old_lines, new_lines);
    let mut ops = Vec::new();
    let mut old_index = 0usize;
    let mut new_index = 0usize;

    while old_index < old_lines.len() || new_index < new_lines.len() {
        let old_before = old_index;
        let new_before = new_index;

        if old_index < old_lines.len()
            && new_index < new_lines.len()
            && old_lines[old_index] == new_lines[new_index]
        {
            ops.push(DiffOp {
                kind: DocDiffLineKind::Context,
                old_line: Some((old_index + 1) as u32),
                new_line: Some((new_index + 1) as u32),
                text: old_lines[old_index].clone(),
                old_before,
                new_before,
            });
            old_index += 1;
            new_index += 1;
        } else if old_index < old_lines.len()
            && (new_index == new_lines.len()
                || lcs[old_index + 1][new_index] >= lcs[old_index][new_index + 1])
        {
            ops.push(DiffOp {
                kind: DocDiffLineKind::Removed,
                old_line: Some((old_index + 1) as u32),
                new_line: None,
                text: old_lines[old_index].clone(),
                old_before,
                new_before,
            });
            old_index += 1;
        } else {
            ops.push(DiffOp {
                kind: DocDiffLineKind::Added,
                old_line: None,
                new_line: Some((new_index + 1) as u32),
                text: new_lines[new_index].clone(),
                old_before,
                new_before,
            });
            new_index += 1;
        }
    }

    ops
}

fn build_lcs_table(old_lines: &[String], new_lines: &[String]) -> Vec<Vec<usize>> {
    let mut table = vec![vec![0usize; new_lines.len() + 1]; old_lines.len() + 1];

    for old_index in (0..old_lines.len()).rev() {
        for new_index in (0..new_lines.len()).rev() {
            table[old_index][new_index] = if old_lines[old_index] == new_lines[new_index] {
                table[old_index + 1][new_index + 1] + 1
            } else {
                table[old_index + 1][new_index].max(table[old_index][new_index + 1])
            };
        }
    }

    table
}

fn build_hunk_ranges(ops: &[DiffOp], context_lines: usize) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    for (index, op) in ops.iter().enumerate() {
        if op.kind == DocDiffLineKind::Context {
            continue;
        }

        let start = index.saturating_sub(context_lines);
        let end = (index + context_lines).min(ops.len() - 1);

        if let Some(previous) = ranges.last_mut() {
            if start <= previous.1 + 1 {
                previous.1 = previous.1.max(end);
                continue;
            }
        }
        ranges.push((start, end));
    }

    ranges
}

fn build_hunk(lines: &[DiffOp]) -> DocDiffHunk {
    let old_lines = lines
        .iter()
        .filter(|line| line.kind != DocDiffLineKind::Added)
        .count() as u32;
    let new_lines = lines
        .iter()
        .filter(|line| line.kind != DocDiffLineKind::Removed)
        .count() as u32;

    DocDiffHunk {
        old_start: hunk_start(lines, Side::Old),
        old_lines,
        new_start: hunk_start(lines, Side::New),
        new_lines,
        lines: lines
            .iter()
            .map(|line| DocDiffLine {
                kind: line.kind,
                old_line: line.old_line,
                new_line: line.new_line,
                text: line.text.clone(),
            })
            .collect(),
    }
}

enum Side {
    Old,
    New,
}

fn hunk_start(lines: &[DiffOp], side: Side) -> u32 {
    let first_line = lines.iter().find_map(|line| match side {
        Side::Old => line.old_line,
        Side::New => line.new_line,
    });

    if let Some(value) = first_line {
        return value;
    }

    match side {
        Side::Old => lines[0].old_before as u32,
        Side::New => lines[0].new_before as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use DocDiffLineKind::*;

    fn line(
        kind: DocDiffLineKind,
        old_line: Option<u32>,
        new_line: Option<u32>,
        text: &str,
    ) -> DocDiffLine {
        DocDiffLine {
            kind,
            old_line,
            new_line,
            text: text.into(),
        }
    }

    #[test]
    fn identical_returns_no_hunks() {
        assert!(build_line_hunks_default("alpha\nbeta\ngamma", "alpha\nbeta\ngamma").is_empty());
    }

    #[test]
    fn replacement_marks_context_removed_added_context() {
        let h = build_line_hunks_default("alpha\nbeta\ngamma", "alpha\ndelta\ngamma");
        assert_eq!(
            h,
            vec![DocDiffHunk {
                old_start: 1,
                old_lines: 3,
                new_start: 1,
                new_lines: 3,
                lines: vec![
                    line(Context, Some(1), Some(1), "alpha"),
                    line(Removed, Some(2), None, "beta"),
                    line(Added, None, Some(2), "delta"),
                    line(Context, Some(3), Some(3), "gamma"),
                ],
            }]
        );
    }

    #[test]
    fn distant_edits_split_into_two_hunks_with_context_one() {
        let h = build_line_hunks("a\nb\nc\nd\ne\nf\ng", "a\nB\nc\nd\ne\nF\ng", 1);
        assert_eq!(
            h,
            vec![
                DocDiffHunk {
                    old_start: 1,
                    old_lines: 3,
                    new_start: 1,
                    new_lines: 3,
                    lines: vec![
                        line(Context, Some(1), Some(1), "a"),
                        line(Removed, Some(2), None, "b"),
                        line(Added, None, Some(2), "B"),
                        line(Context, Some(3), Some(3), "c"),
                    ],
                },
                DocDiffHunk {
                    old_start: 5,
                    old_lines: 3,
                    new_start: 5,
                    new_lines: 3,
                    lines: vec![
                        line(Context, Some(5), Some(5), "e"),
                        line(Removed, Some(6), None, "f"),
                        line(Added, None, Some(6), "F"),
                        line(Context, Some(7), Some(7), "g"),
                    ],
                },
            ]
        );
    }
}
