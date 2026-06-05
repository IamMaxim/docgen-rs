use std::io;
use std::path::Path;

use walkdir::WalkDir;

use crate::model::RawDoc;

/// Walk `root` recursively and read every `.md` file into a RawDoc.
/// `rel_path` is the path relative to `root`, normalized to `/` separators.
pub fn discover_docs(root: &Path) -> std::io::Result<Vec<RawDoc>> {
    let mut docs = Vec::new();
    // Sort entries for deterministic, reproducible discovery order.
    let walker = WalkDir::new(root).sort_by_file_name();
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
}
