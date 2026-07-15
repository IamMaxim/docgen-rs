//! The corpus: the set of notes a base queries over. Each [`Note`] carries its
//! frontmatter properties (as typed [`Value`]s) plus file metadata (`file.*`).
//! The caller (docgen-build / docgen-core) constructs these from discovered docs;
//! this crate stays free of any filesystem or docgen-specific types.

use std::collections::BTreeMap;

use crate::value::{BaseDate, BaseLink, Value};

/// One note (markdown file) in the vault, queryable by a base.
#[derive(Debug, Clone, Default)]
pub struct Note {
    /// Frontmatter properties, keyed by name, coerced to typed values. Accessed
    /// via `note.<name>` or a bare `<name>` in expressions.
    pub properties: BTreeMap<String, Value>,
    // --- file.* metadata ---
    /// `file.name` — filename with extension (e.g. `Intro.md`).
    pub name: String,
    /// `file.basename` — filename without extension.
    pub basename: String,
    /// `file.path` — full vault-relative path (e.g. `guide/Intro.md`).
    pub path: String,
    /// `file.folder` — containing folder path (empty for root).
    pub folder: String,
    /// `file.ext` — extension without the dot (`md`).
    pub ext: String,
    /// `file.size` — byte length of the source file.
    pub size: u64,
    /// `file.ctime` — creation time, if known.
    pub ctime: Option<BaseDate>,
    /// `file.mtime` — modification time, if known.
    pub mtime: Option<BaseDate>,
    /// `file.tags` — all `#tags` (frontmatter + body), without the leading `#`.
    pub tags: Vec<String>,
    /// `file.links` — outbound internal link targets.
    pub links: Vec<BaseLink>,
    /// The doc's site slug (docgen-specific; used to build the cell hyperlink to
    /// the rendered page). Not exposed as a base property.
    pub slug: String,
}

impl Note {
    /// Resolve a `file.<field>` access to a value. Unknown fields → `Null`.
    pub fn file_property(&self, field: &str) -> Value {
        match field {
            "name" => Value::Str(self.name.clone()),
            "basename" => Value::Str(self.basename.clone()),
            "path" => Value::Str(self.path.clone()),
            "folder" => Value::Str(self.folder.clone()),
            "ext" => Value::Str(self.ext.clone()),
            "size" => Value::Number(self.size as f64),
            "ctime" => self.ctime.map(Value::Date).unwrap_or(Value::Null),
            "mtime" => self.mtime.map(Value::Date).unwrap_or(Value::Null),
            "tags" => Value::List(self.tags.iter().cloned().map(Value::Str).collect()),
            "links" => Value::List(self.links.iter().cloned().map(Value::Link).collect()),
            // `file.file` / `file.properties` reflect the note itself.
            "properties" => Value::Object(self.properties.clone()),
            _ => Value::Null,
        }
    }

    /// Resolve a bare/`note.` property. Unknown → `Null`.
    pub fn note_property(&self, name: &str) -> Value {
        self.properties.get(name).cloned().unwrap_or(Value::Null)
    }

    /// Whether this note has any of `tags` (Obsidian's `file.hasTag` matches a tag
    /// or any of its parents: a note tagged `#a/b` matches `hasTag("a")`).
    pub fn has_tag(&self, want: &str) -> bool {
        let want = want.trim_start_matches('#');
        self.tags
            .iter()
            .any(|t| t == want || t.starts_with(&format!("{want}/")))
    }

    /// Whether this note lives in `folder` or any subfolder of it.
    pub fn in_folder(&self, folder: &str) -> bool {
        let folder = folder.trim_matches('/');
        if folder.is_empty() {
            return true;
        }
        self.folder == folder || self.folder.starts_with(&format!("{folder}/"))
    }

    /// Whether this note links to `target` (by basename/path).
    pub fn has_link(&self, target: &BaseLink) -> bool {
        self.links.iter().any(|l| l.same_target(target))
    }
}

/// The whole set of notes a base evaluates against.
#[derive(Debug, Clone, Default)]
pub struct Corpus {
    pub notes: Vec<Note>,
}

impl Corpus {
    pub fn new(notes: Vec<Note>) -> Self {
        Self { notes }
    }

    /// Notes that link to `target` — backs `file.backlinks` / `linksTo`.
    pub fn backlinks_to(&self, target: &BaseLink) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.has_link(target)).collect()
    }
}

