use std::collections::HashMap;
use std::path::PathBuf;

use docgen_config::S3Config;
use docgen_core::asseturl::AssetUrlResolver;
use docgen_core::discover::AssetFile;
use sha2::{Digest, Sha256};

/// One asset's offload identity: where it came from, its content-hashed bucket
/// key, and the public URL that goes into the HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Docs-root-relative path (`/`-separated), matching the asset-pass namespace.
    pub rel_path: String,
    pub src_path: PathBuf,
    pub key: String,
    pub public_url: String,
}

/// The full set of offloaded assets, with a `rel_path -> index` lookup.
#[derive(Debug, Clone, Default)]
pub struct AssetManifest {
    entries: Vec<ManifestEntry>,
    index: HashMap<String, usize>,
}

impl AssetManifest {
    pub fn entries(&self) -> &[ManifestEntry] {
        &self.entries
    }

    /// Test-only constructor: push an entry directly, without touching the
    /// filesystem. Lets `upload.rs`'s pure-function tests build a manifest
    /// even though `entries`/`index` are private fields.
    #[cfg(test)]
    pub(crate) fn push_for_test(&mut self, entry: ManifestEntry) {
        self.index
            .insert(entry.rel_path.clone(), self.entries.len());
        self.entries.push(entry);
    }
}

impl AssetUrlResolver for AssetManifest {
    fn resolve(&self, rel_path: &str) -> Option<String> {
        self.index
            .get(rel_path)
            .map(|&i| self.entries[i].public_url.clone())
    }
}

/// Build the content-hashed bucket key for `rel_path`: inject `hash` before the
/// final extension, under `prefix`. Files with no real extension (or dotfiles)
/// get `hash` appended. `prefix` leading/trailing slashes are trimmed.
pub fn hashed_key(prefix: &str, rel_path: &str, hash: &str) -> String {
    let (dir, file) = match rel_path.rsplit_once('/') {
        Some((d, f)) => (d, f),
        None => ("", rel_path),
    };
    let hashed_file = match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}.{hash}.{ext}"),
        _ => format!("{file}.{hash}"),
    };
    let prefix = prefix.trim_matches('/');
    let mut segments: Vec<&str> = Vec::new();
    if !prefix.is_empty() {
        segments.push(prefix);
    }
    if !dir.is_empty() {
        segments.push(dir);
    }
    if segments.is_empty() {
        hashed_file
    } else {
        format!("{}/{hashed_file}", segments.join("/"))
    }
}

/// First 16 hex chars of the SHA-256 of `bytes`.
fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex = format!("{digest:x}");
    hex[..16].to_string()
}

/// Read + hash every asset, producing an [`AssetManifest`]. Reads file bytes, so
/// returns `io::Result`. `public_url` is joined as `{public_url}/{key}` with the
/// config's `public_url` trailing slash trimmed.
pub fn build_manifest(assets: &[AssetFile], config: &S3Config) -> std::io::Result<AssetManifest> {
    let prefix = config.prefix.as_deref().unwrap_or("");
    let base = config.public_url.trim_end_matches('/');
    let mut entries = Vec::with_capacity(assets.len());
    let mut index = HashMap::with_capacity(assets.len());
    for asset in assets {
        let bytes = std::fs::read(&asset.src_path)?;
        let hash = short_hash(&bytes);
        let key = hashed_key(prefix, &asset.rel_path, &hash);
        let public_url = format!("{base}/{key}");
        index.insert(asset.rel_path.clone(), entries.len());
        entries.push(ManifestEntry {
            rel_path: asset.rel_path.clone(),
            src_path: asset.src_path.clone(),
            key,
            public_url,
        });
    }
    Ok(AssetManifest { entries, index })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_injects_hash_before_extension() {
        assert_eq!(
            hashed_key("docs-assets", "system/attachments/img.png", "abc123"),
            "docs-assets/system/attachments/img.abc123.png"
        );
    }

    #[test]
    fn key_without_prefix() {
        assert_eq!(
            hashed_key("", "system/img.png", "abc123"),
            "system/img.abc123.png"
        );
    }

    #[test]
    fn key_root_level_file() {
        assert_eq!(hashed_key("p", "logo.png", "abc123"), "p/logo.abc123.png");
    }

    #[test]
    fn key_prefix_slashes_trimmed() {
        assert_eq!(hashed_key("/p/", "a/b.png", "h"), "p/a/b.h.png");
    }

    #[test]
    fn key_no_extension_appends_hash() {
        assert_eq!(hashed_key("p", "dir/README", "h"), "p/dir/README.h");
    }

    #[test]
    fn key_dotfile_treated_as_no_extension() {
        // ".gitkeep" has empty stem -> no extension split.
        assert_eq!(hashed_key("", "dir/.gitkeep", "h"), "dir/.gitkeep.h");
    }

    #[test]
    fn key_multi_dot_uses_last_extension() {
        assert_eq!(
            hashed_key("", "a/archive.tar.gz", "h"),
            "a/archive.tar.h.gz"
        );
    }

    #[test]
    fn build_manifest_hashes_and_maps() {
        let dir = std::env::temp_dir().join("docgen_s3_manifest_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("img.png");
        std::fs::write(&file, b"hello").unwrap();

        let assets = vec![AssetFile {
            rel_path: "system/img.png".to_string(),
            src_path: file.clone(),
        }];
        let config = S3Config {
            bucket: "b".into(),
            region: "auto".into(),
            endpoint: None,
            prefix: Some("docs-assets".into()),
            public_url: "https://cdn.example.com/".into(),
            path_style: false,
        };
        let m = build_manifest(&assets, &config).unwrap();
        let url = m.resolve("system/img.png").expect("mapped");
        // SHA-256("hello") = 2cf24dba5fb0a30e...; first 16 hex chars are stable.
        assert_eq!(
            url,
            "https://cdn.example.com/docs-assets/system/img.2cf24dba5fb0a30e.png"
        );
        assert_eq!(m.resolve("nope"), None);

        std::fs::remove_dir_all(&dir).ok();
    }
}
