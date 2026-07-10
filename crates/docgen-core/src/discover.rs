use std::io;
use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

use crate::model::RawDoc;

/// A non-markdown file discovered under the docs root, to be copied verbatim into
/// the built site so relative asset references (images, PDFs, …) resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetFile {
    /// Path relative to the docs root, `/`-separated — also its output location.
    pub rel_path: String,
    /// Absolute source path to copy from.
    pub src_path: PathBuf,
}

/// Directories that should never be treated as documentation, even when they
/// live under `docs/`. A mature project's docs tree often vendors tooling (an
/// Obsidian vault's `.obsidian`, a checked-in `node_modules`, a `.git`), and
/// walking those drowns the sidebar/search/graph in irrelevant files. We prune
/// any hidden directory (name starting with `.`) and a handful of well-known
/// dependency/output dirs by name.
const PRUNED_DIR_NAMES: &[&str] = &["node_modules", "target", "vendor", ".git"];

/// Whether the walker should descend into / yield this entry. The walk root
/// itself (depth 0) is always kept — only nested hidden / dependency dirs are
/// pruned, so a project whose root happens to be hidden still builds.
fn is_kept(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if entry.file_type().is_dir() {
        // Prune hidden dirs (`.obsidian`, `.superpowers`, …) and known vendor dirs.
        if name.starts_with('.') || PRUNED_DIR_NAMES.contains(&name.as_ref()) {
            return false;
        }
    }
    true
}

/// Walk `root` recursively and read every `.md` file into a RawDoc.
/// `rel_path` is the path relative to `root`, normalized to `/` separators.
///
/// Hidden directories and well-known dependency/output dirs (`node_modules`,
/// `target`, …) are pruned — see [`PRUNED_DIR_NAMES`] / [`is_kept`].
pub fn discover_docs(root: &Path) -> std::io::Result<Vec<RawDoc>> {
    let mut docs = Vec::new();
    // Sort entries for deterministic, reproducible discovery order; prune
    // hidden/vendor dirs so a vendored `node_modules` or Obsidian vault under
    // `docs/` doesn't pollute the site.
    let walker = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(is_kept);
    for entry in walker {
        // Propagate traversal errors (unreadable dirs, broken symlinks, loops)
        // instead of silently dropping docs.
        let entry = entry.map_err(io::Error::from)?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let raw = std::fs::read_to_string(path)?;
        docs.push(RawDoc { rel_path: rel, raw });
    }
    Ok(docs)
}

/// Walk `root` recursively and collect every non-`.md` file as an [`AssetFile`].
///
/// Shares the same prune rules as [`discover_docs`] (hidden / vendor dirs), so a
/// vendored `node_modules` or an Obsidian `.obsidian` dir under `docs/` is not
/// copied into the site. Hidden files (`.DS_Store`, …) are skipped too. These are
/// the images/PDFs/etc. that authored markdown links to relatively; the build
/// copies them into the output mirroring this relative tree.
pub fn discover_assets(root: &Path) -> std::io::Result<Vec<AssetFile>> {
    let mut assets = Vec::new();
    let walker = WalkDir::new(root)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(is_kept);
    for entry in walker {
        let entry = entry.map_err(io::Error::from)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // Markdown files become pages, not copied assets.
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            continue;
        }
        // Skip hidden files (e.g. `.DS_Store`) even though they aren't dirs.
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false)
        {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        assets.push(AssetFile {
            rel_path: rel,
            src_path: path.to_path_buf(),
        });
    }
    Ok(assets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_non_md_and_returns_slash_separated_rel_paths() {
        let dir = std::env::temp_dir().join(format!("docgen_discover_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("guide")).unwrap();
        std::fs::write(dir.join("index.md"), "# Home\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignore me\n").unwrap();
        std::fs::write(dir.join("guide/intro.md"), "# Intro\n").unwrap();

        let docs = discover_docs(&dir).unwrap();
        let rels: Vec<&str> = docs.iter().map(|d| d.rel_path.as_str()).collect();

        // Only the two .md files, in deterministic (sorted) order, slash-separated.
        assert_eq!(rels, vec!["guide/intro.md", "index.md"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prunes_hidden_and_vendor_dirs() {
        let dir = std::env::temp_dir().join(format!("docgen_prune_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".obsidian/plugins")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(dir.join("guide")).unwrap();
        std::fs::write(dir.join("index.md"), "# Home\n").unwrap();
        std::fs::write(dir.join("guide/intro.md"), "# Intro\n").unwrap();
        // These must be pruned, not ingested.
        std::fs::write(dir.join(".obsidian/plugins/conf.md"), "# junk\n").unwrap();
        std::fs::write(dir.join("node_modules/pkg/README.md"), "# dep\n").unwrap();

        let docs = discover_docs(&dir).unwrap();
        let rels: Vec<&str> = docs.iter().map(|d| d.rel_path.as_str()).collect();
        assert_eq!(rels, vec!["guide/intro.md", "index.md"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_assets_collects_non_md_preserving_tree() {
        let dir = std::env::temp_dir().join(format!("docgen_assets_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("system/attachments")).unwrap();
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        std::fs::write(dir.join("system/index.md"), "# Sys\n").unwrap();
        std::fs::write(dir.join("system/attachments/image.png"), b"\x89PNG").unwrap();
        std::fs::write(dir.join("logo.svg"), "<svg/>").unwrap();
        // Must be skipped: markdown, hidden file, and files under a pruned dir.
        std::fs::write(dir.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(dir.join(".obsidian/workspace.json"), "{}").unwrap();

        let assets = discover_assets(&dir).unwrap();
        let rels: Vec<&str> = assets.iter().map(|a| a.rel_path.as_str()).collect();
        assert_eq!(rels, vec!["logo.svg", "system/attachments/image.png"]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
