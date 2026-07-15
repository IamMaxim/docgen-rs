---
title: Git-history timeline
---

# Git-history timeline

**What it is.** For every document tracked in git, docgen generates a
`/<slug>/history/` page showing that document's commit timeline with line-level
and block-level diffs, plus a **History** link on the page itself.

**Why you'd want it.** Documentation drifts, and readers often need to know *when*
and *why* something changed. Instead of sending people to `git log`, docgen turns
each doc's history into a browsable page — reviewers, contributors, and readers
all see how a page evolved.

## What you get

- A per-document history page at `/<slug>/history/` with the commit timeline.
- Line-level and block-level diffs between consecutive versions.
- A **History** link added to the document page.

## Behavior and limits

- **Build-history mode only.** docgen renders *committed* history. Uncommitted
  working-tree changes are not shown.
- **Graceful no-op.** A project that isn't a git repository, or a document with
  no commit history, simply skips the history page. The build never fails on
  this.
- **Rename following.** History mirrors `git log -- <path>` with rename
  detection at a 50% similarity threshold. A rename that also heavily rewrites
  the file is recorded as an add plus a delete, so — like git *without*
  `--follow` — history is not stitched across such a rename. There is no copy
  detection.
- **Depth.** Each document walks up to **50 commits** by default. Override with
  the `DOC_DIFF_LIMIT` environment variable, clamped to the range `1..=200`:

  ```sh
  DOC_DIFF_LIMIT=100 docgen build
  ```

## Note

Because history comes from git, it appears only after your docs are committed.
When you deploy from CI, make sure the checkout includes history (for example,
GitHub's `actions/checkout` with `fetch-depth: 0`) so the timeline is complete.
See [[deployment]].
