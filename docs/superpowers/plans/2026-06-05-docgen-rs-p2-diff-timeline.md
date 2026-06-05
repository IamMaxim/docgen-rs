# docgen-rs P2: Git Diff Timeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. STRICT TDD throughout: write the failing test, run it RED, implement, run it GREEN, then commit the logical unit. Keep `cargo test` and `cargo clippy --all-targets` green before declaring any task done.

**Goal:** Port the existing SvelteKit/TypeScript doc-diff timeline (`~/work/docgen/packages/docgen/src/lib/diff`, ~1,814 LOC) to Rust for **behavior parity**, then wire it into the two-pass build so every doc gets a static **`/<slug>/history/` page** showing its commit timeline with line-level and block-level diffs. Git history is read with `git2`; the raw text diff is backed by the `similar` crate **only where it reproduces the originals' LCS line/block grouping behavior** (the originals roll their own LCS — we reproduce their exact op stream, see Cluster B).

**Architecture:** A new pure-logic crate **`docgen-diff`** owns the entire port. This keeps `libgit2` out of `docgen-core` (whose pipeline tests must stay fast and dependency-light) and mirrors the original's clean split between *pure diff algorithms* (no git) and the *git-driven orchestrator*. The crate has two layers:

- **Pure layer (no git):** `types`, `git_parsing` (name-status / numstat parsing), `git_refs`, `line_diff`, `block_diff`, `file_tree`, `timeline_groups`, `payloads`. These are direct, unit-tested ports of the corresponding `*.ts` files and depend on nothing but `serde`/`similar`. Every original `*.test.ts` becomes a Rust `#[test]`.
- **Git layer (`git2`):** `history` (walk commits touching a doc, read blobs at each revision, handle renames / first-commit / no-history) and `report` (the `git-diff.server.ts` orchestrator: assemble per-doc `DocDiffReport`s). Hermetic tests build a self-contained temp git repo.

`docgen-render` gains a history-page template + `render_history`. `docgen`'s `build.rs` calls into `docgen-diff` once per doc and emits `/<slug>/history/index.html`, gracefully no-opping when `docs/` is not in a git repo or a doc has no history.

**Why a per-doc `/<slug>/history/` page (not an inline section)?** (1) Keeps the main doc page lean — diffs can be large; (2) matches the original site's dedicated revision-navigation surface; (3) one self-contained static HTML file per doc, deploy-anywhere, no JS required (P2 is static-only; the Alpine navigation island is P3 and slots onto this same HTML later); (4) trivial graceful no-op — if a doc has no history we simply skip emitting its history page and omit the "History" link.

**Scope boundary vs. original:** The original supports a `dev-worktree` mode (uncommitted changes, untracked docs, symlink guards, env-var base/head refs, CI ref detection). **P2 implements only `build-history` mode** — per-doc commit history from `git2`. The `dev-worktree` path belongs to P5 (dev server); we port its pure helpers (`parseUntrackedDocs`, worktree-line counting) where free, but do not wire a worktree timeline point. This is called out in each affected task.

**Tech Stack:** Rust, `git2` (history/blobs), `similar` (raw text diff backing line/block LCS), `serde`/`serde_json` (payload serialization, parity with the TS JSON shapes), `chrono` (commit time → bucket labels), `comrak` (block HTML via `docgen-core::markdown`), `minijinja` (history template). Reuse `docgen-core::markdown::render_markdown` for block HTML so diff blocks render identically to doc bodies.

**Reference:**
- Spec: `docs/superpowers/specs/2026-06-05-docgen-rust-rewrite-design.md` (P2 row + pipeline step 4).
- Port source: `~/work/docgen/packages/docgen/src/lib/diff/{types,git-parsing,git-refs,line-diff,block-diff,timeline-groups,tree,file-tree,payloads,git-diff.server,markdown-render.server}.ts` and their `*.test.ts`.

---

## File Structure

```
docgen-rs/
  Cargo.toml                          # workspace: add "crates/docgen-diff"
  crates/
    docgen-diff/                      # NEW crate
      Cargo.toml
      src/
        lib.rs                        # module wiring + re-exports
        types.rs                      # DocDiff* structs/enums (port of types.ts)
        git_parsing.rs                # parse_name_status / parse_untracked_docs (git-parsing.ts)
        git_refs.rs                   # EMPTY_TREE_REF, base_ref_for_commit_parents (git-refs.ts)
        line_diff.rs                  # build_line_hunks (line-diff.ts)
        block_diff.rs                 # split_markdown_blocks, build_block_diff (block-diff.ts)
        file_tree.rs                  # build_file_tree (file-tree.ts)
        timeline_groups.rs            # ymd, format_date, bucket_label, group_timeline (timeline-groups.ts)
        payloads.rs                   # summarize_* (payloads.ts)
        history.rs                    # git2: DocHistory, commits touching a doc + blob text
        report.rs                     # orchestrator: build_doc_diff_report (git-diff.server.ts, build-history mode)
        testutil.rs                   # #[cfg(test)] hermetic temp-git-repo helper
      tests/
        history_git.rs                # integration: hermetic repo -> history + report parity
    docgen-render/
      src/lib.rs                      # + HistoryContext, render_history, HISTORY_CSS
      templates/
        history.html                  # NEW: static timeline page template
      assets/
        docgen.css                    # + diff/timeline styles (append)
    docgen/
      src/build.rs                    # + emit /<slug>/history/index.html
      tests/
        history_cli.rs                # NEW end-to-end: hermetic repo fixture -> dist history pages
  fixtures/
    site-basic/ ...                   # unchanged (build_cli.rs stays git-agnostic)
```

`docgen-core` is **unchanged** — its dependency surface stays minimal. `docgen-diff` depends on `docgen-core` only for `markdown::render_markdown` (block HTML). The CLI (`docgen`) depends on `docgen-diff`.

---

## Public API (stable across all three clusters)

These signatures are fixed up-front so Cluster B/C build on Cluster A without churn. All structs derive `Debug, Clone, PartialEq, Serialize` (and `Deserialize` where round-tripped in tests); enums also derive `Eq`. `serde` uses `#[serde(rename_all = "camelCase")]` and lowercase enum tags to match the TS JSON shapes exactly.

