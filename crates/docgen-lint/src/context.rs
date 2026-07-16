//! The site-wide read model rules consume. Built once per run by the engine
//! from the same discovery/prepare/extract path the build uses, so what rules
//! see is exactly what `docgen build` would render.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use docgen_core::discover::{
    discover_assets, discover_bases, discover_diagrams, discover_docs, BaseFileInput,
};
use docgen_core::extract::{extract_refs, DocRefs, HeadingRef};
use docgen_core::frontmatter::parse_frontmatter_checked;
use docgen_core::pipeline::{
    partition_partials, prepare, resolve_include_key, Diagrams, Partials, PreparedDoc,
};
use docgen_core::wikilink::{resolve_target, SlugSet};
use globset::GlobMatcher;

use crate::engine::LintError;

/// One page doc as the linter sees it: prepared metadata, extracted references,
/// the raw on-disk text, and per-file lint state from frontmatter.
pub struct DocEntry {
    pub prepared: PreparedDoc,
    pub refs: DocRefs,
    /// The raw file text (frontmatter included), for rules that need exact source.
    pub raw: String,
    /// Rule ids this file suppresses via frontmatter `lint.ignore`.
    pub suppressed: BTreeSet<String>,
    /// The YAML error message when a frontmatter block exists but is malformed.
    pub frontmatter_error: Option<String>,
}

/// Everything rules need, computed once per run.
pub struct LintContext {
    /// Page docs (partials + ignore-glob matches excluded), discovery order.
    pub docs: Vec<DocEntry>,
    /// The wikilink slug set — page-doc slugs only, mirroring the build:
    /// `render_docs` builds its `SlugSet` from prepared markdown docs alone,
    /// so `.base` page slugs are intentionally NOT included here either.
    pub slugs: SlugSet,
    /// Include-only partials (docs-relative path -> frontmatter-stripped body).
    pub partials: Partials,
    /// Docs-relative paths of the partial files, sorted.
    pub partial_paths: Vec<String>,
    /// Docs-relative paths of every non-`.md` asset file.
    pub assets: BTreeSet<String>,
    /// PlantUML sources (docs-relative path -> source).
    pub diagrams: Diagrams,
    /// Obsidian `.base` files (empty when `[features] bases = false`, like the build).
    pub bases: Vec<BaseFileInput>,
    /// Component registry: the same embedded built-ins the build uses,
    /// overridden by the project `components/` dir.
    pub components: docgen_components::Registry,
    /// Heading outline per page slug.
    pub headings: BTreeMap<String, Vec<HeadingRef>>,
    /// Inbound link count per page slug (wikilinks + internal page links,
    /// deduped per source doc, self-links dropped). Every slug has an entry.
    pub inbound: BTreeMap<String, usize>,
    pub config: docgen_config::SiteConfig,
    pub docs_dir: PathBuf,
    pub project_root: PathBuf,
}

impl LintContext {
    /// Discover + prepare the whole site. `ignore` are the compiled
    /// `[lint] ignore` globs, matched against docs-relative paths; matching
    /// files (pages AND partials) are excluded entirely.
    pub(crate) fn build(
        project_root: &Path,
        config: docgen_config::SiteConfig,
        ignore: &[GlobMatcher],
    ) -> Result<Self, LintError> {
        let docs_dir = project_root.join("docs");

        let raws = discover_docs(&docs_dir)?;
        let raws: Vec<_> = raws
            .into_iter()
            .filter(|r| !ignore.iter().any(|g| g.is_match(&r.rel_path)))
            .collect();
        let (pages, partials) = partition_partials(raws);

        let mut docs = Vec::with_capacity(pages.len());
        for raw in pages {
            let (_, frontmatter_error) = parse_frontmatter_checked(&raw.raw);
            let raw_text = raw.raw.clone();
            let prepared = prepare(raw);
            let refs = extract_refs(&prepared.body_md);
            let suppressed = suppressed_rules(&prepared.frontmatter);
            docs.push(DocEntry {
                prepared,
                refs,
                raw: raw_text,
                suppressed,
                frontmatter_error,
            });
        }

        // Page-doc slugs only — see the `slugs` field docs.
        let slugs: SlugSet = docs.iter().map(|d| d.prepared.slug.clone()).collect();

        let assets: BTreeSet<String> = discover_assets(&docs_dir)?
            .into_iter()
            .map(|a| a.rel_path)
            .collect();
        let diagrams = discover_diagrams(&docs_dir)?;
        // The build only loads `.base` files when the feature is on; mirror that.
        let bases = if config.features.bases {
            discover_bases(&docs_dir)?
        } else {
            Vec::new()
        };

        // Component registry: exactly how docgen-build constructs it — embedded
        // built-ins from docgen-assets, overridden by the project components dir.
        let builtins: Vec<docgen_components::Component> = docgen_assets::builtin_components()
            .into_iter()
            .map(|b| {
                docgen_components::Component::from_parts(
                    b.name,
                    b.template,
                    b.island_js.map(str::to_string),
                    b.style_css.map(str::to_string),
                )
            })
            .collect();
        let components_dir = project_root.join(&config.components.dir);
        let components = docgen_components::build_registry(builtins, &components_dir)?;

        let headings: BTreeMap<String, Vec<HeadingRef>> = docs
            .iter()
            .map(|d| (d.prepared.slug.clone(), d.refs.headings.clone()))
            .collect();

        let inbound = inbound_counts(&docs, &slugs);
        let partial_paths: Vec<String> = partials.keys().cloned().collect();

        Ok(Self {
            docs,
            slugs,
            partials,
            partial_paths,
            assets,
            diagrams,
            bases,
            components,
            headings,
            inbound,
            config,
            docs_dir,
            project_root: project_root.to_path_buf(),
        })
    }
}

