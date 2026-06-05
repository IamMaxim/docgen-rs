//! Hermetic end-to-end test: a self-contained temp git repo as the project
//! root, run `docgen build`, assert the per-doc history pages are emitted and
//! that a non-git build skips history gracefully.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use git2::{Repository, Signature};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "docgen_history_cli_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn configure_local_user(repo: &Repository) {
    let mut cfg = repo.config().unwrap();
    cfg.set_str("user.name", "docgen test").unwrap();
    cfg.set_str("user.email", "test@example.com").unwrap();
}

fn write_and_commit(repo: &Repository, root: &Path, rel: &str, content: &str, subject: &str) {
    let abs = root.join(rel);
    fs::create_dir_all(abs.parent().unwrap()).unwrap();
    fs::write(&abs, content).unwrap();

    let mut index = repo.index().unwrap();
    index
        .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.update_all(["*"].iter(), None).unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let sig = Signature::now("docgen test", "test@example.com").unwrap();
    let parents = match repo.head().ok().and_then(|h| h.target()) {
        Some(oid) => vec![repo.find_commit(oid).unwrap()],
        None => vec![],
    };
    let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
    repo.commit(Some("HEAD"), &sig, &sig, subject, &tree, &parent_refs)
        .unwrap();
}

#[test]
fn build_emits_global_diff_workspace_for_docs_in_a_git_repo() {
    let tmp = unique_temp_dir("git");
    let repo = Repository::init(&tmp).unwrap();
    configure_local_user(&repo);
    write_and_commit(
        &repo,
        &tmp,
        "docs/guide/intro.md",
        "# Intro\n\nfirst body.\n",
        "add intro",
    );
    write_and_commit(
        &repo,
        &tmp,
        "docs/guide/intro.md",
        "# Intro\n\nsecond body.\n",
        "edit intro",
    );

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    // The global /diff workspace shell + its hydration assets.
    let diff = fs::read_to_string(tmp.join("dist/diff/index.html")).unwrap();
    assert!(diff.contains(r#"id="docgen-diff-root""#));
    assert!(diff.contains(r#"src="/islands/diff.js""#));
    assert!(tmp.join("dist/diff.css").exists());
    assert!(tmp.join("dist/islands/diff.js").exists());

    // timeline.json: both commits, summarized (no hunks), camelCase tree fields.
    let timeline = fs::read_to_string(tmp.join("dist/diff/timeline.json")).unwrap();
    assert!(timeline.contains("edit intro"));
    assert!(timeline.contains("add intro"));
    assert!(timeline.contains(r#""mode":"build-history""#));
    assert!(timeline.contains("addedLines"));
    assert!(!timeline.contains("added_lines"));

    // One revisions/<id>.json per commit, carrying the rendered block diff.
    let head_oid = repo.head().unwrap().target().unwrap().to_string();
    let rev = fs::read_to_string(tmp.join(format!("dist/diff/revisions/{head_oid}.json"))).unwrap();
    assert!(rev.contains("second body."));
    assert!(rev.contains(r#""blocks""#));
    assert!(rev.contains("<p>")); // rendered markdown HTML

    // The normal doc page links to the global diff (not a per-page history page).
    let page = fs::read_to_string(tmp.join("dist/guide/intro/index.html")).unwrap();
    assert!(page.contains(r#"href="/diff""#));
    assert!(!page.contains("/history"));
    assert!(!tmp.join("dist/guide/intro/history/index.html").exists());

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn build_in_non_git_dir_skips_history_without_error() {
    let tmp = unique_temp_dir("nogit");
    fs::create_dir_all(tmp.join("docs/guide")).unwrap();
    fs::write(tmp.join("docs/guide/intro.md"), "# Intro\n\nbody.\n").unwrap();

    let status = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(&tmp)
        .status()
        .unwrap();
    assert!(status.success());

    // Doc page built fine.
    let page = fs::read_to_string(tmp.join("dist/guide/intro/index.html")).unwrap();
    assert!(page.contains("<title>Intro</title>"));
    // No diff workspace emitted, and no diff link in the topbar.
    assert!(!tmp.join("dist/diff/index.html").exists());
    assert!(!tmp.join("dist/diff.css").exists());
    assert!(!page.contains(r#"href="/diff""#));

    let _ = fs::remove_dir_all(&tmp);
}