```rust
// ---------- types.rs (port of types.ts) ----------
#[serde(rename_all = "lowercase")] pub enum DocDiffLineKind { Context, Added, Removed }
#[serde(rename_all = "lowercase")] pub enum DocDiffBlockKind { Context, Added, Removed }
#[serde(rename_all = "lowercase")] pub enum DocDiffFileStatus { Added, Modified, Deleted, Renamed }
#[serde(rename_all = "lowercase")] pub enum DocDiffTimelinePointKind { Commit, Worktree }

pub struct DocDiffLine { pub kind: DocDiffLineKind, pub old_line: Option<u32>, pub new_line: Option<u32>, pub text: String }
pub struct DocDiffHunk { pub old_start: u32, pub old_lines: u32, pub new_start: u32, pub new_lines: u32, pub lines: Vec<DocDiffLine> }
pub struct DocDiffBlock { pub id: String, pub kind: DocDiffBlockKind, pub raw: String, pub html: String, pub old_index: Option<usize>, pub new_index: Option<usize> }
pub struct DocDiffFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub old_path: Option<String>,
    pub status: DocDiffFileStatus,
    pub added_lines: u32, pub removed_lines: u32,
    pub hunks: Vec<DocDiffHunk>,
    #[serde(skip_serializing_if = "Option::is_none")] pub blocks: Option<Vec<DocDiffBlock>>,
}
pub enum DocDiffFileTreeNode {  // #[serde(tag = "type", rename_all = "lowercase")]
    Group { id: String, label: String, children: Vec<DocDiffFileTreeNode> },
    File  { id: String, label: String, path: String, old_path: Option<String>, status: DocDiffFileStatus, added_lines: u32, removed_lines: u32 },
}
pub struct DocDiffTimelinePoint {
    pub id: String, pub kind: DocDiffTimelinePointKind,
    pub hash: Option<String>, pub short_hash: String, pub subject: String,
    pub author: Option<String>, pub date: Option<String>,   // date = RFC3339 string, parity with TS ISO
    pub base_ref: String, pub head_ref: String,
    pub files: Vec<DocDiffFile>, pub file_tree: Vec<DocDiffFileTreeNode>,
    pub total_added_lines: u32, pub total_removed_lines: u32, pub warnings: Vec<String>,
}
pub struct DocDiffReport {
    pub mode: String,                 // "build-history" in P2
    pub base_ref: String, pub head_ref: String, pub generated_at: String,
    pub timeline: Vec<DocDiffTimelinePoint>,
    pub selected_point_id: Option<String>, pub selected_file_path: Option<String>,
    pub files: Vec<DocDiffFile>,
    pub total_added_lines: u32, pub total_removed_lines: u32, pub warnings: Vec<String>,
}

// ---------- git_parsing.rs ----------
pub struct NameStatusEntry { pub status: DocDiffFileStatus, pub path: String, pub old_path: Option<String> }
pub fn parse_name_status(stdout: &str) -> Vec<NameStatusEntry>;
pub fn parse_untracked_docs(stdout: &str) -> Vec<String>;       // ported, used by P5

// ---------- git_refs.rs ----------
pub const EMPTY_TREE_REF: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
pub fn base_ref_for_commit_parents(hash: &str, parents: &str) -> String;

// ---------- line_diff.rs ----------
pub fn build_line_hunks(old_text: &str, new_text: &str, context_lines: usize) -> Vec<DocDiffHunk>;
// convenience matching the TS default of 3:
pub fn build_line_hunks_default(old_text: &str, new_text: &str) -> Vec<DocDiffHunk>; // context_lines = 3

// ---------- block_diff.rs ----------
pub fn split_markdown_blocks(markdown: &str) -> Vec<String>;
pub fn strip_invisible_document_parts(markdown: &str) -> String;
pub fn build_block_diff(old_markdown: &str, new_markdown: &str) -> Vec<DocDiffBlock>; // html = "" (filled later)

// ---------- file_tree.rs ----------
pub fn build_file_tree(files: &[DocDiffFile]) -> Vec<DocDiffFileTreeNode>;

// ---------- timeline_groups.rs ----------
pub fn ymd(y: i32, m: u32, d: u32) -> String;                 // zero-padded YYYY-MM-DD
pub fn format_date(value: Option<&str>) -> String;            // RFC3339 -> YYYY-MM-DD (local), "" on null/invalid
pub struct TimelineBucket { pub label: String, pub points: Vec<DocDiffTimelinePoint> }
pub fn bucket_label(point: &DocDiffTimelinePoint, now: DateTime<Local>) -> String;
pub fn group_timeline(points: Vec<DocDiffTimelinePoint>, now: DateTime<Local>) -> Vec<TimelineBucket>;

// ---------- payloads.rs ----------
pub fn summarize_file(file: &DocDiffFile) -> DocDiffFile;            // hunks=[], blocks=None
pub fn summarize_timeline_point(p: &DocDiffTimelinePoint) -> DocDiffTimelinePoint;
pub fn summarize_report(r: &DocDiffReport) -> DocDiffReport;

// ---------- history.rs (git2) ----------
pub struct CommitMeta { pub hash: String, pub short_hash: String, pub parents: Vec<String>,
                        pub author: Option<String>, pub date: Option<String>, pub subject: String }
pub struct RevisionContent { pub meta: CommitMeta, pub old_text: String, pub new_text: String,
                             pub status: DocDiffFileStatus, pub path: String, pub old_path: Option<String> }
/// All commits (newest-first) that touched `doc_rel_path` (relative to repo root, e.g. "docs/guide/intro.md"),
/// each paired with the file content at that revision and at its first parent. Rename-aware via diff find_similar.
pub fn doc_revisions(repo: &git2::Repository, doc_rel_path: &str, limit: usize) -> Result<Vec<RevisionContent>, DiffError>;
/// Open the repo that contains `path` (walks up). Ok(None) when `path` is not inside any git repo.
pub fn discover_repo(path: &std::path::Path) -> Result<Option<git2::Repository>, DiffError>;

// ---------- report.rs (orchestrator) ----------
/// Build the build-history report for one doc. `doc_rel_path` is relative to repo root.
/// Returns Ok(None) when the doc has no commit history (graceful no-op).
pub fn build_doc_diff_report(repo: &git2::Repository, doc_rel_path: &str, limit: usize)
    -> Result<Option<DocDiffReport>, DiffError>;

#[derive(Debug, thiserror::Error)] pub enum DiffError { #[error("git error: {0}")] Git(#[from] git2::Error), /* ... */ }
```

`docgen-render` additions:

```rust
pub const DEFAULT_HISTORY_TEMPLATE: &str = include_str!("../templates/history.html");
pub struct HistoryContext<'a> {
    pub title: &'a str,            // doc title
    pub slug: &'a str,
    pub tree: &'a [TreeNode],
    pub buckets: &'a [TimelineBucketView],   // render-friendly view (see Cluster C, Task C2)
}
impl Renderer { pub fn render_history(&self, ctx: &HistoryContext) -> Result<String, minijinja::Error>; }
```

---

## Cluster A — Git history extraction

> Pure-layer ports that the git layer needs, then the `git2` history walk. End state: given a hermetic repo and a doc path, we can list every commit that touched it (meta + content at each revision and its parent), rename-aware, first-commit-safe, no-history-safe.

### Task A1: Scaffold `docgen-diff` crate + `types.rs`

**Files:** create `crates/docgen-diff/Cargo.toml`, `src/lib.rs`, `src/types.rs`; edit root `Cargo.toml`.

- [ ] **Step 1 — workspace + deps.** Add `"crates/docgen-diff"` to root `Cargo.toml` `members`. Create the crate and add deps (current versions via `cargo add`):

```bash
cargo new --lib crates/docgen-diff
cargo add --package docgen-diff git2 similar chrono serde_json thiserror
cargo add --package docgen-diff serde --features derive
cargo add --package docgen-diff --path crates/docgen-core
```

Set `[package]` to inherit `edition/license/version` from the workspace (match the other crates). For `chrono`, disable default features if clippy flags unused (`chrono = { version = "...", default-features = false, features = ["clock"] }`) — keep `clock` for `Local::now`.

- [ ] **Step 2 — RED: write `types.rs` round-trip tests first.** The load-bearing parity property is the JSON shape (camelCase, lowercase tags, `oldPath`/`blocks` omitted when absent). Write tests that assert exact serialized JSON for representative values:

```rust
#[test]
fn line_kind_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&DocDiffLineKind::Added).unwrap(), r#""added""#);
}
#[test]
fn file_omits_old_path_and_blocks_when_absent() {
    let f = DocDiffFile { path: "docs/a.md".into(), old_path: None, status: DocDiffFileStatus::Modified,
        added_lines: 1, removed_lines: 2, hunks: vec![], blocks: None };
    let v = serde_json::to_string(&f).unwrap();
    assert!(!v.contains("oldPath")); assert!(!v.contains("blocks"));
    assert!(v.contains(r#""addedLines":1"#));
}
#[test]
fn file_tree_node_uses_type_tag() {
    let n = DocDiffFileTreeNode::Group { id: "docs/dev".into(), label: "dev".into(), children: vec![] };
    let v = serde_json::to_string(&n).unwrap();
    assert!(v.contains(r#""type":"group""#));
}
```