/// Convert a parsed YAML frontmatter mapping (`serde_yml::Value`) into typed base
/// properties. Strings that look like `[[wikilinks]]` become [`Value::Link`];
/// `YYYY-MM-DD[ HH:mm[:ss]]` strings become [`Value::Date`]; sequences become
/// lists; nested maps become objects. This is the bridge from docgen's
/// frontmatter to the base value model.
pub fn properties_from_yaml(fm: &serde_yml::Value) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    if let serde_yml::Value::Mapping(map) = fm {
        // serde_yml (noyalib shim) mappings are String-keyed.
        for (k, v) in map {
            out.insert(k.to_string(), value_from_yaml(v));
        }
    }
    out
}

/// Convert a single YAML node into a base [`Value`], inferring dates and links
/// from string scalars.
pub fn value_from_yaml(v: &serde_yml::Value) -> Value {
    match v {
        serde_yml::Value::Null => Value::Null,
        serde_yml::Value::Bool(b) => Value::Bool(*b),
        serde_yml::Value::Number(_) => v.as_f64().map(Value::Number).unwrap_or(Value::Null),
        serde_yml::Value::String(s) => scalar_string_to_value(s),
        serde_yml::Value::Sequence(items) => {
            Value::List(items.iter().map(value_from_yaml).collect())
        }
        serde_yml::Value::Mapping(map) => {
            let mut obj = BTreeMap::new();
            for (k, val) in map {
                obj.insert(k.to_string(), value_from_yaml(val));
            }
            Value::Object(obj)
        }
        // Tagged scalars (e.g. `!Custom foo`): treat as their string form.
        other => other
            .as_str()
            .map(scalar_string_to_value)
            .unwrap_or(Value::Null),
    }
}

/// Infer a value from a YAML string scalar: `[[wikilink]]`/`[[a|b]]` → link,
/// ISO-ish date → date, otherwise a plain string.
pub fn scalar_string_to_value(s: &str) -> Value {
    let trimmed = s.trim();
    if let Some(link) = parse_wikilink(trimmed) {
        return Value::Link(link);
    }
    if let Some(date) = parse_date(trimmed) {
        return Value::Date(date);
    }
    Value::Str(s.to_string())
}

/// Parse a `[[target]]` or `[[target|display]]` wikilink string. Returns `None`
/// if the string isn't a single wikilink.
pub fn parse_wikilink(s: &str) -> Option<BaseLink> {
    let inner = s.strip_prefix("[[")?.strip_suffix("]]")?;
    if inner.contains("[[") {
        return None; // not a single link
    }
    let (target, display) = match inner.split_once('|') {
        Some((t, d)) => (t.trim(), Some(d.trim().to_string())),
        None => (inner.trim(), None),
    };
    // Drop any `#heading` / `^block` anchor from the target.
    let target = target.split(['#', '^']).next().unwrap_or(target).trim();
    let target = target.strip_suffix(".md").unwrap_or(target);
    Some(BaseLink {
        path: target.to_string(),
        display,
    })
}

/// Parse a `YYYY-MM-DD`, `YYYY-MM-DDTHH:mm[:ss]`, or `YYYY-MM-DD HH:mm[:ss]`
/// string into a [`BaseDate`]. Lenient but strict enough not to swallow arbitrary
/// hyphenated text. Returns `None` when the shape doesn't match.
pub fn parse_date(s: &str) -> Option<BaseDate> {
    let s = s.trim();
    let (date_part, time_part) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut dp = date_part.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: u32 = dp.next()?.parse().ok()?;
    let day: u32 = dp.next()?.parse().ok()?;
    if dp.next().is_some() || !(1..=12).contains(&month) {
        return None;
    }
    // Reject impossible calendar dates (e.g. Feb 30, Apr 31): validate the day
    // against the month's actual length so `epoch_millis` never aliases an
    // out-of-range day onto a different real date.
    if day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // A plausible year band guards against matching things like `12-34-56` ids.
    if !(1000..=9999).contains(&year) {
        return None;
    }
    let (mut hour, mut minute, mut second, mut millisecond, has_time) =
        (0u32, 0u32, 0u32, 0u32, time_part.is_some());
    if let Some(t) = time_part {
        // Strip a trailing timezone (`Z` or `±hh:mm`) — dates are treated as naive.
        // A time never contains `+`/`-` itself, so splitting on them isolates the
        // clock portion for both positive and negative offsets.
        let t = t.trim().trim_end_matches('Z');
        let t = t.split(['+', '-']).next().unwrap_or(t);
        let mut tp = t.split(':');
        hour = tp.next()?.trim().parse().ok()?;
        minute = tp.next().unwrap_or("0").parse().ok()?;
        // Seconds may carry a fractional part (`ss.SSS`); split it into millis.
        let sec_field = tp.next().unwrap_or("0");
        let (sec_str, ms) = match sec_field.split_once('.') {
            Some((s, frac)) => {
                // Take up to 3 fractional digits, right-padded, as milliseconds.
                let mut f = frac.chars().take(3).collect::<String>();
                while f.len() < 3 {
                    f.push('0');
                }
                (s, f.parse::<u32>().ok()?)
            }
            None => (sec_field, 0),
        };
        second = sec_str.parse().ok()?;
        millisecond = ms;
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
    }
    Some(BaseDate {
        year,
        month,
        day,
        hour,
        minute,
        second,
        millisecond,
        has_time,
    })
}

