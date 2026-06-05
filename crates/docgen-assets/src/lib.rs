//! `docgen-assets` owns every vendored and authored frontend file used by the
//! generated doc site. Files are embedded at compile time via `include_dir!`,
//! and exposed through a typed enumerate/emit API. The build subcommand drives
//! emission via [`assets_for`] + [`emit`].

use include_dir::{include_dir, Dir};

/// Every vendored + authored frontend file, embedded at compile time.
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeds_the_vendored_tree() {
        let alpine = ASSETS
            .get_file("vendor/alpine/alpine.min.js")
            .expect("alpine embedded");
        assert!(!alpine.contents().is_empty());
    }

    #[test]
    fn embeds_authored_docgen_files() {
        for p in ["docgen/docgen.css", "docgen/search.js", "docgen/bootstrap.js"] {
            assert!(ASSETS.get_file(p).is_some(), "missing embedded {p}");
        }
    }
}
