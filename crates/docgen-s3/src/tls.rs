//! Process-level rustls provider selection.
//!
//! `attohttpc`'s rustls stack compiles in two crypto providers here (see the
//! `attohttpc` entry in Cargo.toml for why aws-lc-rs is unavoidable). rustls
//! refuses to guess between them: with both features on, `ClientConfig::builder()`
//! — which is what `attohttpc` calls on the first request — panics unless a
//! process-default provider has been installed. So install one, and make it ring.

use std::sync::Once;

/// Install `ring` as the process-default rustls provider. Idempotent, and safe to
/// call from anywhere: every entry point into an upload goes through here.
///
/// This lives in `docgen-s3` rather than the CLI's `main` on purpose — the panic
/// belongs to this crate's dependency, so anything that can reach `upload()` (the
/// binary, the server, a library consumer) is covered without having to know.
pub(crate) fn ensure_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // Err means a provider was already installed — by a host application that
        // embeds us, or by an earlier call that raced this one. Either way a
        // default now exists, which is all `ClientConfig::builder()` requires, so
        // there is nothing to recover from.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Named key-exchange groups, the one part of a provider that differs between
    /// ring and aws-lc-rs. Cipher suites deliberately do NOT work here: the names
    /// are IANA-standard and both providers expose the identical nine, so asserting
    /// on them passes under aws-lc-rs and catches nothing.
    fn kx_groups(provider: &rustls::crypto::CryptoProvider) -> Vec<String> {
        provider
            .kx_groups
            .iter()
            .map(|g| format!("{:?}", g.name()))
            .collect()
    }

    /// The invariant the end-to-end test cannot see: an upload over aws-lc-rs
    /// succeeds exactly as happily as one over ring, so only a direct comparison
    /// catches the provider silently flipping (someone drops the `-ring` feature, or
    /// installs a different default earlier in the process).
    #[test]
    fn ring_is_the_installed_default_provider() {
        let ring = rustls::crypto::ring::default_provider();
        let aws = rustls::crypto::aws_lc_rs::default_provider();

        // Self-check first: this test is only meaningful while the two providers are
        // actually distinguishable. Today aws-lc-rs carries X25519MLKEM768 (from
        // `prefer-post-quantum` in rustls' defaults, which attohttpc forces on) and
        // ring does not. If a rustls upgrade ever aligns them, this fails loudly and
        // demands a new discriminator — rather than silently degrading into a test
        // that passes under either provider, which is the trap that produced this
        // whole bug.
        assert_ne!(
            kx_groups(&ring),
            kx_groups(&aws),
            "ring and aws-lc-rs are no longer distinguishable by kx_groups; this test \
             can no longer tell which provider is installed — find a new discriminator"
        );

        ensure_crypto_provider();
        let installed = rustls::crypto::CryptoProvider::get_default()
            .expect("ensure_crypto_provider must leave a default installed");

        assert_eq!(
            kx_groups(installed),
            kx_groups(&ring),
            "process-default rustls provider is not ring"
        );
    }

    /// Guards the trap directly: `ClientConfig::builder()` is the exact call
    /// `attohttpc` makes, and with two providers compiled in it panics unless
    /// `ensure_crypto_provider` ran first. Reaching the assertion proves the
    /// ambiguity is resolved rather than merely absent.
    #[test]
    fn client_config_builds_without_panicking() {
        ensure_crypto_provider();

        let config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();

        assert!(
            !config.crypto_provider().cipher_suites.is_empty(),
            "resolved crypto provider has no cipher suites"
        );
    }
}
