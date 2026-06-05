//! THE path-traversal rejection test: every escape vector must be rejected and
//! the in-bounds case accepted.

use std::fs;

use docgen_server::{resolve_doc_path, PathGuardError};

#[test]
fn rejects_all_traversal_vectors() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let docs = root.join("docs");
    fs::create_dir_all(docs.join("guide")).unwrap();
    fs::write(docs.join("ok.md"), "# ok\n").unwrap();
    fs::write(docs.join("guide/intro.md"), "# intro\n").unwrap();
    // A secret OUTSIDE docs, plus a symlink inside docs that points at it.
    fs::write(root.join("secret.md"), "secret\n").unwrap();

    // Caller canonicalizes docs_dir.
    let docs = docs.canonicalize().unwrap();

    // In-bounds Ok cases resolve to canonical paths inside docs.
    let ok = resolve_doc_path(&docs, "guide/intro.md").unwrap();
    assert!(ok.starts_with(&docs));
    assert!(ok.ends_with("guide/intro.md"));
    resolve_doc_path(&docs, "./ok.md").unwrap();

    // `..` component.
    assert_eq!(
        resolve_doc_path(&docs, "../secret.md"),
        Err(PathGuardError::Traversal)
    );
    assert_eq!(
        resolve_doc_path(&docs, "../../etc/passwd"),
        Err(PathGuardError::Traversal)
    );

    // Absolute paths.
    assert_eq!(
        resolve_doc_path(&docs, "/etc/passwd"),
        Err(PathGuardError::Absolute)
    );
    let abs = docs.join("ok.md");
    assert_eq!(
        resolve_doc_path(&docs, abs.to_str().unwrap()),
        Err(PathGuardError::Absolute)
    );

    // Backslash.
    assert_eq!(
        resolve_doc_path(&docs, "guide\\..\\..\\x.md"),
        Err(PathGuardError::Traversal)
    );

    // Non-markdown.
    assert_eq!(
        resolve_doc_path(&docs, "ok.txt"),
        Err(PathGuardError::NotMarkdown)
    );

    // In-bounds but absent.
    assert_eq!(
        resolve_doc_path(&docs, "nope.md"),
        Err(PathGuardError::NotFound)
    );

    // A `.md` path whose target is actually a directory.
    fs::create_dir_all(docs.join("adir.md")).unwrap();
    assert_eq!(
        resolve_doc_path(&docs, "adir.md"),
        Err(PathGuardError::NotAFile)
    );

    // Symlink escape: docs/escape.md -> ../secret.md (realpath leaves docs).
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("secret.md"), docs.join("escape.md")).unwrap();
        assert_eq!(
            resolve_doc_path(&docs, "escape.md"),
            Err(PathGuardError::Traversal)
        );
    }
}
