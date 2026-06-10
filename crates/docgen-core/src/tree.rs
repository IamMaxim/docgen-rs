use std::collections::BTreeMap;

use crate::model::{Doc, TreeNode};

#[derive(Default)]
struct Builder {
    dirs: BTreeMap<String, Builder>,
    docs: BTreeMap<String, (String, String)>, // leaf name -> (slug, title)
    /// Folder note: an `index.md` directly inside this directory. Stored as its
    /// slug instead of being added to `docs`, so the folder itself links to it.
    note: Option<String>,
}

fn insert(node: &mut Builder, parts: &[&str], slug: &str, title: &str, depth: usize) {
    match parts {
        // An `index.md` *inside* a folder (depth > 0) is that folder's note, not a
        // child entry. A root-level `index.md` (depth 0) is the site home and stays
        // an ordinary doc.
        [leaf] if *leaf == "index" && depth > 0 => {
            node.note = Some(slug.to_string());
        }
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
            );
        }
        [] => {}
    }
}

fn to_nodes(builder: Builder) -> Vec<TreeNode> {
    let mut out = Vec::new();
    // Directories first (BTreeMap keeps them name-sorted), then loose docs.
    for (name, child) in builder.dirs {
        let slug = child.note.clone();
        out.push(TreeNode::Dir {
            name,
            slug,
            children: to_nodes(child),
        });
    }
    for (name, (slug, title)) in builder.docs {
        out.push(TreeNode::Doc { name, slug, title });
    }
    out
}

/// Build a name-sorted sidebar tree from documents, keyed off their slugs.
/// A folder's `index.md` becomes the folder's note (the folder links to it)
/// rather than a separate `index` child entry.
pub fn build_tree(docs: &[Doc]) -> Vec<TreeNode> {
    let mut root = Builder::default();
    for doc in docs {
        let parts: Vec<&str> = doc.slug.split('/').collect();
        insert(&mut root, &parts, &doc.slug, &doc.title, 0);
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
