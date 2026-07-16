//! Guards that exactly one rustls crypto provider is compiled into the binary.
//!
//! Two HTTP clients reach rustls from this binary: `ureq` (docgen-plantuml)
//! asks for `rustls/ring`, and `attohttpc` (docgen-s3, via `rust-s3`) once asked
//! for `rustls/default`, which means `aws-lc-rs`. Cargo feature unification is
//! additive, so rustls compiled with *both*, and two things broke:
//!
//! 1. rustls refuses to guess a provider from ambiguous features, so
//!    `ClientConfig::builder()` — exactly what `attohttpc` calls — panicked at
//!    runtime on the first S3 upload.
//! 2. `aws-lc-sys` is a C library needing cmake (and NASM on Windows), so the
//!    build failed outright wherever those were absent.
//!
//! This test must live *here*, in the crate that pulls in both clients: neither
//! `docgen-plantuml` nor `docgen-s3` sees the conflict on its own, because each
//! resolves a single provider unambiguously when built alone. It is therefore
//! the one place where a dependency bump that re-enables `aws-lc-rs` anywhere in
//! the graph can be caught.
#![cfg(feature = "s3")]

/// The call `attohttpc` makes in `tls/rustls_impl.rs` to build its TLS config.
/// It resolves the provider from crate features and panics if they are
/// ambiguous, so reaching the assertion at all proves the graph is unambiguous.
#[test]
fn exactly_one_crypto_provider_is_enabled() {
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();

    // A provider carrying no cipher suites would take the panic-free path above
    // while still being useless, so confirm a real one was resolved.
    assert!(
        !config.crypto_provider().cipher_suites.is_empty(),
        "resolved crypto provider has no cipher suites"
    );
}
