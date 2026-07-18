use std::collections::BTreeMap;

use crate::model::{Doc, TreeNode};

#[derive(Default)]
struct Builder {
    dirs: BTreeMap<String, Builder>,
    docs: BTreeMap<String, (String, String)>, // leaf name -> (slug, title)
    /// Folder note: an `index.md` directly inside this directory. Stored as its
    /// slug instead of being added to `docs`, so the folder itself links to it.
    note: Option<String>,
    /// `sidebar: false` on this directory's `index.md` — hide the entire subtree.
    hidden: bool,
}

fn insert(node: &mut Builder, parts: &[&str], slug: &str, title: &str, depth: usize, hidden: bool) {
    match parts {
        // An `index.md` *inside* a folder (depth > 0) is that folder's note, not a
        // child entry. A root-level `index.md` (depth 0) is the site home and stays
        // an ordinary doc. A hidden folder note hides the whole directory subtree.
        [leaf] if *leaf == "index" && depth > 0 => {
            if hidden {
                node.hidden = true;
            } else {
                node.note = Some(slug.to_string());
            }
        }
        // A hidden leaf is dropped from the tree entirely (it still builds + is
        // reachable by URL; it just doesn't appear in the sidebar).
        [_] if hidden => {}
        [leaf] => {
            node.docs
                .insert(leaf.to_string(), (slug.to_string(), title.to_string()));
        }
        [head, rest @ ..] => {
            insert(
                node.dirs.entry(head.to_string()).or_default(),
                rest,
                slug,
                title,
                depth + 1,
                hidden,
            );
        }
        [] => {}
    }
}

fn to_nodes(builder: Builder) -> Vec<TreeNode> {
    let mut out = Vec::new();
    // Directories first (BTreeMap keeps them name-sorted), then loose docs.
    for (name, child) in builder.dirs {
        // A directory hidden via its `index.md` drops with its whole subtree.
        if child.hidden {
            continue;
        }
        let slug = child.note.clone();
        let children = to_nodes(child);
        // Prune a directory left empty by hidden children: with no children and
        // no folder note it would render as a dangling, non-navigable group.
        if children.is_empty() && slug.is_none() {
            continue;
        }
        out.push(TreeNode::Dir {
            name,
            slug,
            children,
        });
    }
    for (name, (slug, title)) in builder.docs {
        out.push(TreeNode::Doc { name, slug, title });
    }
    out
}

