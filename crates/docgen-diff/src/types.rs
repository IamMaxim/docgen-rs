//! Core diff data model — a faithful port of `types.ts`.
//!
//! The load-bearing parity property is the **JSON shape**: camelCase fields,
//! lowercase enum tags, and `oldPath`/`blocks` omitted when absent. These
//! mirror the original SvelteKit payloads byte-for-byte so the P3 navigation
//! island (and any consumer of the shipped JSON) reads identical data.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocDiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocDiffBlockKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocDiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocDiffTimelinePointKind {
    Commit,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffLine {
    pub kind: DocDiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DocDiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffBlock {
    pub id: String,
    pub kind: DocDiffBlockKind,
    pub raw: String,
    pub html: String,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: DocDiffFileStatus,
    pub added_lines: u32,
    pub removed_lines: u32,
    pub hunks: Vec<DocDiffHunk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<DocDiffBlock>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DocDiffFileTreeNode {
    Group {
        id: String,
        label: String,
        children: Vec<DocDiffFileTreeNode>,
    },
    File {
        id: String,
        label: String,
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        old_path: Option<String>,
        status: DocDiffFileStatus,
        added_lines: u32,
        removed_lines: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffTimelinePoint {
    pub id: String,
    pub kind: DocDiffTimelinePointKind,
    pub hash: Option<String>,
    pub short_hash: String,
    pub subject: String,
    pub author: Option<String>,
    /// RFC3339 string, parity with the TS ISO date.
    pub date: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
    pub files: Vec<DocDiffFile>,
    pub file_tree: Vec<DocDiffFileTreeNode>,
    pub total_added_lines: u32,
    pub total_removed_lines: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDiffReport {
    /// "build-history" in P2.
    pub mode: String,
    pub base_ref: String,
    pub head_ref: String,
    pub generated_at: String,
    pub timeline: Vec<DocDiffTimelinePoint>,
    pub selected_point_id: Option<String>,
    pub selected_file_path: Option<String>,
    pub files: Vec<DocDiffFile>,
    pub total_added_lines: u32,
    pub total_removed_lines: u32,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&DocDiffLineKind::Added).unwrap(),
            r#""added""#
        );
    }

    #[test]
    fn file_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&DocDiffFileStatus::Renamed).unwrap(),
            r#""renamed""#
        );
    }

    #[test]
    fn file_omits_old_path_and_blocks_when_absent() {
        let f = DocDiffFile {
            path: "docs/a.md".into(),
            old_path: None,
            status: DocDiffFileStatus::Modified,
            added_lines: 1,
            removed_lines: 2,
            hunks: vec![],
            blocks: None,
        };
        let v = serde_json::to_string(&f).unwrap();
        assert!(!v.contains("oldPath"));
        assert!(!v.contains("blocks"));
        assert!(v.contains(r#""addedLines":1"#));
        assert!(v.contains(r#""removedLines":2"#));
    }

    #[test]
    fn file_tree_node_uses_type_tag() {
        let n = DocDiffFileTreeNode::Group {
            id: "docs/dev".into(),
            label: "dev".into(),
            children: vec![],
        };
        let v = serde_json::to_string(&n).unwrap();
        assert!(v.contains(r#""type":"group""#));
    }

    #[test]
    fn file_tree_file_node_omits_old_path_when_absent() {
        let n = DocDiffFileTreeNode::File {
            id: "docs/a.md".into(),
            label: "a.md".into(),
            path: "docs/a.md".into(),
            old_path: None,
            status: DocDiffFileStatus::Added,
            added_lines: 3,
            removed_lines: 0,
        };
        let v = serde_json::to_string(&n).unwrap();
        assert!(v.contains(r#""type":"file""#));
        assert!(!v.contains("oldPath"));
    }
}
