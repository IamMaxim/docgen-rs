//! Base-ref selection — a faithful port of `git-refs.ts`.

/// The well-known git empty-tree object id. Used as the diff base for a
/// parentless (first) commit, so its full content shows up as additions.
pub const EMPTY_TREE_REF: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The diff base for a commit given its space-separated parent hashes:
/// the first parent for normal and merge commits, or the empty tree when
/// there are no parents. The `_hash` arg is unused (parity with the original).
pub fn base_ref_for_commit_parents(_hash: &str, parents: &str) -> String {
    parents
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .unwrap_or_else(|| EMPTY_TREE_REF.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree_for_parentless() {
        assert_eq!(base_ref_for_commit_parents("abc123", ""), EMPTY_TREE_REF);
    }

    #[test]
    fn first_parent_for_normal_and_merge() {
        assert_eq!(base_ref_for_commit_parents("abc123", "parent1"), "parent1");
        assert_eq!(
            base_ref_for_commit_parents("abc123", "parent1 parent2"),
            "parent1"
        );
    }
}
