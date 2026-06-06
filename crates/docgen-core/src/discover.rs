use std::io;
use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::model::RawDoc;

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
}
