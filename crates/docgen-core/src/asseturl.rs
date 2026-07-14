//! An S3-agnostic hook that lets the asset-URL rewrite pass point a resolved,
//! docs-root-relative asset path at an externally-hosted URL (e.g. an S3/CDN
//! object). `docgen-core` owns the trait so it never depends on `docgen-s3`;
//! the S3 crate implements it. When no resolver is supplied, the pass emits the
//! usual base-absolute local URL.
pub trait AssetUrlResolver {
    /// Return the public URL for the asset at `rel_path` (docs-root-relative,
    /// `/`-separated, no leading slash), or `None` to fall back to the local URL.
    fn resolve(&self, rel_path: &str) -> Option<String>;
}
