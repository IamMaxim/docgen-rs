//! Bridge from docgen's document model to the `docgen-bases` engine: builds a
//! queryable [`Corpus`] of notes (frontmatter properties + file metadata + tags +
//! links) from the prepared docs, and re-exports the pieces the build needs.
//!
//! This module owns the docgen-specific glue (reading `PreparedDoc`s, scanning
//! `#tags`/`[[wikilinks]]` out of bodies); `docgen-bases` itself stays free of any
//! docgen types.

use docgen_bases::{properties_from_yaml, value_from_yaml, BaseLink, Corpus, Note, Value};

use crate::pipeline::PreparedDoc;

/// Filesystem facts a caller supplies per doc (docgen-core does no I/O). Times are
/// epoch-milliseconds; `None` when unavailable (git checkouts don't preserve
/// mtime, so this is best-effort).
#[derive(Debug, Clone, Copy, Default)]
pub struct FileFacts {
    pub size: u64,
    pub ctime_ms: Option<i64>,
    pub mtime_ms: Option<i64>,
}

/// Build a [`Note`] from a prepared doc plus its file facts.
pub fn note_from_doc(p: &PreparedDoc, facts: FileFacts) -> Note {
    let name = p
        .rel_path
        .rsplit('/')
        .next()
        .unwrap_or(&p.rel_path)
        .to_string();
    let basename = name
        .rsplit_once('.')
        .map(|(b, _)| b.to_string())
        .unwrap_or_else(|| name.clone());
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_string())
        .unwrap_or_default();
    let folder = p
        .rel_path
        .rsplit_once('/')
        .map(|(d, _)| d.to_string())
        .unwrap_or_default();

    let mut properties = properties_from_yaml(&p.frontmatter);
    // Obsidian exposes the note title (from an `title:`/first-H1) too; docgen has
    // already resolved it, so surface it as a synthetic property when absent.
    properties
        .entry("title".to_string())
        .or_insert_with(|| Value::Str(p.title.clone()));

    let tags = collect_tags(&p.frontmatter, &p.body_md);
    let links = collect_links(&p.frontmatter, &p.body_md);

    Note {
        properties,
        name,
        basename,
        path: p.rel_path.clone(),
        folder,
        ext,
        size: facts.size,
        ctime: facts.ctime_ms.map(ms_to_date),
        mtime: facts.mtime_ms.map(ms_to_date),
        tags,
        links,
        slug: p.slug.clone(),
    }
}

/// Build a corpus from all prepared (page) docs. `facts` supplies per-doc file
/// metadata by docs-relative path.
pub fn build_corpus(prepared: &[PreparedDoc], facts: &dyn Fn(&str) -> FileFacts) -> Corpus {
    Corpus::new(
        prepared
            .iter()
            .map(|p| note_from_doc(p, facts(&p.rel_path)))
            .collect(),
    )
}

fn ms_to_date(ms: i64) -> docgen_bases::BaseDate {
    docgen_bases::eval::date_from_epoch_millis(ms, true)
}

/// Collect tags from a note: frontmatter `tags:`/`tag:` (string or list) plus
/// inline `#tags` in the body. Leading `#` is stripped; duplicates removed,
/// first-seen order preserved.
fn collect_tags(fm: &serde_yml::Value, body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: String| {
        let t = t.trim_start_matches('#').to_string();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    };
    for key in ["tags", "tag"] {
        if let Some(v) = fm.get(key) {
            match value_from_yaml(v) {
                Value::Str(s) => s.split([',', ' ']).for_each(|t| push(t.to_string())),
                Value::List(items) => {
                    for it in items {
                        push(it.display());
                    }
                }
                _ => {}
            }
        }
    }
    for tag in scan_inline_tags(body) {
        push(tag);
    }
    out
}

/// Scan `#tag` occurrences in body text. A tag starts at a `#` that is at the
/// start of the string or preceded by whitespace, followed by a tag char run
/// (alphanumeric, `/`, `-`, `_`); a purely numeric run (`#123`) is not a tag.
fn scan_inline_tags(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || chars[i - 1].is_whitespace();
        if chars[i] == '#' && boundary {
            let start = i + 1;
            let mut j = start;
            while j < chars.len()
                && (chars[j].is_alphanumeric() || matches!(chars[j], '/' | '-' | '_'))
            {
                j += 1;
            }
            let tag: String = chars[start..j].iter().collect();
            // Require at least one non-digit char (so `#123` is not a tag).
            if !tag.is_empty() && tag.chars().any(|c| !c.is_ascii_digit()) {
                out.push(tag);
            }
            i = j.max(start);
        } else {
            i += 1;
        }
    }
    out
}

