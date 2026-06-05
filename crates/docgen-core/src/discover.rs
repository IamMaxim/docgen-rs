use std::path::Path;

use walkdir::WalkDir;

use crate::model::RawDoc;

/// Walk `root` recursively and read every `.md` file into a RawDoc.
/// `rel_path` is the path relative to `root`, normalized to `/` separators.
pub fn discover_docs(root: &Path) -> std::io::Result<Vec<RawDoc>> {
    let mut docs = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
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
