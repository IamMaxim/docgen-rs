---
title: Git-history timeline
---

# Git-history timeline

**What it is.** docgen generates a single interactive **`/diff/`** workspace for
the whole site: a timeline of the commits that touched your docs, each with
line-level and block-level diffs of what changed. It's reached from a diff icon
in the topbar.

**Why you'd want it.** Documentation drifts, and readers often need to know *when*
and *why* something changed. Instead of sending people to `git log`, docgen turns
your docs' history into a browsable timeline — reviewers, contributors, and
readers all see how the site evolved, with rendered diffs rather than raw patch
text.

## What you get

- A `/diff/` workspace with a timeline of every commit that changed a doc.
- Per-commit, per-file line-level and block-level diffs, loaded lazily as you
  select a commit.
- A diff icon in the topbar that links to the workspace (shown only when the
  workspace was emitted).

## Turning it off

The `/diff/` workspace and its client assets are controlled by the `diff` toggle
in `[features]` (on by default):

```toml
[features]
diff = false   # no /diff/ workspace, no diff assets, no topbar icon
```

See [[configuration]] for the full `[features]` table.

## Behavior and limits

- **Build-history mode only.** docgen renders *committed* history. Uncommitted
  working-tree changes are not shown.
- **Graceful no-op.** A project that isn't a git repository, or one whose docs
  have no commit history, simply skips the `/diff/` workspace (and its topbar
  icon). The build never fails on this — so `diff = true` is inert outside a git
  repo either way.
- **Rename following.** History mirrors `git log` with rename detection at a 50%
  similarity threshold. A rename that also heavily rewrites the file is recorded
  as an add plus a delete, so — like git *without* `--follow` — history is not
  stitched across such a rename. There is no copy detection.
- **Depth.** The timeline walks up to **50 commits** by default. Override with
  the `DOC_DIFF_LIMIT` environment variable, clamped to the range `1..=200`:

  ```sh
  DOC_DIFF_LIMIT=100 docgen build
  ```

## Note

Because the timeline comes from git, it appears only after your docs are
committed. When you deploy from CI, make sure the checkout includes history (for
example, GitHub's `actions/checkout` with `fetch-depth: 0`) so the timeline is
complete. See [[deployment]].
