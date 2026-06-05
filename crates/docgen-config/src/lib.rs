//! Parses an optional `docgen.toml`. When absent, `SiteConfig::default()`
//! reproduces docgen's pre-P6 hard-coded behaviour exactly, so a project with
//! no config builds identically to before.

use std::path::Path;

use serde::Deserialize;

/// Feature toggles. All default `true` — the pre-P6 behaviour (every feature on).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Features {
    /// Emit the `/graph/` page + its island.
    pub graph: bool,
    /// Render math (build-time KaTeX) + link its stylesheet.
    pub math: bool,
    /// Allow mermaid diagrams + lazy island.
    pub mermaid: bool,
    /// Emit the search index + search client.
    pub search: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self { graph: true, math: true, mermaid: true, search: true }
    }
}

/// `[components]` section.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ComponentsConfig {
    /// Project-relative directory holding `<name>/template.html` components.
    pub dir: String,
}

impl Default for ComponentsConfig {
    fn default() -> Self {
        Self { dir: "components".to_string() }
    }
}

/// The whole resolved site config. `Default` == pre-P6 behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct SiteConfig {
    /// Optional site title; when set, page `<title>` becomes `"{page} — {title}"`
    /// (home page uses just `title`). When `None`, per-page titles are unchanged.
    pub title: Option<String>,
    /// Base path for the deployed site (e.g. `/docs`). Empty = served at root
    /// (unchanged behaviour). Emitted as `<base href>` only in P6.
    pub base: String,
    pub features: Features,
    pub components: ComponentsConfig,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: toml::de::Error,
    },
}

/// Load `docgen.toml` from `project_root`. Missing file → `SiteConfig::default()`
/// (not an error). Present-but-malformed → `Err`.
pub fn load(project_root: &Path) -> Result<SiteConfig, ConfigError> {
    let path = project_root.join("docgen.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SiteConfig::default())
        }
        Err(e) => {
            return Err(ConfigError::Io {
                path: path.display().to_string(),
                source: e,
            })
        }
    };
    toml::from_str(&text).map_err(|e| ConfigError::Parse {
        path: path.display().to_string(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_pre_p6_behaviour() {
        let c = SiteConfig::default();
        assert_eq!(c.title, None);
        assert_eq!(c.base, "");
        assert!(c.features.graph && c.features.math && c.features.mermaid && c.features.search);
        assert_eq!(c.components.dir, "components");
    }

    #[test]
    fn missing_file_yields_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), SiteConfig::default());
    }

    #[test]
    fn parses_title_base_and_feature_toggles() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("docgen.toml"),
            "title = \"My Docs\"\nbase = \"/docs\"\n[features]\ngraph = false\nmermaid = false\n",
        )
        .unwrap();
        let c = load(dir.path()).unwrap();
        assert_eq!(c.title.as_deref(), Some("My Docs"));
        assert_eq!(c.base, "/docs");
        assert!(!c.features.graph);
        assert!(!c.features.mermaid);
        // Unspecified toggles keep their default (true).
        assert!(c.features.math);
        assert!(c.features.search);
    }

    #[test]
    fn partial_features_table_keeps_other_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docgen.toml"), "[features]\nsearch = false\n").unwrap();
        let c = load(dir.path()).unwrap();
        assert!(!c.features.search);
        assert!(c.features.graph);
    }

    #[test]
    fn malformed_toml_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("docgen.toml"), "title = = =\n").unwrap();
        assert!(load(dir.path()).is_err());
    }
}