/// Build a name-sorted sidebar tree from documents, keyed off their slugs.
/// A folder's `index.md` becomes the folder's note (the folder links to it)
/// rather than a separate `index` child entry. A doc with `sidebar: false`
/// (`hidden_from_sidebar`) is omitted; on a folder's `index.md` that hides the
/// whole subtree, and a directory left empty by hidden children is pruned.
pub fn build_tree(docs: &[Doc]) -> Vec<TreeNode> {
    let mut root = Builder::default();
    for doc in docs {
        let parts: Vec<&str> = doc.slug.split('/').collect();
        insert(
            &mut root,
            &parts,
            &doc.slug,
            &doc.title,
            0,
            doc.hidden_from_sidebar,
        );
    }
    to_nodes(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Doc, TreeNode};

    fn doc(slug: &str, title: &str) -> Doc {
        Doc {
            rel_path: format!("{slug}.md"),
            slug: slug.into(),
            title: title.into(),
            description: None,
            body_html: String::new(),
            has_math: false,
            has_mermaid: false,
            components_used: Default::default(),
            headings: Vec::new(),
            hidden_from_sidebar: false,
        }
    }

    /// Like [`doc`], but flagged `sidebar: false`.
    fn hidden_doc(slug: &str, title: &str) -> Doc {
        Doc {
            hidden_from_sidebar: true,
            ..doc(slug, title)
        }
    }

    #[test]
    fn groups_docs_under_directories() {
        let docs = vec![doc("index", "Home"), doc("guide/intro", "Intro")];
        let tree = build_tree(&docs);

        // Directories come before loose docs; both sorted by name.
        assert_eq!(tree.len(), 2);
        match &tree[0] {
            TreeNode::Dir { name, children, .. } => {
                assert_eq!(name, "guide");
                assert_eq!(children.len(), 1);
                assert!(
                    matches!(&children[0], TreeNode::Doc { slug, .. } if slug == "guide/intro")
                );
            }
            other => panic!("expected dir, got {other:?}"),
        }
        assert!(matches!(&tree[1], TreeNode::Doc { slug, .. } if slug == "index"));
    }

    #[test]
    fn dirs_come_before_docs_even_when_doc_sorts_first() {
        // "aaa" sorts before "zzz_dir" alphabetically; dirs-first must still win.
        let docs = vec![doc("aaa", "A"), doc("zzz_dir/page", "Page")];
        let tree = build_tree(&docs);
        assert_eq!(tree.len(), 2);
        assert!(matches!(&tree[0], TreeNode::Dir { name, .. } if name == "zzz_dir"));
        assert!(matches!(&tree[1], TreeNode::Doc { slug, .. } if slug == "aaa"));
    }

    #[test]
    fn multiple_dirs_and_docs_each_sorted_within_group() {
        let docs = vec![
            doc("m_doc", "M"),
            doc("b_dir/x", "X"),
            doc("a_doc", "A"),
            doc("a_dir/y", "Y"),
        ];
        let tree = build_tree(&docs);
        // Dirs first (a_dir, b_dir), then docs (a_doc, m_doc).
        assert!(matches!(&tree[0], TreeNode::Dir { name, .. } if name == "a_dir"));
        assert!(matches!(&tree[1], TreeNode::Dir { name, .. } if name == "b_dir"));
        assert!(matches!(&tree[2], TreeNode::Doc { name, .. } if name == "a_doc"));
        assert!(matches!(&tree[3], TreeNode::Doc { name, .. } if name == "m_doc"));
    }

    #[test]
    fn groups_nested_directories() {
        let docs = vec![doc("a/b/c", "Deep")];
        let tree = build_tree(&docs);
        // a -> b -> c (doc), three levels deep.
        let a = match &tree[0] {
            TreeNode::Dir { name, children, .. } if name == "a" => children,
            other => panic!("expected dir a, got {other:?}"),
        };
        let b = match &a[0] {
            TreeNode::Dir { name, children, .. } if name == "b" => children,
            other => panic!("expected dir b, got {other:?}"),
        };
        assert!(matches!(&b[0], TreeNode::Doc { slug, .. } if slug == "a/b/c"));
    }

    #[test]
    fn folder_index_becomes_folder_note_not_child() {
        // `guide/index.md` is the "guide" folder's note: the dir carries its slug
        // and `index` is NOT a separate child. `guide/intro` stays a child.
        let docs = vec![doc("guide/index", "Guide"), doc("guide/intro", "Intro")];
        let tree = build_tree(&docs);
        assert_eq!(tree.len(), 1);
        match &tree[0] {
            TreeNode::Dir {
                name,
                slug,
                children,
            } => {
                assert_eq!(name, "guide");
                assert_eq!(slug.as_deref(), Some("guide/index"));
                // Only `intro` is a child — no `index` entry.
                assert_eq!(children.len(), 1);
                assert!(
                    matches!(&children[0], TreeNode::Doc { slug, .. } if slug == "guide/intro")
                );
            }
            other => panic!("expected dir, got {other:?}"),
        }
    }

    #[test]
    fn hidden_leaf_is_omitted_from_sidebar() {
        // `sidebar: false` drops the page from the tree; its siblings remain.
        let docs = vec![
            doc("guide/intro", "Intro"),
            hidden_doc("guide/secret", "Secret"),
        ];
        let tree = build_tree(&docs);
        let children = match &tree[0] {
            TreeNode::Dir { children, .. } => children,
            other => panic!("expected dir, got {other:?}"),
        };
        assert_eq!(children.len(), 1);
        assert!(matches!(&children[0], TreeNode::Doc { slug, .. } if slug == "guide/intro"));
    }

    #[test]
    fn directory_emptied_by_hidden_children_is_pruned() {
        // Every page under `releases/` is hidden and there's no folder note, so the
        // whole `releases` group vanishes — while a sibling top-level doc remains.
        let docs = vec![
            hidden_doc("releases/v0-1-0", "0.1.0"),
            hidden_doc("releases/v0-2-0", "0.2.0"),
            doc("releases", "Releases"), // the `.base` page — a loose top-level doc
        ];
        let tree = build_tree(&docs);
        // No `releases` Dir node; only the loose `releases` Doc (the base page).
        assert_eq!(tree.len(), 1);
        assert!(matches!(&tree[0], TreeNode::Doc { slug, .. } if slug == "releases"));
    }

    #[test]
    fn hidden_folder_note_hides_whole_subtree() {
        // `sidebar: false` on a directory's `index.md` hides the entire subtree,
        // even pages that are not themselves flagged.
        let docs = vec![
            hidden_doc("archive/index", "Archive"),
            doc("archive/old", "Old"),
            doc("guide/intro", "Intro"),
        ];
        let tree = build_tree(&docs);
        assert_eq!(tree.len(), 1);
        assert!(matches!(&tree[0], TreeNode::Dir { name, .. } if name == "guide"));
    }

    #[test]
    fn root_index_stays_an_ordinary_doc() {
        // A top-level `index.md` is the site home, not a folder note — it must
        // remain a normal doc node (depth 0).
        let docs = vec![doc("index", "Home"), doc("guide/intro", "Intro")];
        let tree = build_tree(&docs);
        assert!(tree
            .iter()
            .any(|n| matches!(n, TreeNode::Doc { slug, .. } if slug == "index")));
    }
}
