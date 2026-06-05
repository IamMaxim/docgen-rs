//! Changed-file tree grouping — a faithful port of `file-tree.ts`.
//!
//! Files are sorted by path and grouped by their `docs/`-relative path
//! segments into `Group`/`File` nodes. Group ids are the cumulative
//! `docs/<…>` path; labels drop the leading `docs` segment for display.

use crate::types::{DocDiffFile, DocDiffFileTreeNode};

/// Group changed files into a nested tree keyed by `docs/`-relative segments.
pub fn build_file_tree(files: &[DocDiffFile]) -> Vec<DocDiffFileTreeNode> {
    let mut sorted: Vec<&DocDiffFile> = files.iter().collect();
    // `localeCompare` ≈ byte ordering for the ASCII paths under test.
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut roots: Vec<DocDiffFileTreeNode> = Vec::new();

    for file in sorted {
        let segments: Vec<&str> = file.path.split('/').filter(|s| !s.is_empty()).collect();
        let display: &[&str] = if segments.first() == Some(&"docs") {
            &segments[1..]
        } else {
            &segments[..]
        };
        let file_name = display.last().copied().unwrap_or(file.path.as_str());
        let group_segments = if display.is_empty() {
            &[][..]
        } else {
            &display[..display.len() - 1]
        };

        let mut cursor = &mut roots;
        let mut id_prefix = String::from("docs");

        for segment in group_segments {
            id_prefix = format!("{id_prefix}/{segment}");
            // Find the existing group index, or create one.
            let pos = cursor.iter().position(
                |node| matches!(node, DocDiffFileTreeNode::Group { id, .. } if id == &id_prefix),
            );
            let idx = match pos {
                Some(i) => i,
                None => {
                    cursor.push(DocDiffFileTreeNode::Group {
                        id: id_prefix.clone(),
                        label: (*segment).to_string(),
                        children: Vec::new(),
                    });
                    cursor.len() - 1
                }
            };
            cursor = match &mut cursor[idx] {
                DocDiffFileTreeNode::Group { children, .. } => children,
                _ => unreachable!("group node selected by id is always a group"),
            };
        }

        cursor.push(DocDiffFileTreeNode::File {
            id: file.path.clone(),
            label: file_name.to_string(),
            path: file.path.clone(),
            old_path: file.old_path.clone(),
            status: file.status,
            added_lines: file.added_lines,
            removed_lines: file.removed_lines,
        });
    }

    roots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DocDiffFileStatus;

    fn file(path: &str) -> DocDiffFile {
        DocDiffFile {
            path: path.into(),
            old_path: None,
            status: DocDiffFileStatus::Modified,
            added_lines: 1,
            removed_lines: 2,
            hunks: vec![],
            blocks: None,
        }
    }

    #[test]
    fn groups_changed_files_by_docs_path_segments() {
        let tree = build_file_tree(&[
            file("docs/dev/client.svx"),
            file("docs/game-design/world.md"),
        ]);
        assert_eq!(
            tree,
            vec![
                DocDiffFileTreeNode::Group {
                    id: "docs/dev".into(),
                    label: "dev".into(),
                    children: vec![DocDiffFileTreeNode::File {
                        id: "docs/dev/client.svx".into(),
                        label: "client.svx".into(),
                        path: "docs/dev/client.svx".into(),
                        old_path: None,
                        status: DocDiffFileStatus::Modified,
                        added_lines: 1,
                        removed_lines: 2,
                    }],
                },
                DocDiffFileTreeNode::Group {
                    id: "docs/game-design".into(),
                    label: "game-design".into(),
                    children: vec![DocDiffFileTreeNode::File {
                        id: "docs/game-design/world.md".into(),
                        label: "world.md".into(),
                        path: "docs/game-design/world.md".into(),
                        old_path: None,
                        status: DocDiffFileStatus::Modified,
                        added_lines: 1,
                        removed_lines: 2,
                    }],
                },
            ]
        );
    }
}