- [ ] **Step 3 — GREEN: define the structs/enums** exactly as in the Public API block above. Use `#[serde(rename_all = "camelCase")]` on structs; `#[serde(rename_all = "lowercase")]` on enums; `#[serde(tag = "type", rename_all = "lowercase")]` on `DocDiffFileTreeNode`; `#[serde(skip_serializing_if = "Option::is_none")]` on `old_path` and `blocks`. `DocDiffFileTreeNode::File.old_path` also gets `skip_serializing_if` to match `oldPath?`.

- [ ] **Step 4 — verify.** `cargo test -p docgen-diff types::` then `cargo clippy -p docgen-diff --all-targets`. **Expected:** all `types::` tests pass; clippy clean.

- [ ] **Commit:** `feat(diff): scaffold docgen-diff crate + DocDiff types with JSON parity`

### Task A2: `git_refs.rs` — base-ref selection

**Files:** create `crates/docgen-diff/src/git_refs.rs`; wire `pub mod git_refs;` in `lib.rs`.

- [ ] **Step 1 — RED.** Port `git-refs.test.ts` verbatim:

```rust
#[test] fn empty_tree_for_parentless() { assert_eq!(base_ref_for_commit_parents("abc123", ""), EMPTY_TREE_REF); }
#[test] fn first_parent_for_normal_and_merge() {
    assert_eq!(base_ref_for_commit_parents("abc123", "parent1"), "parent1");
    assert_eq!(base_ref_for_commit_parents("abc123", "parent1 parent2"), "parent1");
}
```

- [ ] **Step 2 — GREEN.** `EMPTY_TREE_REF` const; `base_ref_for_commit_parents` splits `parents` on ASCII whitespace, takes the first non-empty token, else `EMPTY_TREE_REF.to_string()`. The `_hash` arg is unused (parity; prefix `_`).

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff git_refs::` → green. Commit: `feat(diff): port git_refs base-ref selection`

### Task A3: `git_parsing.rs` — name-status & untracked parsing

**Files:** create `crates/docgen-diff/src/git_parsing.rs`; wire module.

- [ ] **Step 1 — RED.** Port all three `git-parsing.test.ts` cases:

```rust
#[test]
fn parses_added_modified_deleted_renamed() {
    assert_eq!(
        parse_name_status("A\tdocs/new.md\nM\tdocs/a.md\nD\tdocs/old.md\nR100\tdocs/from.md\tdocs/to.md\n"),
        vec![
            NameStatusEntry { status: Added,    path: "docs/new.md".into(), old_path: None },
            NameStatusEntry { status: Modified, path: "docs/a.md".into(),   old_path: None },
            NameStatusEntry { status: Deleted,  path: "docs/old.md".into(), old_path: None },
            NameStatusEntry { status: Renamed,  path: "docs/to.md".into(),  old_path: Some("docs/from.md".into()) },
        ]);
}
#[test]
fn one_sided_docs_renames_map_to_add_or_delete() {
    assert_eq!(
        parse_name_status("R100\tdocs/from.md\toutside/from.md\nR100\toutside/to.md\tdocs/to.md\n"),
        vec![
            NameStatusEntry { status: Deleted, path: "docs/from.md".into(), old_path: None },
            NameStatusEntry { status: Added,   path: "docs/to.md".into(),   old_path: None },
        ]);
}
#[test]
fn untracked_keeps_docs_md_and_svx() {
    assert_eq!(parse_untracked_docs("docs/a.md\ndocs/b.svx\nclient/nope.md\n"), vec!["docs/a.md", "docs/b.svx"]);
}
```

- [ ] **Step 2 — GREEN.** Faithful port of `git-parsing.ts`:
  - `is_doc_path(p)`: `p.starts_with("docs/") && (p.ends_with(".md") || p.ends_with(".svx"))`.
  - `parse_name_status`: split on `\n`, trim, drop empties, `flat_map(parse_line)`. `parse_line` splits on `\t` into `[status, first, second]`. `A`/`M`/`D` → `entry(status, first)` (empty vec unless `is_doc_path(first)`). `R*` (starts_with `'R'`) → both-doc → `Renamed{old_path:first, path:second}`; old-only → `Deleted{first}`; new-only → `Added{second}`; neither → `[]`.
  - `parse_untracked_docs`: split, trim, keep `is_doc_path`.
  - **Note:** `is_doc_path`'s `docs/` + `.md/.svx` filter is for the *worktree/P5* path. P2's `report.rs` reads a single known doc path, so it does not depend on this filter — but the port stays faithful for P5 reuse. Keep `.svx` support even though P2 emits only `.md` (forward-compat, matches original tests).

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff git_parsing::` → green; clippy clean. Commit: `feat(diff): port git name-status + untracked parsing`

### Task A4: `history.rs` — git2 commit walk + blob content (the core of Cluster A)

**Files:** create `crates/docgen-diff/src/history.rs`, `src/testutil.rs`; wire modules. Add `DiffError` (in `lib.rs` or a small `error.rs`).

- [ ] **Step 1 — hermetic git fixture helper (in `testutil.rs`, `#[cfg(test)]`).** This is the reusable temp-repo builder every git test uses. It must be fully self-contained (configure a local user, no global config dependence):

```rust
#![cfg(test)]
use std::path::{Path, PathBuf};
use git2::{Repository, Signature};

pub struct TempRepo { pub dir: PathBuf, pub repo: Repository }

impl TempRepo {
    /// Fresh empty repo in a unique temp dir with a local committer identity.
    pub fn init() -> Self {
        let dir = std::env::temp_dir().join(format!("docgen_diff_{}_{}",
            std::process::id(), unique_counter()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Repository::init(&dir).unwrap();
        // local identity so commits don't need global git config (hermetic)
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "docgen test").unwrap();
        cfg.set_str("user.email", "test@example.com").unwrap();
        TempRepo { dir, repo }
    }

    /// Write `content` to `rel` (relative to repo root, creating parent dirs),
    /// stage everything, commit with `subject`. Returns the commit oid hex.
    pub fn commit_file(&self, rel: &str, content: &str, subject: &str) -> String {
        let abs = self.dir.join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, content).unwrap();
        self.commit_all(subject)
    }

    /// Stage all changes (adds, modifies, deletes) and commit.
    pub fn commit_all(&self, subject: &str) -> String {
        let mut index = self.repo.index().unwrap();
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
        index.update_all(["*"].iter(), None).unwrap();   // pick up deletions
        index.write().unwrap();
        let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = Signature::now("docgen test", "test@example.com").unwrap();
        let parents = match self.repo.head().ok().and_then(|h| h.target()) {
            Some(oid) => vec![self.repo.find_commit(oid).unwrap()],
            None => vec![],
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        let oid = self.repo.commit(Some("HEAD"), &sig, &sig, subject, &tree, &parent_refs).unwrap();
        oid.to_string()
    }

    /// git mv: rename a tracked file on disk (commit separately).
    pub fn rename_file(&self, from: &str, to: &str) {
        let to_abs = self.dir.join(to);
        std::fs::create_dir_all(to_abs.parent().unwrap()).unwrap();
        std::fs::rename(self.dir.join(from), to_abs).unwrap();
    }
    pub fn delete_file(&self, rel: &str) { std::fs::remove_file(self.dir.join(rel)).unwrap(); }
}
impl Drop for TempRepo { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); } }

fn unique_counter() -> u64 { /* AtomicU64 fetch_add to avoid collisions within a process */ }
```

- [ ] **Step 2 — RED: `discover_repo` + `doc_revisions` tests** (in `history.rs` `#[cfg(test)]`, or `tests/history_git.rs` — keep the unit ones inline, integration in `tests/`):

