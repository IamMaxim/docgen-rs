//! `docgen init` scaffolds a fresh site from an embedded template tree. Replaces
//! the Node `create-docgen`. Plain-file copy + `_gitignore`→`.gitignore` rename.

use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};

static TEMPLATE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/template");

pub struct InitOptions {
    /// Target directory to scaffold into (created if missing).
    pub target: PathBuf,
    /// Overwrite existing files if the dir is non-empty. Default false → error.
    pub force: bool,
}

/// Scaffold a new docgen site into `opts.target`. Errors if the target is a
/// non-empty dir and `force` is false (so we never clobber an existing project).
pub fn scaffold(opts: &InitOptions) -> anyhow::Result<()> {
    if opts.target.exists()
        && opts
            .target
            .read_dir()
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
        && !opts.force
    {
        anyhow::bail!(
            "target {} is not empty (use --force to scaffold anyway)",
            opts.target.display()
        );
    }
    std::fs::create_dir_all(&opts.target)?;
    write_dir(&TEMPLATE, &opts.target)?;
    Ok(())
}

/// Map a template path component, applying the `_gitignore` → `.gitignore` rename.
fn rename_dotfile(name: &str) -> String {
    match name {
        "_gitignore" => ".gitignore".to_string(),
        other => other.to_string(),
    }
}

fn write_dir(dir: &Dir, target: &Path) -> std::io::Result<()> {
    for file in dir.files() {
        let rel = file.path();
        let mut dest = target.to_path_buf();
        for comp in rel.components() {
            dest.push(rename_dotfile(&comp.as_os_str().to_string_lossy()));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.contents())?;
    }
    for sub in dir.dirs() {
        write_dir(sub, target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_expected_tree_into_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("site");
        scaffold(&InitOptions {
            target: t.clone(),
            force: false,
        })
        .unwrap();
        assert!(t.join("docgen.toml").is_file());
        assert!(t.join(".gitignore").is_file()); // renamed from _gitignore
        assert!(t.join("docs/index.md").is_file());
        assert!(t.join("docs/guide.md").is_file());
        assert!(t.join("components/note/template.html").is_file());
        // _gitignore must NOT survive un-renamed
        assert!(!t.join("_gitignore").exists());
    }

    #[test]
    fn refuses_nonempty_dir_without_force() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("existing.txt"), "x").unwrap();
        let err = scaffold(&InitOptions {
            target: dir.path().to_path_buf(),
            force: false,
        });
        assert!(err.is_err());
    }

    #[test]
    fn force_overwrites_nonempty_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("existing.txt"), "x").unwrap();
        scaffold(&InitOptions {
            target: dir.path().to_path_buf(),
            force: true,
        })
        .unwrap();
        assert!(dir.path().join("docgen.toml").is_file());
    }

    #[test]
    fn scaffolded_docgen_toml_parses_with_config_loader() {
        // cross-crate sanity: the embedded config is valid for docgen-config.
        let dir = tempfile::tempdir().unwrap();
        scaffold(&InitOptions {
            target: dir.path().to_path_buf(),
            force: true,
        })
        .unwrap();
        let cfg = docgen_config::load(dir.path()).unwrap();
        assert_eq!(cfg.title.as_deref(), Some("My Docs"));
    }
}
