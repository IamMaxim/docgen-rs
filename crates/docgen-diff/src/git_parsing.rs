//! `git --name-status` / untracked parsing — a faithful port of `git-parsing.ts`.
//!
//! The `is_doc_path` filter (`docs/` + `.md`/`.svx`) belongs to the worktree/P5
//! path; P2's `report.rs` reads a single known doc path and does not depend on
//! it. The port stays faithful (including `.svx` support) for P5 reuse.

use crate::types::DocDiffFileStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameStatusEntry {
    pub status: DocDiffFileStatus,
    pub path: String,
    pub old_path: Option<String>,
}

/// Parse `git diff --name-status` output into doc-file change entries.
pub fn parse_name_status(stdout: &str) -> Vec<NameStatusEntry> {
    stdout
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .flat_map(parse_name_status_line)
        .collect()
}

/// Parse `git ls-files --others` output, keeping only doc paths.
pub fn parse_untracked_docs(stdout: &str) -> Vec<String> {
    stdout
        .split('\n')
        .map(|line| line.trim())
        .filter(|line| is_doc_path(line))
        .map(|line| line.to_string())
        .collect()
}

fn parse_name_status_line(line: &str) -> Vec<NameStatusEntry> {
    let mut parts = line.split('\t');
    let raw_status = parts.next().unwrap_or("");
    let first_path = parts.next();
    let second_path = parts.next();

    match raw_status {
        "A" => entry(DocDiffFileStatus::Added, first_path),
        "M" => entry(DocDiffFileStatus::Modified, first_path),
        "D" => entry(DocDiffFileStatus::Deleted, first_path),
        s if s.starts_with('R') => {
            let old_is_doc = is_doc_path_opt(first_path);
            let new_is_doc = is_doc_path_opt(second_path);

            if old_is_doc && new_is_doc {
                return vec![NameStatusEntry {
                    status: DocDiffFileStatus::Renamed,
                    old_path: Some(first_path.unwrap().to_string()),
                    path: second_path.unwrap().to_string(),
                }];
            }
            if old_is_doc {
                return vec![NameStatusEntry {
                    status: DocDiffFileStatus::Deleted,
                    path: first_path.unwrap().to_string(),
                    old_path: None,
                }];
            }
            if new_is_doc {
                return vec![NameStatusEntry {
                    status: DocDiffFileStatus::Added,
                    path: second_path.unwrap().to_string(),
                    old_path: None,
                }];
            }
            vec![]
        }
        _ => vec![],
    }
}

fn entry(status: DocDiffFileStatus, path: Option<&str>) -> Vec<NameStatusEntry> {
    match path {
        Some(p) if is_doc_path(p) => vec![NameStatusEntry {
            status,
            path: p.to_string(),
            old_path: None,
        }],
        _ => vec![],
    }
}

fn is_doc_path_opt(path: Option<&str>) -> bool {
    path.map(is_doc_path).unwrap_or(false)
}

fn is_doc_path(path: &str) -> bool {
    path.starts_with("docs/") && (path.ends_with(".md") || path.ends_with(".svx"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use DocDiffFileStatus::*;

    fn ns(status: DocDiffFileStatus, path: &str, old_path: Option<&str>) -> NameStatusEntry {
        NameStatusEntry {
            status,
            path: path.into(),
            old_path: old_path.map(|s| s.into()),
        }
    }

    #[test]
    fn parses_added_modified_deleted_renamed() {
        assert_eq!(
            parse_name_status(
                "A\tdocs/new.md\nM\tdocs/a.md\nD\tdocs/old.md\nR100\tdocs/from.md\tdocs/to.md\n"
            ),
            vec![
                ns(Added, "docs/new.md", None),
                ns(Modified, "docs/a.md", None),
                ns(Deleted, "docs/old.md", None),
                ns(Renamed, "docs/to.md", Some("docs/from.md")),
            ]
        );
    }

    #[test]
    fn one_sided_docs_renames_map_to_add_or_delete() {
        assert_eq!(
            parse_name_status(
                "R100\tdocs/from.md\toutside/from.md\nR100\toutside/to.md\tdocs/to.md\n"
            ),
            vec![
                ns(Deleted, "docs/from.md", None),
                ns(Added, "docs/to.md", None),
            ]
        );
    }

    #[test]
    fn untracked_keeps_docs_md_and_svx() {
        assert_eq!(
            parse_untracked_docs("docs/a.md\ndocs/b.svx\nclient/nope.md\n"),
            vec!["docs/a.md", "docs/b.svx"]
        );
    }
}