/// Collect outbound links: frontmatter values that are links, plus `[[wikilinks]]`
/// in the body.
fn collect_links(fm: &serde_yml::Value, body: &str) -> Vec<BaseLink> {
    let mut out: Vec<BaseLink> = Vec::new();
    let mut push = |l: BaseLink| {
        if !out.iter().any(|e| e.path == l.path) {
            out.push(l);
        }
    };
    collect_links_from_value(&value_from_yaml(fm), &mut push);
    for link in scan_body_wikilinks(body) {
        push(link);
    }
    out
}

fn collect_links_from_value(v: &Value, push: &mut impl FnMut(BaseLink)) {
    match v {
        Value::Link(l) => push(l.clone()),
        Value::List(items) => items
            .iter()
            .for_each(|it| collect_links_from_value(it, push)),
        Value::Object(map) => map
            .values()
            .for_each(|it| collect_links_from_value(it, push)),
        _ => {}
    }
}

/// Scan `[[target|label]]` wikilinks out of body markdown.
fn scan_body_wikilinks(body: &str) -> Vec<BaseLink> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(end) = body[i + 2..].find("]]") {
                let inner = &body[i + 2..i + 2 + end];
                let (target, label) = crate::wikilink::parse_wikilink(inner);
                let target = target.split(['#', '^']).next().unwrap_or(&target).trim();
                let target = target.strip_suffix(".md").unwrap_or(target);
                if !target.is_empty() {
                    out.push(match label {
                        Some(d) => BaseLink::with_display(target, d),
                        None => BaseLink::new(target),
                    });
                }
                i = i + 2 + end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RawDoc;
    use crate::pipeline::prepare;

    fn prep(path: &str, raw: &str) -> PreparedDoc {
        prepare(RawDoc {
            rel_path: path.into(),
            raw: raw.into(),
        })
    }

    #[test]
    fn builds_note_metadata() {
        let p = prep(
            "guide/Intro.md",
            "---\ntitle: Introduction\nstatus: done\ntags: [book, guide]\n---\n# Introduction\nSee [[guide/Advanced]] and #project/active.\n",
        );
        let note = note_from_doc(
            &p,
            FileFacts {
                size: 42,
                ..Default::default()
            },
        );
        assert_eq!(note.name, "Intro.md");
        assert_eq!(note.basename, "Intro");
        assert_eq!(note.folder, "guide");
        assert_eq!(note.ext, "md");
        assert_eq!(note.size, 42);
        assert_eq!(note.slug, "guide/Intro");
        // Frontmatter tags + inline tag.
        assert!(note.tags.contains(&"book".to_string()));
        assert!(note.tags.contains(&"guide".to_string()));
        assert!(note.tags.contains(&"project/active".to_string()));
        // Body wikilink.
        assert!(note.links.iter().any(|l| l.path == "guide/Advanced"));
        // Property access.
        assert!(matches!(note.note_property("status"), Value::Str(s) if s == "done"));
        assert!(matches!(note.note_property("title"), Value::Str(s) if s == "Introduction"));
    }

    #[test]
    fn inline_tag_scan_ignores_numbers_and_midword() {
        let tags = scan_inline_tags("a #real tag, issue #123, email a#b, and #nested/tag.");
        assert!(tags.contains(&"real".to_string()));
        assert!(tags.contains(&"nested/tag".to_string()));
        assert!(!tags.iter().any(|t| t == "123"));
        assert!(!tags.iter().any(|t| t == "b")); // a#b is not a tag (no boundary)
    }

    #[test]
    fn corpus_from_multiple_docs() {
        let docs = vec![
            prep("a.md", "---\ntype: note\n---\n# A\n"),
            prep("b.md", "# B\n"),
        ];
        let corpus = build_corpus(&docs, &|_| FileFacts::default());
        assert_eq!(corpus.notes.len(), 2);
        assert_eq!(corpus.notes[0].basename, "a");
    }

    #[test]
    fn frontmatter_link_property_collected() {
        let p = prep(
            "x.md",
            "---\nauthor: \"[[People/Herbert|Herbert]]\"\n---\n# X\n",
        );
        let note = note_from_doc(&p, FileFacts::default());
        assert!(note.links.iter().any(|l| l.path == "People/Herbert"));
    }
}