```rust
#[test]
fn discover_repo_returns_none_outside_git() {
    let dir = std::env::temp_dir().join(format!("docgen_nogit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
    assert!(discover_repo(&dir).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doc_revisions_lists_commits_newest_first_with_content() {
    let r = TempRepo::init();
    r.commit_file("docs/a.md", "# A\nfirst\n", "add a");
    r.commit_file("docs/a.md", "# A\nsecond\n", "edit a");
    r.commit_file("docs/other.md", "x\n", "unrelated");   // must NOT appear

    let revs = doc_revisions(&r.repo, "docs/a.md", 50).unwrap();
    assert_eq!(revs.len(), 2);
    // newest-first
    assert_eq!(revs[0].meta.subject, "edit a");
    assert_eq!(revs[1].meta.subject, "add a");
    // content at each revision + its parent
    assert_eq!(revs[0].new_text, "# A\nsecond\n");
    assert_eq!(revs[0].old_text, "# A\nfirst\n");
    assert_eq!(revs[0].status, DocDiffFileStatus::Modified);
    // first commit: parentless -> empty old_text, status Added
    assert_eq!(revs[1].old_text, "");
    assert_eq!(revs[1].new_text, "# A\nfirst\n");
    assert_eq!(revs[1].status, DocDiffFileStatus::Added);
    // short_hash is a prefix of hash; parents recorded
    assert!(revs[0].meta.hash.starts_with(&revs[0].meta.short_hash));
    assert_eq!(revs[1].meta.parents.len(), 0);
}

#[test]
fn doc_revisions_follows_a_rename() {
    let r = TempRepo::init();
    r.commit_file("docs/old.md", "# Doc\nbody line\nmore\n", "create");
    r.rename_file("docs/old.md", "docs/new.md");
    r.commit_all("rename");
    // querying the new path sees the rename commit as Renamed (old_path set)
    let revs = doc_revisions(&r.repo, "docs/new.md", 50).unwrap();
    assert!(revs.iter().any(|rev| rev.status == DocDiffFileStatus::Renamed
        && rev.old_path.as_deref() == Some("docs/old.md")));
    // and the create commit is reachable through the rename (old_path history)
    assert!(revs.iter().any(|rev| rev.meta.subject == "create"));
}

#[test]
fn doc_revisions_empty_for_untouched_path() {
    let r = TempRepo::init();
    r.commit_file("docs/a.md", "x\n", "a");
    assert!(doc_revisions(&r.repo, "docs/ghost.md", 50).unwrap().is_empty());
}
```

- [ ] **Step 3 — GREEN: implement `discover_repo` + `doc_revisions`.**
  - `discover_repo(path)`: `git2::Repository::discover(path)` → map `Ok` to `Some`; map the `NotFound` error class (`e.code() == ErrorCode::NotFound`) to `Ok(None)`; other errors propagate. This is the graceful "not a git repo" path.
  - `doc_revisions(repo, path, limit)`:
    1. Build a `Revwalk`, `push_head()`, set `SORT_TIME | SORT_TOPOLOGICAL` so order is newest-first (parity with `git log` default for `recentDocCommits`).
    2. For each commit oid (cap at `limit`), get `commit` + its tree, and the first parent's tree (or `None`). Compute `repo.diff_tree_to_tree(parent_tree, commit_tree, opts)` with `DiffOptions::pathspec("docs/...")`? **No** — to follow renames we cannot pre-restrict the pathspec (a rename changes the path). Instead: enable `diff.find_similar` (`Diff::find_similar` with `DiffFindOptions::renames(true)`) on the full tree-diff, then scan deltas for one whose `new_file.path` equals `path` **or** whose `old_file.path` equals `path` (rename target/source). This matches the original's reliance on `git log -- <path>` with default rename following.
    3. **Track the path as it moves back in time:** start with the queried `path`; when a delta for the current commit is a rename whose `new_file.path == current_path`, record `status=Renamed`, `old_path=old_file.path`, then set `current_path = old_file.path` for older commits. (This reproduces `--follow` semantics. Document the limitation: only first-parent rename chains are followed; copy detection is off — same as the original.)
    4. Classify status from the delta: `Added`/`Modified`/`Deleted`/`Renamed` (`git2::Delta` → `DocDiffFileStatus`).
    5. Read content: `new_text` = blob at `commit_tree:current_new_path` (empty if delta is `Deleted`); `old_text` = blob at `parent_tree:current_old_path` (empty if no parent or `Added`). Use `tree.get_path(Path)` → `blob` → `String::from_utf8_lossy`. Skip non-UTF8 gracefully (lossy).
    6. `CommitMeta`: `hash` = oid hex; `short_hash` = `commit.as_object().short_id()?` string (libgit2 abbreviation, parity with `%h`); `parents` = parent oid hexes; `author` = `commit.author().name()` (None if empty); `date` = RFC3339 from `commit.time()` (see Task B5 for the chrono conversion); `subject` = first line of `commit.message()`.
    7. Only push a `RevisionContent` when the delta actually touched the tracked path; commits that didn't are skipped (parity with `entries.length === 0 → continue`).

  - `DiffError`: `#[from] git2::Error`, plus a `Utf8`/`NotFound` variant if needed.

- [ ] **Step 4 — verify.** `cargo test -p docgen-diff history::` and `cargo test -p docgen-diff --test history_git`. **Expected:** all four tests pass. `cargo clippy -p docgen-diff --all-targets` clean.

- [ ] **Commit:** `feat(diff): git2 doc history walk (renames, first-commit, no-history)`

---

## Cluster B — Diff algorithms + timeline grouping

> Pure ports of `line-diff.ts`, `block-diff.ts`, `file-tree.ts`, `timeline-groups.ts`, `payloads.ts`. Each original `*.test.ts` becomes a Rust test with the **exact same expected values**. We use `similar` only where it reproduces the originals' op stream; the originals' LCS tie-breaking (`lcs[i+1][j] >= lcs[i][j+1]` ⇒ prefer *removed*) is load-bearing for the expected outputs, so we port that LCS directly (see note in B1).

### Task B1: `line_diff.rs` — `build_line_hunks`

**Files:** create `crates/docgen-diff/src/line_diff.rs`; wire module.

- [ ] **Step 1 — RED.** Port `line-diff.test.ts` exactly (three cases). Critical: the second case fixes the op order `context, removed, added, context` and the line numbers; the third uses `context_lines = 1` and asserts **two separate hunks** with exact `old_start`/`old_lines`/`new_start`/`new_lines`.

```rust
#[test] fn identical_returns_no_hunks() {
    assert!(build_line_hunks_default("alpha\nbeta\ngamma", "alpha\nbeta\ngamma").is_empty());
}
#[test] fn replacement_marks_context_removed_added_context() {
    let h = build_line_hunks_default("alpha\nbeta\ngamma", "alpha\ndelta\ngamma");
    assert_eq!(h, vec![DocDiffHunk {
        old_start: 1, old_lines: 3, new_start: 1, new_lines: 3,
        lines: vec![
            line(Context, Some(1), Some(1), "alpha"),
            line(Removed, Some(2), None,    "beta"),
            line(Added,   None,    Some(2), "delta"),
            line(Context, Some(3), Some(3), "gamma"),
        ]}]);
}
#[test] fn distant_edits_split_into_two_hunks_with_context_one() {
    let h = build_line_hunks("a\nb\nc\nd\ne\nf\ng", "a\nB\nc\nd\ne\nF\ng", 1);
    // two hunks: (oldStart 1) and (oldStart 5), each context/removed/added/context (see line-diff.test.ts)
    assert_eq!(h.len(), 2);
    assert_eq!(h[0].old_start, 1); assert_eq!(h[1].old_start, 5);
    // ... full structural assert mirroring the TS expected array ...
}
```