/// Parse `lint.ignore` from a doc's frontmatter into the set of suppressed rule
/// ids. Absent or malformed shapes are tolerated silently (empty set).
fn suppressed_rules(frontmatter: &serde_yml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Some(seq) = frontmatter
        .get("lint")
        .and_then(|l| l.get("ignore"))
        .and_then(|v| v.as_sequence())
    {
        for item in seq {
            if let Some(s) = item.as_str() {
                out.insert(s.to_string());
            }
        }
    }
    out
}

/// Count inbound links per slug by resolving every doc's wikilinks plus its
/// internal page links (absolute `/slug` or relative `path.md`). Targets are
/// deduped per source doc and self-links dropped, mirroring how the build's
/// link graph treats edges. Every known slug gets an entry (0 when unlinked).
fn inbound_counts(docs: &[DocEntry], slugs: &SlugSet) -> BTreeMap<String, usize> {
    let mut inbound: BTreeMap<String, usize> = slugs.iter().map(|s| (s.clone(), 0)).collect();
    for entry in docs {
        let base_dir = entry
            .prepared
            .rel_path
            .rsplit_once('/')
            .map(|(d, _)| d)
            .unwrap_or("");
        let mut targets: BTreeSet<String> = BTreeSet::new();
        for w in &entry.refs.wikilinks {
            if let Some(slug) = resolve_target(&w.target, slugs) {
                targets.insert(slug);
            }
        }
        for l in &entry.refs.links {
            if let Some(slug) = resolve_page_link(&l.url, base_dir, slugs) {
                targets.insert(slug);
            }
        }
        for t in targets {
            if t != entry.prepared.slug {
                if let Some(n) = inbound.get_mut(&t) {
                    *n += 1;
                }
            }
        }
    }
    inbound
}

/// Resolve an internal markdown-link URL to a page slug, if it names one.
/// External URLs (scheme), mailto and pure-fragment links yield `None`.
fn resolve_page_link(url: &str, base_dir: &str, slugs: &SlugSet) -> Option<String> {
    if url.contains("://") || url.starts_with("mailto:") || url.starts_with('#') {
        return None;
    }
    let path = url.split(['#', '?']).next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    let key = match path.strip_prefix('/') {
        Some(rest) => rest.to_string(),
        None => resolve_include_key(base_dir, path)?,
    };
    let slug = key
        .strip_suffix(".md")
        .unwrap_or(&key)
        .trim_end_matches('/');
    slugs.contains(slug).then(|| slug.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug_set(slugs: &[&str]) -> SlugSet {
        slugs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn resolve_page_link_handles_absolute_relative_and_external() {
        let slugs = slug_set(&["index", "guide/intro"]);
        // Absolute slug link.
        assert_eq!(
            resolve_page_link("/guide/intro", "", &slugs).as_deref(),
            Some("guide/intro")
        );
        // Relative .md link resolved against the source dir.
        assert_eq!(
            resolve_page_link("./intro.md", "guide", &slugs).as_deref(),
            Some("guide/intro")
        );
        assert_eq!(
            resolve_page_link("../index.md", "guide", &slugs).as_deref(),
            Some("index")
        );
        // Anchors/queries stripped.
        assert_eq!(
            resolve_page_link("/guide/intro#setup", "", &slugs).as_deref(),
            Some("guide/intro")
        );
        // Not pages.
        assert_eq!(resolve_page_link("https://example.com/x", "", &slugs), None);
        assert_eq!(resolve_page_link("#anchor", "", &slugs), None);
        assert_eq!(resolve_page_link("/nope", "", &slugs), None);
    }

    #[test]
    fn suppressed_rules_parses_sequence_and_tolerates_malformed() {
        let fm: serde_yml::Value =
            serde_yml::from_str("lint:\n  ignore: [orphan-page, broken-wikilink]\n").unwrap();
        let s = suppressed_rules(&fm);
        assert!(s.contains("orphan-page") && s.contains("broken-wikilink"));

        // Absent / wrong shapes -> silently empty.
        assert!(suppressed_rules(&serde_yml::Value::Null).is_empty());
        let fm: serde_yml::Value = serde_yml::from_str("lint:\n  ignore: nope\n").unwrap();
        assert!(suppressed_rules(&fm).is_empty());
        let fm: serde_yml::Value = serde_yml::from_str("lint: 3\n").unwrap();
        assert!(suppressed_rules(&fm).is_empty());
    }
}