/// Days in `month` (1-12) of `year`, accounting for leap years (proleptic
/// Gregorian).
fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilink_parsing() {
        let l = parse_wikilink("[[Books]]").unwrap();
        assert_eq!(l.path, "Books");
        assert_eq!(l.display, None);
        let l2 = parse_wikilink("[[Categories/Books|Books]]").unwrap();
        assert_eq!(l2.path, "Categories/Books");
        assert_eq!(l2.display.as_deref(), Some("Books"));
        assert!(parse_wikilink("not a link").is_none());
        // Anchor stripped.
        assert_eq!(parse_wikilink("[[Note#Heading]]").unwrap().path, "Note");
    }

    #[test]
    fn date_parsing() {
        let d = parse_date("2024-01-02").unwrap();
        assert_eq!((d.year, d.month, d.day), (2024, 1, 2));
        assert!(!d.has_time);
        let dt = parse_date("2024-01-02 15:04:05").unwrap();
        assert_eq!((dt.hour, dt.minute, dt.second), (15, 4, 5));
        assert!(dt.has_time);
        let iso = parse_date("2024-01-02T15:04").unwrap();
        assert_eq!((iso.hour, iso.minute), (15, 4));
        // Non-dates.
        assert!(parse_date("hello").is_none());
        assert!(parse_date("12-34-56").is_none());
        assert!(parse_date("2024-13-01").is_none());
    }

    #[test]
    fn parse_date_handles_timezones_and_fractions() {
        // Negative offset (Western timezone) must parse, not degrade to a string.
        let d = parse_date("2024-01-02T15:04:05-05:00").unwrap();
        assert_eq!((d.hour, d.minute, d.second), (15, 4, 5));
        // Positive offset and Z too.
        assert!(parse_date("2024-01-02T15:04:05+05:30").is_some());
        assert!(parse_date("2024-01-02T15:04:05Z").is_some());
        // Fractional seconds → milliseconds.
        let f = parse_date("2024-01-02T15:04:05.123").unwrap();
        assert_eq!(f.millisecond, 123);
        assert_eq!(f.second, 5);
    }

    #[test]
    fn parse_date_rejects_impossible_calendar_dates() {
        assert!(parse_date("2024-02-30").is_none()); // Feb never has 30
        assert!(parse_date("2023-02-29").is_none()); // 2023 not a leap year
        assert!(parse_date("2024-02-29").is_some()); // 2024 is a leap year
        assert!(parse_date("2024-04-31").is_none()); // April has 30 days
        assert!(parse_date("2024-04-30").is_some());
    }

    #[test]
    fn yaml_to_values_infers_types() {
        let fm: serde_yml::Value = serde_yml::from_str(
            "title: Hello\ncount: 3\ndone: true\ncats:\n  - \"[[Categories/Books|Books]]\"\ncreated: 2024-05-06\n",
        )
        .unwrap();
        let props = properties_from_yaml(&fm);
        assert!(matches!(props.get("title"), Some(Value::Str(s)) if s == "Hello"));
        assert!(matches!(props.get("count"), Some(Value::Number(n)) if *n == 3.0));
        assert!(matches!(props.get("done"), Some(Value::Bool(true))));
        match props.get("cats") {
            Some(Value::List(items)) => {
                assert!(matches!(&items[0], Value::Link(l) if l.path == "Categories/Books"));
            }
            other => panic!("expected list, got {other:?}"),
        }
        // A bare YAML date scalar is parsed by serde_yml as a string here.
        assert!(matches!(props.get("created"), Some(Value::Date(_))));
    }

    #[test]
    fn has_tag_matches_parents() {
        let mut n = Note::default();
        n.tags = vec!["project/active".into()];
        assert!(n.has_tag("project"));
        assert!(n.has_tag("project/active"));
        assert!(n.has_tag("#project"));
        assert!(!n.has_tag("proj"));
    }

    #[test]
    fn in_folder_matches_subfolders() {
        let mut n = Note::default();
        n.folder = "Home/Things".into();
        assert!(n.in_folder("Home"));
        assert!(n.in_folder("Home/Things"));
        assert!(!n.in_folder("Misc"));
        assert!(n.in_folder("")); // vault root matches all
    }
}