- [ ] **Step 2 — GREEN. Two acceptable implementations; pick (a) for guaranteed parity:**
  - **(a) Direct LCS port (recommended).** Port `splitLines`, `buildLcsTable` (bottom-up, fill from the end), `buildDiffOps` (the exact `while` loop with the `lcs[i+1][j] >= lcs[i][j+1]` tie-break), `buildHunkRanges` (context expansion + merge when `start <= prev.end + 1`), `buildHunk`, `hunkStart`. This reproduces the expected arrays byte-for-byte. `oldLine`/`newLine` are 1-based; `null` ⇒ `None`.
  - **(b) `similar`-backed.** Use `similar::TextDiff::from_lines` to get the op stream, then apply the *same* `buildHunkRanges`/`buildHunk` grouping. **Only choose (b) if** its op order matches (a) on all three tests; `similar`'s Myers may tie-break differently on the `beta`→`delta` replacement, producing `added` before `removed`. If any test diverges, fall back to (a). **The tests are the gate** — implementation choice is free as long as they pass.
  - `splitLines` parity detail: strip a trailing `\r` per line, and drop a single trailing empty line (so `"a\n"` → `["a"]`, `""` → `[]`).

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff line_diff::` → all green. Commit: `feat(diff): port line-level hunk diff (LCS + context grouping)`

### Task B2: `block_diff.rs` — block segmentation + block diff

**Files:** create `crates/docgen-diff/src/block_diff.rs`; wire module.

- [ ] **Step 1 — RED.** Port all three `block-diff.test.ts` cases:
  - `split_markdown_blocks` preserves fenced-code and table blocks (exact expected vec).
  - `build_block_diff` returns the full stream with `context/removed/added` kinds + `raw` text (frontmatter & `<script>` stripped). Assert the `(kind, raw)` projection exactly as the TS test does.

```rust
#[test]
fn split_preserves_fenced_code_and_tables() {
    assert_eq!(
        split_markdown_blocks("# Title\n\nPara one.\n\n```rust\nfn main() {}\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n"),
        vec!["# Title", "Para one.", "```rust\nfn main() {}\n```", "| A | B |\n| - | - |\n| 1 | 2 |"]);
}
#[test]
fn block_diff_full_stream_added_removed_context() {
    let blocks = build_block_diff(
        "# Title\n\nSame paragraph.\n\nOld paragraph.\n\nTail paragraph.\n",
        "# Title\n\nSame paragraph.\n\nNew paragraph.\n\nTail paragraph.\n");
    let proj: Vec<_> = blocks.iter().map(|b| (&b.kind, b.raw.as_str())).collect();
    assert_eq!(proj, vec![
        (&Context, "# Title"), (&Context, "Same paragraph."),
        (&Removed, "Old paragraph."), (&Added, "New paragraph."), (&Context, "Tail paragraph.")]);
}
#[test]
fn block_diff_strips_frontmatter_and_script() {
    let blocks = build_block_diff(
        "---\ntitle: Old\n---\n\n<script>const hidden = true;</script>\n\nVisible old.",
        "---\ntitle: New\n---\n\n<script>const hidden = false;</script>\n\nVisible new.");
    let proj: Vec<_> = blocks.iter().map(|b| (&b.kind, b.raw.as_str())).collect();
    assert_eq!(proj, vec![(&Removed, "Visible old."), (&Added, "Visible new.")]);
}
```

- [ ] **Step 2 — GREEN.** Faithful port:
  - `strip_invisible_document_parts`: remove a leading frontmatter block (`^---...---\s*`), remove all `<script>...</script>` (case-insensitive), then `trim`. Use the `regex` crate (add it: `cargo add --package docgen-diff regex`) for these three patterns; the block-segmentation line classifiers below can be plain string checks to avoid per-line regex cost, but keep regex where the TS used it for clarity. **Subtlety:** JS `replace(/^---[\s\S]*?---\s*/, '')` is non-greedy and anchored at start — use `(?s)^---.*?---\s*` with `Regex::replace` (first match only). Verify against test 3.
  - `split_markdown_blocks`: port the `while` loop classifying fenced code (` ``` `), tables (`|`), ATX headings (`#{1,6} `), blockquotes (`>`), lists (`-*+`/ordered, with continuation), block-level HTML (`<[A-Z]`), and paragraph runs (until blank line). `trim_block` = join with `\n` then trim trailing whitespace. Final filter drops blank blocks. The list-continuation regex `^(\s{0,3}[-*+]\s+|\s{0,3}\d+\.\s+|\s{2,}\S)` and the heading/quote checks must match the TS precisely — port them as `regex::Regex` (lazily compiled via `once_cell`/`OnceLock`) for fidelity.
  - `normalize_block(b)` = `b.trim()` then collapse all whitespace runs to a single space (port `replace(/\s+/g, ' ')`).
  - `build_block_diff`: split both sides, `normalize_block` each, run the **same LCS as line-diff** over the normalized vectors with the identical tie-break, emit `DocDiffBlock { id: format!("block-{i}"), kind, raw, html: String::new(), old_index, new_index }`. `raw` is the **un-normalized** block from the side that owns it (new side for context/added, old side for removed) — match the TS (`raw: newBlocks[newIndex]` for context/added, `oldBlocks[oldIndex]` for removed).

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff block_diff::` → green. Commit: `feat(diff): port markdown block segmentation + block diff`

### Task B3: `file_tree.rs` — group changed files by path

**Files:** create `crates/docgen-diff/src/file_tree.rs`; wire module.

- [ ] **Step 1 — RED.** Port `file-tree.test.ts`: two modified files under `docs/dev` and `docs/game-design` produce two groups, each with one file leaf carrying `status/added_lines/removed_lines`. Assert the full `Vec<DocDiffFileTreeNode>` equals the expected nested structure (ids `docs/dev`, labels stripped of the `docs/` prefix, file ids = full path).

- [ ] **Step 2 — GREEN.** Port `buildFileTree`: sort files by `path` (lexicographic, `localeCompare` ≈ `str::cmp` for these ASCII paths — note: `localeCompare` is locale-aware; document that we use byte ordering, which matches the test inputs). Split path on `/`, drop a leading `docs` segment for display, file name = last display segment, group segments = the rest. Walk/create `Group` nodes keyed by cumulative `docs/<...>` id; push a `File` leaf. `old_path` only set when present.

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff file_tree::` → green. Commit: `feat(diff): port changed-file tree grouping`

### Task B4: `payloads.rs` — summary trimming

**Files:** create `crates/docgen-diff/src/payloads.rs`; wire module.

- [ ] **Step 1 — RED.** Port `payloads.test.ts`: `summarize_timeline_point` on a full point yields files with `hunks == []` and `blocks == None`, preserving `file_tree` (group preserved). Build the full point fixture in-test (mirror `fullPoint`).

```rust
#[test]
fn summarize_strips_hunks_and_blocks() {
    let summary = summarize_timeline_point(&full_point());
    assert_eq!(summary.files.len(), 1);
    assert!(summary.files[0].hunks.is_empty());
    assert!(summary.files[0].blocks.is_none());
    assert!(matches!(summary.file_tree[0], DocDiffFileTreeNode::Group { .. }));
}
```

