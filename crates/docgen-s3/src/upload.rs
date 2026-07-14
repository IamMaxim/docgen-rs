use std::collections::HashSet;

use anyhow::{Context, Result};
use docgen_config::S3Config;
use s3::creds::Credentials as S3Credentials;
use s3::{Bucket, Region};

use crate::manifest::{AssetManifest, ManifestEntry};

/// Upload result, for the build's log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UploadStats {
    pub uploaded: usize,
    pub skipped: usize,
}

/// Static S3 credentials, read from the environment.
#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

/// Read credentials from the standard AWS env vars. `None` if either the access
/// key or the secret is absent (the signal to fall back to local copy).
pub fn credentials_from_env() -> Option<Credentials> {
    let access_key = std::env::var("AWS_ACCESS_KEY_ID")
        .ok()
        .filter(|s| !s.is_empty())?;
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
        .ok()
        .filter(|s| !s.is_empty())?;
    let session_token = std::env::var("AWS_SESSION_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    Some(Credentials {
        access_key,
        secret_key,
        session_token,
    })
}

/// PURE: the manifest entries whose keys are NOT already in the bucket.
pub fn keys_to_upload<'a>(
    manifest: &'a AssetManifest,
    existing: &HashSet<String>,
) -> Vec<&'a ManifestEntry> {
    manifest
        .entries()
        .iter()
        .filter(|e| !existing.contains(&e.key))
        .collect()
}

/// PURE: a coarse content-type from the file extension. Covers the common doc
/// attachment types; everything else is `application/octet-stream`.
pub fn content_type_for(rel_path: &str) -> &'static str {
    let ext = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let ext = match ext.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => return "application/octet-stream",
    };
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// Construct the `rust-s3` bucket handle from config + credentials, with the
/// immutable cache header set on every request.
fn bucket_handle(config: &S3Config, creds: &Credentials) -> Result<Box<Bucket>> {
    let region = Region::Custom {
        region: config.region.clone(),
        endpoint: config
            .endpoint
            .clone()
            .unwrap_or_else(|| format!("https://s3.{}.amazonaws.com", config.region)),
    };
    let credentials = S3Credentials::new(
        Some(&creds.access_key),
        Some(&creds.secret_key),
        None,
        creds.session_token.as_deref(),
        None,
    )
    .context("building S3 credentials")?;

    let mut bucket = Bucket::new(&config.bucket, region, credentials)
        .context("constructing S3 bucket handle")?;
    if config.path_style {
        bucket.set_path_style();
    }
    bucket.add_header("Cache-Control", "public, max-age=31536000, immutable");
    Ok(bucket)
}

/// One paginated `ListObjectsV2` over the prefix; collect every existing key.
fn list_existing_keys(bucket: &Bucket, prefix: &str) -> Result<HashSet<String>> {
    let results = bucket
        .list(prefix.to_string(), None)
        .context("listing existing objects (ListObjectsV2)")?;
    let mut keys = HashSet::new();
    for page in results {
        for obj in page.contents {
            keys.insert(obj.key);
        }
    }
    Ok(keys)
}

/// Upload every manifest object not already present in the bucket. Idempotent:
/// content-hashed keys mean an existing key is byte-identical, so a re-run after
/// a transient failure resumes cleanly.
pub fn upload(
    manifest: &AssetManifest,
    config: &S3Config,
    creds: &Credentials,
) -> Result<UploadStats> {
    let bucket = bucket_handle(config, creds)?;
    let prefix = config.prefix.as_deref().unwrap_or("");
    let existing = list_existing_keys(&bucket, prefix)?;
    let pending = keys_to_upload(manifest, &existing);

    let mut uploaded = 0usize;
    for entry in &pending {
        let bytes = std::fs::read(&entry.src_path)
            .with_context(|| format!("reading asset {}", entry.src_path.display()))?;
        let content_type = content_type_for(&entry.rel_path);
        let resp = bucket
            .put_object_with_content_type(&entry.key, &bytes, content_type)
            .with_context(|| format!("uploading {} -> {}", entry.rel_path, entry.key))?;
        let code = resp.status_code();
        anyhow::ensure!(
            (200..300).contains(&code),
            "uploading {} -> {}: unexpected status {code}",
            entry.rel_path,
            entry.key
        );
        uploaded += 1;
    }

    Ok(UploadStats {
        uploaded,
        skipped: manifest.entries().len() - uploaded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{AssetManifest, ManifestEntry};
    use std::collections::HashSet;

    fn manifest_of(entries: Vec<(&str, &str)>) -> AssetManifest {
        // Build a manifest directly from (rel_path, key) pairs for diff testing.
        let mut m = AssetManifest::default();
        for (rel, key) in entries {
            m.push_for_test(ManifestEntry {
                rel_path: rel.to_string(),
                src_path: std::path::PathBuf::from(rel),
                key: key.to_string(),
                public_url: format!("https://cdn/{key}"),
            });
        }
        m
    }

    #[test]
    fn keys_to_upload_returns_only_missing() {
        let m = manifest_of(vec![
            ("a.png", "p/a.h1.png"),
            ("b.png", "p/b.h2.png"),
            ("c.png", "p/c.h3.png"),
        ]);
        let mut existing = HashSet::new();
        existing.insert("p/b.h2.png".to_string());
        let missing: Vec<_> = keys_to_upload(&m, &existing)
            .into_iter()
            .map(|e| e.key.clone())
            .collect();
        assert_eq!(
            missing,
            vec!["p/a.h1.png".to_string(), "p/c.h3.png".to_string()]
        );
    }

    #[test]
    fn keys_to_upload_all_present_is_empty() {
        let m = manifest_of(vec![("a.png", "k1")]);
        let mut existing = HashSet::new();
        existing.insert("k1".to_string());
        assert!(keys_to_upload(&m, &existing).is_empty());
    }

    #[test]
    fn content_type_common_extensions() {
        assert_eq!(content_type_for("a/b.png"), "image/png");
        assert_eq!(content_type_for("a/b.PDF"), "application/pdf");
        assert_eq!(content_type_for("a/b.svg"), "image/svg+xml");
        assert_eq!(
            content_type_for("a/unknown.xyz"),
            "application/octet-stream"
        );
        assert_eq!(content_type_for("noext"), "application/octet-stream");
    }
}
