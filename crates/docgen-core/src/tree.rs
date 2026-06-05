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
}