- [ ] **Step 2 — GREEN.** `summarize_file`: clone with `hunks = vec![]`, `blocks = None`. `summarize_timeline_point`: clone, map files through `summarize_file`. `summarize_report`: map both `timeline` (each point's files) and top-level `files`. These produce the lightweight JSON the history page ships as data (P3 island reads it).

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff payloads::` → green. Commit: `feat(diff): port payload summarization`

### Task B5: `timeline_groups.rs` — date bucketing

**Files:** create `crates/docgen-diff/src/timeline_groups.rs`; wire module. Uses `chrono`.

- [ ] **Step 1 — RED.** Port `timeline-groups.test.ts`. Adapt the JS `Date`-based tests to `chrono::Local`:
  - `ymd(2026, 1, 5) == "2026-01-05"`, `ymd(2026, 12, 31) == "2026-12-31"` (note: our `ymd` takes y/m/d ints, not a Date — the JS `ymd(new Date(2026,0,5))` used 0-based months; our API is 1-based and documented).
  - `format_date(None) == ""`, `format_date(Some("not-a-date")) == ""`, `format_date(Some("2026-03-15T12:00:00Z"))` matches `^2026-03-(14|15)$` (timezone-dependent — assert with a regex/`starts_with("2026-03-1")` and length 10).
  - `bucket_label` for a `worktree` point → `"Working tree"`; for commit points dated today/yesterday/earlier relative to a fixed `now` → `"Today"`/`"Yesterday"`/`"Earlier"`. Build `now` via `Local.with_ymd_and_hms(2026,5,15,12,0,0)`.
  - `group_timeline` preserves order and groups by label: `[worktree, today, today, earlier]` → `[("Working tree",[wt]),("Today",[a,b]),("Earlier",[c])]`.

```rust
#[test]
fn buckets_today_yesterday_earlier() {
    let now = Local.with_ymd_and_hms(2026,5,15,12,0,0).unwrap();
    let today    = Local.with_ymd_and_hms(2026,5,15,8,0,0).unwrap().to_rfc3339();
    let yesterday= Local.with_ymd_and_hms(2026,5,14,8,0,0).unwrap().to_rfc3339();
    let earlier  = Local.with_ymd_and_hms(2026,5, 1,8,0,0).unwrap().to_rfc3339();
    assert_eq!(bucket_label(&point_dated(&today), now),     "Today");
    assert_eq!(bucket_label(&point_dated(&yesterday), now), "Yesterday");
    assert_eq!(bucket_label(&point_dated(&earlier), now),   "Earlier");
}
```

- [ ] **Step 2 — GREEN.**
  - `ymd(y,m,d)` = `format!("{:04}-{:02}-{:02}", y, m, d)`.
  - `format_date(Some(s))`: parse `s` as RFC3339 (`DateTime::parse_from_rfc3339`), convert to `Local`, format `ymd(date.year(), date.month(), date.day())`; on parse failure or `None` → `""`. This mirrors `new Date(value)` + `ymd` (local-timezone), so the `(14|15)` ambiguity is genuinely TZ-dependent — same as the JS test.
  - `bucket_label(point, now)`: `worktree` kind → `"Working tree"`. Else compute `today = ymd(now)`, `yesterday = ymd(now - 1 day)`, `day = format_date(point.date.as_deref())` (already local `ymd`). Return `"Today"`/`"Yesterday"`/`"Earlier"`.
  - `group_timeline(points, now)`: iterate, find-or-push a `TimelineBucket` by label, preserving first-seen order. (Move semantics: take ownership of points, push into the matching bucket.)
  - Add a `#[cfg(test)]` `point_dated`/`point_worktree` constructor mirroring the TS `point(...)` factory.

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-diff timeline_groups::` → green; clippy clean. Commit: `feat(diff): port timeline date bucketing`

### Task B6: `report.rs` — orchestrator (build-history mode)

**Files:** create `crates/docgen-diff/src/report.rs`; wire module + top-level re-exports in `lib.rs`.

This ties Cluster A's `history` to Cluster B's diff/grouping — the Rust analogue of `git-diff.server.ts`'s `loadDocDiffTimelineReport` + `buildTimelinePoint`, restricted to one doc and build-history mode.

- [ ] **Step 1 — RED (hermetic, in `tests/history_git.rs`).**

```rust
#[test]
fn report_builds_timeline_for_a_doc_with_hunks_and_blocks() {
    let r = TempRepo::init();
    r.commit_file("docs/a.md", "# A\n\nfirst paragraph.\n", "add a");
    r.commit_file("docs/a.md", "# A\n\nsecond paragraph.\n", "edit a");

    let report = build_doc_diff_report(&r.repo, "docs/a.md", 50).unwrap().unwrap();
    assert_eq!(report.mode, "build-history");
    assert_eq!(report.timeline.len(), 2);               // two commits touched the doc
    let head = &report.timeline[0];                      // newest = "edit a"
    assert_eq!(head.subject, "edit a");
    assert_eq!(head.kind, DocDiffTimelinePointKind::Commit);
    let file = &head.files[0];
    assert_eq!(file.path, "docs/a.md");
    assert!(!file.hunks.is_empty());                     // line diff present
    let blocks = file.blocks.as_ref().unwrap();
    assert!(blocks.iter().any(|b| b.kind == DocDiffBlockKind::Removed && b.raw == "first paragraph."));
    assert!(blocks.iter().any(|b| b.kind == DocDiffBlockKind::Added   && b.raw == "second paragraph."));
    // block html populated via docgen-core markdown
    assert!(blocks.iter().any(|b| b.html.contains("<p>")));
    // totals + file tree
    assert!(head.total_added_lines >= 1 && head.total_removed_lines >= 1);
    assert!(!head.file_tree.is_empty());
    // first commit (oldest) is an Added file with empty old side
    assert_eq!(report.timeline[1].files[0].status, DocDiffFileStatus::Added);
}

#[test]
fn report_is_none_when_doc_has_no_history() {
    let r = TempRepo::init();
    r.commit_file("docs/a.md", "x\n", "a");
    assert!(build_doc_diff_report(&r.repo, "docs/ghost.md", 50).unwrap().is_none());
}
```

- [ ] **Step 2 — GREEN: implement `build_doc_diff_report`.**
  1. `let revs = history::doc_revisions(repo, doc_rel_path, limit)?;` If empty → `Ok(None)` (graceful no-op).
  2. For each `RevisionContent`, build one `DocDiffTimelinePoint`:
     - `hunks = line_diff::build_line_hunks_default(&rev.old_text, &rev.new_text)`.
     - `blocks = block_diff::build_block_diff(&rev.old_text, &rev.new_text)`, then fill each `block.html = docgen_core::markdown::render_markdown(&block.raw)` (reuses the shared comrak+syntect pipeline → diff blocks render identically to doc bodies; this is the Rust replacement for `renderMarkdownBlock`). **Skip the file** if `hunks.is_empty() && blocks.iter().all(|b| b.kind == Context)` (parity with the TS `return null`). For build-history each point is a single doc file, so an all-context point is dropped entirely — but renames/first-commits always have content, so this mainly guards no-op commits.
     - `added_lines`/`removed_lines` = count `Added`/`Removed` lines across `hunks` (parity with `countLines`).
     - `DocDiffFile { path, old_path, status, added_lines, removed_lines, hunks, blocks: Some(blocks) }`.
     - `file_tree = file_tree::build_file_tree(&files)`; totals = sum over files; `base_ref = git_refs::base_ref_for_commit_parents(&meta.hash, &meta.parents.join(" "))`; `head_ref = meta.hash`; `id = meta.hash`; `short_hash`, `subject`, `author`, `date` from `meta`; `kind = Commit`; `warnings = vec![]`.
  3. Assemble `DocDiffReport` (the Rust analogue of `reportFromTimeline`): `mode = "build-history"`, `selected_point = timeline.first()`, `base_ref`/`head_ref` from the selected point (fallback `EMPTY_TREE_REF`/`"HEAD"`), `generated_at = Local::now().to_rfc3339()`, `selected_point_id`, `selected_file_path`, top-level `files`/totals from the selected point, `warnings = vec![]`.

- [ ] **Step 3 — verify.** `cargo test -p docgen-diff --test history_git` and `cargo test -p docgen-diff`. **Expected:** all report + history tests pass. `cargo clippy -p docgen-diff --all-targets` clean.

- [ ] **Commit:** `feat(diff): orchestrate build-history DocDiffReport per doc`

### Task B7: `lib.rs` re-exports + crate-level green gate

- [ ] **Step 1.** Re-export the public surface from `lib.rs`: `pub use types::*; pub use report::{build_doc_diff_report, DocDiffReport}; pub use history::{discover_repo, doc_revisions}; pub use timeline_groups::{group_timeline, bucket_label, TimelineBucket};` etc. Keep module decls `pub mod`.
- [ ] **Step 2 — verify.** `cargo test -p docgen-diff` (all unit + integration green), `cargo clippy -p docgen-diff --all-targets -- -D warnings` clean, `cargo fmt --check`.
- [ ] **Commit:** `chore(diff): re-export public API + crate green`

---

## Cluster C — Render + build wiring + hermetic CLI tests

> Turn a `DocDiffReport` into a static `/<slug>/history/` page and wire it into `build.rs`, gracefully no-opping when there's no git / no history. Hermetic end-to-end test builds a temp git repo and asserts the emitted HTML.

### Task C1: `docgen-render` history template + `render_history`

**Files:** create `crates/docgen-render/templates/history.html`; edit `crates/docgen-render/src/lib.rs`, `crates/docgen-render/assets/docgen.css`. Add `chrono` + `docgen-diff` dep to `docgen-render`? **No** — keep render git-free. Instead, `docgen-render` defines a **render-friendly view model** (`TimelineBucketView`) that the CLI fills from `docgen-diff` types. This keeps `docgen-render`'s only domain dep as `docgen-core` (parity with current design).

- [ ] **Step 1 — RED.** Add render tests in `lib.rs` `#[cfg(test)]`:

```rust
#[test]
fn renders_history_timeline_with_buckets_and_diff_lines() {
    let buckets = vec![TimelineBucketView {
        label: "Today".into(),
        points: vec![TimelinePointView {
            short_hash: "abc1234".into(), subject: "edit a".into(),
            author: Some("docgen test".into()), date: Some("2026-05-15".into()),
            added_lines: 1, removed_lines: 1,
            files: vec![FileView {
                path: "docs/a.md".into(), status: "modified".into(),
                hunks: vec![HunkView { lines: vec![
                    LineView { kind: "context".into(), text: "# A".into(), old_line: Some(1), new_line: Some(1) },
                    LineView { kind: "removed".into(), text: "first".into(),  old_line: Some(2), new_line: None },
                    LineView { kind: "added".into(),   text: "second".into(), old_line: None,    new_line: Some(2) },
                ]}],
            }],
        }],
    }];
    let html = Renderer::new(DEFAULT_HISTORY_TEMPLATE).unwrap()
        .render_history(&HistoryContext { title: "A", slug: "a", tree: &[], buckets: &buckets }).unwrap();
    assert!(html.contains("<title>History: A</title>"));
    assert!(html.contains("Today"));
    assert!(html.contains("edit a"));
    assert!(html.contains("abc1234"));
    assert!(html.contains("docgen-diff-line--removed"));
    assert!(html.contains("docgen-diff-line--added"));
    assert!(html.contains("first"));    // diff text escaped but present
    assert!(html.contains(r#"href="/a""#));   // back-to-doc link
}
#[test]
fn history_escapes_diff_text() {
    // a line containing <script> must be escaped in output
    // ... assert html.contains("&lt;script&gt;") and not "<script>"
}
```

- [ ] **Step 2 — GREEN.** Define the view structs in `lib.rs` (all `Serialize`):

```rust
pub struct LineView { pub kind: String, pub text: String, pub old_line: Option<u32>, pub new_line: Option<u32> }
pub struct HunkView { pub lines: Vec<LineView> }
pub struct FileView { pub path: String, pub status: String, pub hunks: Vec<HunkView> }
pub struct TimelinePointView { pub short_hash: String, pub subject: String, pub author: Option<String>,
                               pub date: Option<String>, pub added_lines: u32, pub removed_lines: u32, pub files: Vec<FileView> }
pub struct TimelineBucketView { pub label: String, pub points: Vec<TimelinePointView> }
pub struct HistoryContext<'a> { pub title: &'a str, pub slug: &'a str, pub tree: &'a [TreeNode], pub buckets: &'a [TimelineBucketView] }
pub const DEFAULT_HISTORY_TEMPLATE: &str = include_str!("../templates/history.html");
impl Renderer { pub fn render_history(&self, ctx: &HistoryContext) -> Result<String, minijinja::Error> { /* register history.html, render */ } }
```

Register `history.html` in `Renderer::new` alongside `page.html` (rename the constructor to register both, or add a second `add_template_owned`). `history.html` (escapes by default; diff text rendered via `{{ line.text }}` so it's HTML-escaped — diffs are source text, never raw HTML):

```html
<!DOCTYPE html><html lang="en"><head><meta charset="utf-8"/>
<meta name="viewport" content="width=device-width, initial-scale=1"/>
<title>History: {{ title }}</title><link rel="stylesheet" href="/docgen.css"/></head>
<body>
  <nav class="docgen-sidebar">{# reuse the same render_nodes macro as page.html #}...</nav>
  <main class="docgen-content docgen-history">
    <p><a href="/{{ slug | safe }}">&larr; Back to {{ title }}</a></p>
    <h1>History: {{ title }}</h1>
    {% for bucket in buckets %}
      <section class="docgen-timeline-bucket">
        <h2>{{ bucket.label }}</h2>
        {% for p in bucket.points %}
          <article class="docgen-commit">
            <header><code class="docgen-commit__hash">{{ p.short_hash }}</code>
              <span class="docgen-commit__subject">{{ p.subject }}</span>
              {% if p.author %}<span class="docgen-commit__author">{{ p.author }}</span>{% endif %}
              {% if p.date %}<time>{{ p.date }}</time>{% endif %}
              <span class="docgen-commit__stat">+{{ p.added_lines }} −{{ p.removed_lines }}</span>
            </header>
            {% for f in p.files %}
              <div class="docgen-diff-file">
                <div class="docgen-diff-file__path docgen-diff-file--{{ f.status }}">{{ f.path }}</div>
                {% for h in f.hunks %}<pre class="docgen-diff-hunk">{% for line in h.lines %}<span class="docgen-diff-line docgen-diff-line--{{ line.kind }}">{{ line.text }}</span>
{% endfor %}</pre>{% endfor %}
              </div>
            {% endfor %}
          </article>
        {% endfor %}
      </section>
    {% endfor %}
  </main>
</body></html>
```

Append diff/timeline styles to `assets/docgen.css` (`.docgen-diff-line--added`, `--removed`, `--context`, bucket/commit layout). Add a render test asserting `DOCGEN_CSS.contains("docgen-diff-line--added")`.

- [ ] **Step 3 — verify + commit.** `cargo test -p docgen-render` → green; clippy clean. Commit: `feat(render): static history timeline page + diff styles`

### Task C2: `build.rs` — emit `/<slug>/history/` pages

**Files:** edit `crates/docgen/src/build.rs`; add `docgen-diff` + `chrono` deps to `crates/docgen/Cargo.toml` (`cargo add --package docgen docgen-diff chrono`).

- [ ] **Step 1 — design the mapping helper (pure, unit-testable).** Add a private `fn report_to_buckets(report: &DocDiffReport, now: DateTime<Local>) -> Vec<TimelineBucketView>` in `build.rs` (or a small `crates/docgen/src/history.rs`). It calls `docgen_diff::group_timeline(report.timeline.clone(), now)` then maps each `DocDiffTimelinePoint` → `TimelinePointView` and each `DocDiffFile`/`DocDiffHunk`/`DocDiffLine` → the render view structs, stringifying enums (`status`/`kind`) to their lowercase names and formatting `date` via `docgen_diff::format_date(point.date.as_deref())`. Unit-test this mapping in isolation (no git).

- [ ] **Step 2 — RED: extend `build()` + add hermetic CLI test (`tests/history_cli.rs`).** The CLI test builds a self-contained git repo *as the project root*, runs `docgen build`, and asserts the history page exists and contains the diff:

```rust
// tests/history_cli.rs  (uses git2 as a dev-dependency of the docgen crate)
#[test]
fn build_emits_history_pages_for_docs_in_a_git_repo() {
    let tmp = unique_temp_dir();
    let repo = git2::Repository::init(&tmp).unwrap();
    configure_local_user(&repo);
    write_and_commit(&repo, &tmp, "docs/guide/intro.md", "# Intro\n\nfirst body.\n", "add intro");
    write_and_commit(&repo, &tmp, "docs/guide/intro.md", "# Intro\n\nsecond body.\n", "edit intro");

    let status = Command::new(env!("CARGO_BIN_EXE_docgen")).arg("build").arg(&tmp).status().unwrap();
    assert!(status.success());

    let hist = fs::read_to_string(tmp.join("dist/guide/intro/history/index.html")).unwrap();
    assert!(hist.contains("History: Introduction") || hist.contains("History: intro"));
    assert!(hist.contains("edit intro"));
    assert!(hist.contains("add intro"));
    assert!(hist.contains("docgen-diff-line--removed"));
    assert!(hist.contains("first body."));
    assert!(hist.contains(r#"href="/guide/intro""#));   // back link

    // the normal doc page still builds and links to history
    let page = fs::read_to_string(tmp.join("dist/guide/intro/index.html")).unwrap();
    assert!(page.contains(r#"href="/guide/intro/history""#));   // History nav link
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn build_in_non_git_dir_skips_history_without_error() {
    // copy the site-basic fixture into a plain (non-git) temp dir, build, assert success
    // and that NO history/index.html files were written, and pages omit the History link.
    ...
}
```

- [ ] **Step 3 — GREEN: wire it into `build()`.** After the existing page loop:
  1. `let repo = docgen_diff::discover_repo(&docs_dir)?;` (open the repo containing `docs/`). If `None` → skip the whole history phase (the `build_in_non_git_dir` test). Set a `history_enabled` bool used by the page template to conditionally emit the History nav link.
  2. Compute the doc's repo-root-relative path: `git2::Repository::discover` gives `repo.workdir()`; for each doc, `doc_repo_path = relative(docs_dir, workdir) + "/" + doc.rel_path` — i.e. the doc path as git sees it (e.g. `docs/guide/intro.md`). Derive it from `docs_dir.strip_prefix(workdir)` joined with `doc.rel_path`, normalized to `/`.
  3. `if let Some(report) = docgen_diff::build_doc_diff_report(&repo, &doc_repo_path, limit)? { let buckets = report_to_buckets(&report, Local::now()); let html = renderer.render_history(&HistoryContext { title:&doc.title, slug:&doc.slug, tree:&tree, buckets:&buckets })?; let out = dist_dir.join(&doc.slug).join("history"); fs::create_dir_all(&out)?; fs::write(out.join("index.html"), html)?; }` Else (no history) skip that doc's history page.
  4. `limit`: default 50 (parity with `diffLimit`'s default), capped 200. Read from an optional `DOC_DIFF_LIMIT` env var for parity, else 50.
  5. **Page template History link:** add to `page.html` a conditional nav link `{% if history_enabled %}<a href="/{{ slug }}/history">History</a>{% endif %}`, and pass `slug` + `history_enabled` into `PageContext`/`render_page`. Only emit the link for docs that actually got a history page — pass a per-doc `has_history: bool` instead of a global flag (set it from whether `build_doc_diff_report` returned `Some`). Update `render_page` + `PageContext` accordingly and adjust the existing `build_cli.rs` expectations (the non-git fixture build sets `has_history=false`, so no link — keeps `build_cli.rs` green).

- [ ] **Step 4 — verify.** Run:
  ```
  cargo test -p docgen --test history_cli
  cargo test                      # whole workspace, incl. build_cli.rs unchanged-green
  cargo clippy --all-targets -- -D warnings
  ```
  **Expected:** new history CLI tests pass; `build_cli.rs` still green (its fixture dir is non-git → no history pages, no History link); clippy clean. Manually eyeball one emitted `dist/.../history/index.html` to confirm structure.

- [ ] **Commit:** `feat(cli): emit per-doc /history pages from git timeline; skip when no git/history`

### Task C3: Docs + final green gate

**Files:** edit root `README.md` (P2 section); optionally `crates/docgen-diff/README.md`.

- [ ] **Step 1.** Document P2: the `/<slug>/history/` page, build-history-only scope (dev-worktree deferred to P5), rename-following limitation (first-parent rename chains, no copy detection), and the `DOC_DIFF_LIMIT` env var.
- [ ] **Step 2 — full green gate.**
  ```
  cargo test
  cargo clippy --all-targets -- -D warnings
  cargo fmt --check
  ```
  **Expected tail (illustrative):**
  ```
  test result: ok. N passed; 0 failed; 0 ignored ...   (docgen-diff lib)
  test result: ok. M passed; 0 failed; 0 ignored ...   (history_git)
  test result: ok. K passed; 0 failed; 0 ignored ...   (docgen history_cli)
  ```
- [ ] **Commit:** `docs(p2): document git diff timeline + history pages`

---

## Parity checklist (every original test has a Rust home)

| Original `*.test.ts` | Rust test location |
| --- | --- |
| `git-refs.test.ts` (2) | A2 `git_refs::` |
| `git-parsing.test.ts` (3) | A3 `git_parsing::` |
| `line-diff.test.ts` (3) | B1 `line_diff::` |
| `block-diff.test.ts` (3) | B2 `block_diff::` |
| `file-tree.test.ts` (1) | B3 `file_tree::` |
| `payloads.test.ts` (1) | B4 `payloads::` |
| `timeline-groups.test.ts` (6) | B5 `timeline_groups::` |
| `markdown-render.server.test.ts` | covered by reuse of `docgen-core::markdown` (block HTML) + B6 block-html assertion |
| `tree.ts` (flattenTree/groupBlockRuns) | **deferred to P3** — those are island view-helpers (collapsed-state flattening, block-run grouping for the interactive navigator); P2's static page renders buckets directly. Documented, not ported. |
| `git-diff.server.ts` (no unit test; integration) | B6 + C2 hermetic git tests |

**Deliberate scope cuts (documented, not silent):**
- `dev-worktree` mode (untracked docs, worktree point, symlink guards, env base/head ref resolution, CI ref detection) → **P5**. Pure helpers (`parse_untracked_docs`) ported now for reuse.
- `tree.ts` interactive flatten/collapse + `groupBlockRuns` → **P3** (Alpine island).
- `summarizeReport` JSON is produced (B4) but not yet emitted to disk; the P3 island will ship it as `<script type="application/json">`. P2 renders fully static HTML.

## Risks / notes

- **Rename following:** `git2` has no direct `--follow`; we emulate via `Diff::find_similar` + path-tracking across first-parent history (Task A4 step 3). Limitation vs. `git log --follow`: only first-parent rename chains; no copy detection. Matches the original's effective behavior (it relied on git's default rename detection in `diff --name-status`, rename threshold `R100`+). The A4 rename test gates this.
- **LCS tie-break is load-bearing.** The originals prefer *removed* on ties (`>=`). Porting the LCS directly (B1/B2 option a) guarantees the expected op order; `similar` is allowed only if it matches the three line tests and the three block tests exactly.
- **Timezone in `format_date`.** Parity-faithful: both JS and our `chrono::Local` path are local-tz, so the `2026-03-(14|15)` ambiguity is reproduced, not "fixed."
- **`docs/`-prefix coupling.** `file_tree` and `is_doc_path` assume a `docs/` prefix (original convention). `build.rs` constructs the git-relative path with that prefix; if a project's docs dir isn't literally `docs/`, the file-tree label-stripping still works (it only strips a leading `docs` segment when present) and history still resolves by actual path.
- **Performance:** one `Revwalk` + tree-diff per doc is O(docs × history). `limit` (default 50) bounds it. Acceptable for P2; a shared single-walk optimization is a later concern.
