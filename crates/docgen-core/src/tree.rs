use std::collections::BTreeMap;

use crate::model::{Doc, TreeNode};

#[derive(Default)]
struct Builder {
    dirs: BTreeMap<String, Builder>,
    docs: BTreeMap<String, (String, String)>, // leaf name -> (slug, title)
}

fn insert(node: &mut Builder, parts: &[&str], slug: &str, title: &str) {
    match parts {
        [leaf] => {
            node.docs.insert(leaf.to_string(), (slug.to_string(), title.to_string()));
        }
        [head, rest @ ..] => {
            insert(node.dirs.entry(head.to_string()).or_default(), rest, slug, title);
        }
        [] => {}
    }
}

fn to_nodes(builder: Builder) -> Vec<TreeNode> {
    let mut out = Vec::new();
    // Directories first (BTreeMap keeps them name-sorted), then loose docs.
    for (name, child) in builder.dirs {
        out.push(TreeNode::Dir { name, children: to_nodes(child) });
    }
    for (name, (slug, title)) in builder.docs {
        out.push(TreeNode::Doc { name, slug, title });
    }
    out
}

/// Build a name-sorted sidebar tree from documents, keyed off their slugs.
pub fn build_tree(docs: &[Doc]) -> Vec<TreeNode> {
    let mut root = Builder::default();
    for doc in docs {
        let parts: Vec<&str> = doc.slug.split('/').collect();
        insert(&mut root, &parts, &doc.slug, &doc.title);
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
            TreeNode::Dir { name, children } => {
                assert_eq!(name, "guide");
                assert_eq!(children.len(), 1);
                assert!(matches!(&children[0], TreeNode::Doc { slug, .. } if slug == "guide/intro"));
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
            TreeNode::Dir { name, children } if name == "a" => children,
            other => panic!("expected dir a, got {other:?}"),
        };
        let b = match &a[0] {
            TreeNode::Dir { name, children } if name == "b" => children,
            other => panic!("expected dir b, got {other:?}"),
        };
        assert!(matches!(&b[0], TreeNode::Doc { slug, .. } if slug == "a/b/c"));
    }
}
