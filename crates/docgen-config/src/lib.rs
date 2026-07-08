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
        Self {
            graph: true,
            math: true,
            mermaid: true,
            search: true,
        }
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
        Self {
            dir: "components".to_string(),
        }
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
    /// (unchanged behaviour). Prefixed onto every emitted asset/nav/wikilink URL
    /// so a sub-path deployment resolves correctly (no `<base>` tag is used —
    /// `<base>` only affects relative URLs, but our links are root-absolute).
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SiteConfig::default()),
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

/// Normalize a configured/derived `base` into a leading-slash, no-trailing-slash
/// form: `""`/`"/"` -> `""`, `"docs"`/`"/docs/"`/`"docs/"` -> `"/docs"`,
/// `"/group/project/"` -> `"/group/project"`. Interior slashes are preserved so
/// multi-segment sub-paths (GitLab's `namespace/project`) round-trip correctly.
pub fn normalize_base(base: &str) -> String {
    let trimmed = base.trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

/// Extract the path component of an absolute URL without pulling in a URL parser.
/// `https://ns.gitlab.io/proj` -> `/proj`; `https://host/a/b` -> `/a/b`;
/// `https://ns.gitlab.io` (no path) -> `""`. This is what makes GitLab's subdomain
/// Pages layout (`ns.gitlab.io/project`) and subpath layout (`host/group/project`)
/// both resolve to the right base: `CI_PAGES_URL` already encodes which one is in
/// effect, so its path is authoritative.
fn url_path(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    match after_scheme.find('/') {
        Some(i) => &after_scheme[i..],
        None => "",
    }
}

/// Resolve the effective deploy base path from config plus environment, applying
/// this precedence (first match wins), then [`normalize_base`]:
///  1. `DOCGEN_BASE` — explicit override. Present-but-empty forces the root
///     (an escape hatch for a custom-domain deploy under CI).
///  2. `docgen.toml`'s `base`, when non-empty — the project author's intent.
///  3. `CI_PAGES_URL` — the *path* of GitLab's actual Pages URL. Correct for both
///     subdomain (`ns.gitlab.io/project`) and subpath (`host/group/project`)
///     layouts, with zero CI config.
///  4. `CI_PROJECT_PATH` — `/<namespace>/<project>`, a fallback for older GitLab
///     that doesn't expose `CI_PAGES_URL` to the job.
///  5. `""` — served at the domain root.
pub fn resolve_base(config_base: &str) -> String {
    resolve_base_from(
        config_base,
        std::env::var("DOCGEN_BASE").ok().as_deref(),
        std::env::var("CI_PAGES_URL").ok().as_deref(),
        std::env::var("CI_PROJECT_PATH").ok().as_deref(),
    )
}

/// Pure core of [`resolve_base`] — env values are passed in so the precedence
/// logic is testable without mutating process-global environment.
fn resolve_base_from(
    config_base: &str,
    docgen_base_env: Option<&str>,
    ci_pages_url: Option<&str>,
    ci_project_path: Option<&str>,
) -> String {
    if let Some(explicit) = docgen_base_env {
        return normalize_base(explicit);
    }
    if !config_base.trim().is_empty() {
        return normalize_base(config_base);
    }
    if let Some(url) = ci_pages_url.filter(|u| !u.trim().is_empty()) {
        return normalize_base(url_path(url));
    }
    if let Some(path) = ci_project_path.filter(|p| !p.trim().is_empty()) {
        return normalize_base(path);
    }
    String::new()
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
        std::fs::write(
            dir.path().join("docgen.toml"),
            "[features]\nsearch = false\n",
        )
        .unwrap();
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

    #[test]
    fn normalize_base_canonicalizes() {
        assert_eq!(normalize_base(""), "");
        assert_eq!(normalize_base("/"), "");
        assert_eq!(normalize_base("docs"), "/docs");
        assert_eq!(normalize_base("/docs/"), "/docs");
        assert_eq!(normalize_base("docs/"), "/docs");
        // multi-segment sub-path (GitLab namespace/project) round-trips
        assert_eq!(normalize_base("/group/project/"), "/group/project");
        assert_eq!(normalize_base("group/project"), "/group/project");
    }

    #[test]
    fn url_path_extracts_path_component() {
        // subdomain layout -> just the project segment
        assert_eq!(url_path("https://group.gitlab.io/project"), "/project");
        // subpath layout -> full group/project path
        assert_eq!(
            url_path("https://gitlab.example.com/group/project"),
            "/group/project"
        );
        // custom domain at root -> no path
        assert_eq!(url_path("https://docs.example.com"), "");
        assert_eq!(url_path("http://host/a/b/"), "/a/b/");
    }

    #[test]
    fn resolve_base_precedence() {
        // 1. DOCGEN_BASE wins over everything (and is normalized).
        assert_eq!(
            resolve_base_from(
                "/from-toml",
                Some("/override/"),
                Some("https://x.io/pages"),
                Some("g/p")
            ),
            "/override"
        );
        // 1b. present-but-empty DOCGEN_BASE forces root even when others are set.
        assert_eq!(
            resolve_base_from(
                "/from-toml",
                Some(""),
                Some("https://x.io/pages"),
                Some("g/p")
            ),
            ""
        );
        // 2. docgen.toml base beats CI auto-detect.
        assert_eq!(
            resolve_base_from("/from-toml", None, Some("https://x.io/pages"), Some("g/p")),
            "/from-toml"
        );
        // 3. CI_PAGES_URL path used when config base is empty; subdomain layout.
        assert_eq!(
            resolve_base_from(
                "",
                None,
                Some("https://group.gitlab.io/project"),
                Some("group/project")
            ),
            "/project"
        );
        // 3b. subpath layout via CI_PAGES_URL.
        assert_eq!(
            resolve_base_from(
                "",
                None,
                Some("https://gitlab.example.com/group/project"),
                Some("group/project")
            ),
            "/group/project"
        );
        // 4. CI_PROJECT_PATH fallback when CI_PAGES_URL is absent.
        assert_eq!(
            resolve_base_from("", None, None, Some("group/project")),
            "/group/project"
        );
        // 4b. CI_PAGES_URL is authoritative when present: a root custom domain
        // (no path) means the site really is at root, so base is "" — we do NOT
        // fall through to CI_PROJECT_PATH and wrongly re-add a sub-path.
        assert_eq!(
            resolve_base_from(
                "",
                None,
                Some("https://docs.example.com"),
                Some("group/project")
            ),
            ""
        );
        // 5. nothing set -> root.
        assert_eq!(resolve_base_from("", None, None, None), "");
        assert_eq!(resolve_base_from("  ", None, None, Some("  ")), "");
    }
}
