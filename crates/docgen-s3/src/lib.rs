//! Optional S3-compatible asset offload for docgen. Computes a content-hashed
//! manifest of authored attachments and uploads the missing ones. Gated behind
//! `docgen-build`'s `s3` cargo feature.
pub mod manifest;
pub mod upload;

pub use manifest::{build_manifest, AssetManifest, ManifestEntry};
pub use upload::{credentials_from_env, upload, Credentials, UploadStats};
