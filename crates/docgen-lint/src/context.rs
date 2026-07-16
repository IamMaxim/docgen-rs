//! The site-wide read model rules consume. Built once per run by the engine
//! from the same discovery/prepare/extract path the build uses, so what rules
//! see is exactly what `docgen build` would render.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use docgen_core::assetpass::normalize_join;
use docgen_core::discover::{
    discover_assets, discover_bases, discover_diagrams, discover_docs, BaseFileInput,
};
use docgen_core::extract::{extract_refs, DocRefs, HeadingRef};
use docgen_core::frontmatter::parse_frontmatter_checked;
use docgen_core::pipeline::{is_partial_rel, prepare, Diagrams, Partials, PreparedDoc};
use docgen_core::wikilink::{resolve_target, SlugSet};
use globset::GlobMatcher;

use crate::engine::LintError;

/// One doc (page or partial) as the linter sees it: prepared metadata,
/// extracted references, the raw on-disk text, and per-file lint state from
/// frontmatter.
pub struct DocEntry {
    pub prepared: PreparedDoc,
    /// Extracted references, with lines already shifted by [`Self::line_offset`]
    /// so they point into the RAW on-disk file (frontmatter included).
    pub refs: DocRefs,
    /// The raw file text (frontmatter included), for rules that need exact source.
    pub raw: String,
    /// Lines the stripped frontmatter block (fences included) occupied in the
    /// raw file. `refs` is already shifted; rules that re-extract positions
    /// from `prepared.body_md` themselves must add this to their line numbers.
    pub line_offset: usize,
    /// True for include-only `_*.md` partials. Partials are linted for content
    /// problems (links, assets, diagrams, frontmatter) but skipped by
    /// page-level rules (titles, orphans, slugs, emptiness).
    pub is_partial: bool,
    /// Rule ids this file suppresses via frontmatter `lint.ignore`.
    pub suppressed: BTreeSet<String>,
    /// The YAML error message when a frontmatter block exists but is malformed.
    pub frontmatter_error: Option<String>,
}

/// Everything rules need, computed once per run.
pub struct LintContext {
    /// All docs — pages AND include-only partials (`is_partial` distinguishes
    /// them; ignore-glob matches excluded) — in discovery order.
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

        // Prepare EVERY doc — pages and partials alike — from the original
        // RawDoc, so partials keep their frontmatter (suppression, invalid-
        // frontmatter checks) and get the same refs/line-offset treatment.
        // The `partials` include map is rebuilt from the prepared bodies,
        // which matches `partition_partials` (frontmatter-stripped).
        let mut docs = Vec::with_capacity(raws.len());
        let mut partials = Partials::new();
        for raw in raws {
            let is_partial = is_partial_rel(&raw.rel_path);
            let (_, frontmatter_error) = parse_frontmatter_checked(&raw.raw);
            let raw_text = raw.raw.clone();
            let prepared = prepare(raw);
            // Diagnostics must point into the raw file, but the body the refs
            // are extracted from starts AFTER the stripped frontmatter block —
            // shift every position once, here, so no rule ever re-adjusts.
            let line_offset = frontmatter_line_offset(&raw_text, &prepared.body_md);
            let mut refs = extract_refs(&prepared.body_md);
            refs.offset_lines(line_offset);
            let suppressed = suppressed_rules(&prepared.frontmatter);
            if is_partial {
                partials.insert(prepared.rel_path.clone(), prepared.body_md.clone());
            }
            docs.push(DocEntry {
                prepared,
                refs,
                raw: raw_text,
                line_offset,
                is_partial,
                suppressed,
                frontmatter_error,
            });
        }

        // Page-doc slugs only — see the `slugs` field docs.
        let slugs: SlugSet = docs
            .iter()
            .filter(|d| !d.is_partial)
            .map(|d| d.prepared.slug.clone())
            .collect();

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

        // Keyed by slug for pages; partial slugs (basename starts with `_`)
        // can never collide with page slugs, so partials are included too —
        // it lets `broken-anchor` validate a partial's same-page anchors.
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

/// The number of raw-file lines the stripped frontmatter block consumed
/// (opening fence, YAML, closing fence). `body` is always a byte suffix of the
/// (BOM-stripped) raw text — see `frontmatter::parse_frontmatter_checked` — so
/// the offset is exactly the newline count of the prefix before it. Handles
/// CRLF (counting `\n` covers it) and a leading BOM.
fn frontmatter_line_offset(raw: &str, body: &str) -> usize {
    let input = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let consumed = input.len().saturating_sub(body.len());
    input[..consumed].matches('\n').count()
}

/// Count inbound links per slug by resolving every PAGE doc's wikilinks plus
/// its internal page links (absolute `/slug` or relative `path.md`). Targets
/// are deduped per source doc and self-links dropped, mirroring how the
/// build's link graph treats edges. Partials are not counted as sources — the
/// build's graph is built from the top-level render pass only, and included
/// partial bodies never contribute edges there either. Every known slug gets
/// an entry (0 when unlinked).
fn inbound_counts(docs: &[DocEntry], slugs: &SlugSet) -> BTreeMap<String, usize> {
    let mut inbound: BTreeMap<String, usize> = slugs.iter().map(|s| (s.clone(), 0)).collect();
    for entry in docs.iter().filter(|d| !d.is_partial) {
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
/// Path joining uses `normalize_join` — `..` climbing past the docs root is
/// CLAMPED, exactly like the build's `rewrite_page_link` — so a root-escaping
/// link resolves to the same page the build would link to.
fn resolve_page_link(url: &str, base_dir: &str, slugs: &SlugSet) -> Option<String> {
    if url.contains("://") || url.starts_with("mailto:") || url.starts_with('#') {
        return None;
    }
    let path = url.split(['#', '?']).next().unwrap_or("");
    if path.is_empty() {
        return None;
    }
    let key = match path.strip_prefix('/') {
        Some(rest) => normalize_join("", rest),
        None => normalize_join(base_dir, path),
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
        // Root-escaping `..` is clamped, like the build's normalize_join —
        // NOT treated as unresolvable (M3).
        assert_eq!(
            resolve_page_link("../index.md", "", &slugs).as_deref(),
            Some("index")
        );
    }

    #[test]
    fn frontmatter_line_offset_counts_stripped_lines_exactly() {
        let cases: Vec<(String, usize)> = vec![
            // No frontmatter -> no offset.
            ("# Body\n".to_string(), 0),
            // 3-line block (---, title, ---).
            ("---\ntitle: X\n---\n# Body\n".to_string(), 3),
            // 4-line block.
            ("---\ntitle: X\ndescription: D\n---\nbody\n".to_string(), 4),
            // CRLF endings.
            ("---\r\ntitle: X\r\n---\r\nbody\r\n".to_string(), 3),
            // Leading BOM is stripped before counting.
            ("\u{feff}---\ntitle: X\n---\nbody\n".to_string(), 3),
            // Unterminated block: the whole input is body, offset 0.
            ("---\ntitle: X\n".to_string(), 0),
        ];
        for (raw, want) in cases {
            let (parsed, _) = parse_frontmatter_checked(&raw);
            assert_eq!(
                frontmatter_line_offset(&raw, &parsed.body),
                want,
                "raw: {raw:?}"
            );
        }
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
