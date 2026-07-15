//! The typed value model shared by the evaluator, the function library, and the
//! renderer. Mirrors Obsidian Bases' runtime types: null, boolean, number,
//! string, date, duration, list, object, and link.
//!
//! The model is intentionally forgiving (Obsidian is): comparisons and coercions
//! between mismatched types yield sensible defaults rather than errors, so a
//! filter over notes with heterogeneous frontmatter never explodes.

use std::cmp::Ordering;
use std::collections::BTreeMap;

/// A wikilink value: a target path plus an optional display label. Produced by
/// the `link(...)` function, by frontmatter `[[wikilinks]]`, and by `file.asLink`.
#[derive(Debug, Clone, PartialEq)]
pub struct BaseLink {
    /// The link target as written (e.g. `Categories/Books` or `Books`). The
    /// `.md` extension and any `[[ ]]`/alias are already stripped.
    pub path: String,
    /// Optional display text (`link("path", "Display")` or `[[path|Display]]`).
    pub display: Option<String>,
}

impl BaseLink {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            display: None,
        }
    }

    pub fn with_display(path: impl Into<String>, display: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            display: Some(display.into()),
        }
    }

    /// The path's final segment without directories or extension — how Obsidian
    /// compares links written as full paths vs bare names
    /// (`link("Categories/Books")` matches a note named `Books`).
    pub fn basename(&self) -> &str {
        let no_dir = self.path.rsplit('/').next().unwrap_or(&self.path);
        no_dir.strip_suffix(".md").unwrap_or(no_dir)
    }

    /// Two links refer to the same note if their basenames match (Obsidian resolves
    /// short links by basename) OR one path is a suffix-path of the other.
    pub fn same_target(&self, other: &BaseLink) -> bool {
        if self.path == other.path {
            return true;
        }
        self.basename().eq_ignore_ascii_case(other.basename())
    }
}

/// A calendar date-time with millisecond resolution. Stored as broken-down fields
/// plus a total-milliseconds value (epoch-relative, proleptic Gregorian) so date
/// arithmetic and ordering are exact without pulling in a datetime dependency.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BaseDate {
    pub year: i64,
    pub month: u32, // 1-12
    pub day: u32,   // 1-31
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millisecond: u32,
    /// Whether the source carried a time component (drives default formatting:
    /// a bare `2024-01-02` renders without the `00:00:00`).
    pub has_time: bool,
}

impl BaseDate {
    /// Days from the epoch (1970-01-01) to this date, via a well-known
    /// civil-from-days algorithm (Howard Hinnant). Valid for the full proleptic
    /// Gregorian range.
    fn days_from_epoch(&self) -> i64 {
        let y = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400; // [0, 399]
        let m = self.month as i64;
        let d = self.day as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
        era * 146097 + doe - 719468
    }

    /// Total milliseconds since the Unix epoch (UTC, no timezone handling — dates
    /// in a static docs vault are naive). Used for ordering and subtraction.
    pub fn epoch_millis(&self) -> i64 {
        let days = self.days_from_epoch();
        let secs =
            days * 86400 + self.hour as i64 * 3600 + self.minute as i64 * 60 + self.second as i64;
        secs * 1000 + self.millisecond as i64
    }
}

impl PartialOrd for BaseDate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.epoch_millis().cmp(&other.epoch_millis()))
    }
}

/// A runtime value in the Bases expression language.
#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Date(BaseDate),
    /// A duration in milliseconds (from `duration(...)` or a date subtraction).
    Duration(i64),
    List(Vec<Value>),
    Object(BTreeMap<String, Value>),
    Link(BaseLink),
}

impl Value {
    /// A short type name, for `isType(...)` and diagnostics.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::Str(_) => "string",
            Value::Date(_) => "date",
            Value::Duration(_) => "duration",
            Value::List(_) => "list",
            Value::Object(_) => "object",
            Value::Link(_) => "link",
        }
    }

    /// Obsidian truthiness: null / false / 0 / empty string / empty list / empty
    /// object are falsey; everything else is truthy. Drives filters and `if(...)`.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Duration(d) => *d != 0,
            Value::List(l) => !l.is_empty(),
            Value::Object(o) => !o.is_empty(),
            Value::Date(_) | Value::Link(_) => true,
        }
    }

    /// Whether the value is "empty" for `isEmpty()` / the Filled/Empty summaries:
    /// null, empty string, empty list, or empty object.
    pub fn is_empty(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Str(s) => s.is_empty(),
            Value::List(l) => l.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => false,
        }
    }

    /// Best-effort numeric coercion (for arithmetic and `number(...)`). Booleans →
    /// 0/1, numeric strings parse, durations → their millisecond count; otherwise
    /// `None`.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Str(s) => s.trim().parse::<f64>().ok(),
            Value::Duration(d) => Some(*d as f64),
            _ => None,
        }
    }

    /// Coerce to a string the way an expression's `+` with a string operand would.
    pub fn as_str_coerced(&self) -> String {
        self.display()
    }

    /// The human-facing rendering used for table cells and string coercion.
    /// Numbers drop a trailing `.0`; lists join with `, `; links show their
    /// display or basename; dates use their default format.
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => format_number(*n),
            Value::Str(s) => s.clone(),
            Value::Date(d) => crate::format::default_date(d),
            Value::Duration(ms) => format!("{ms}ms"),
            Value::List(items) => items
                .iter()
                .map(Value::display)
                .collect::<Vec<_>>()
                .join(", "),
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| format!("{k}: {}", v.display()))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Link(l) => l
                .display
                .clone()
                .unwrap_or_else(|| l.basename().to_string()),
        }
    }
}

/// Format a number without a trailing `.0` for integers, keeping full precision
/// otherwise. `3.0` → `"3"`, `3.14` → `"3.14"`.
pub fn format_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        // Trim trailing zeros from the fractional part.
        let s = format!("{n}");
        s
    }
}

impl Value {
    /// Structural equality used by `==`/`!=` and `.contains(...)`. Numbers compare
    /// numerically (incl. across numeric strings), links compare by target,
    /// dates by instant; mismatched types are unequal (except numeric coercion).
    pub fn loose_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Date(a), Value::Date(b)) => a.epoch_millis() == b.epoch_millis(),
            (Value::Duration(a), Value::Duration(b)) => a == b,
            (Value::Link(a), Value::Link(b)) => a.same_target(b),
            // A link compared to a string matches by basename/path (so
            // `categories.contains("Books")` works alongside `link("Books")`).
            (Value::Link(a), Value::Str(b)) | (Value::Str(b), Value::Link(a)) => {
                a.path == *b || a.basename().eq_ignore_ascii_case(b)
            }
            (Value::List(a), Value::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.loose_eq(y))
            }
            // Cross-numeric (number vs numeric string / bool).
            _ => match (self.as_number(), other.as_number()) {
                (Some(a), Some(b))
                    if matches!(self, Value::Number(_) | Value::Bool(_))
                        || matches!(other, Value::Number(_) | Value::Bool(_)) =>
                {
                    a == b
                }
                _ => false,
            },
        }
    }

    /// Ordering for `<`/`>`/sorting. Numbers and dates order naturally; strings
    /// lexicographically (case-insensitive, like Obsidian's column sort); other
    /// combinations fall back to a stable type-name ordering so a sort never
    /// panics on mixed columns. `Null` sorts last.
    pub fn loose_cmp(&self, other: &Value) -> Ordering {
        match (self, other) {
            (Value::Null, Value::Null) => Ordering::Equal,
            (Value::Null, _) => Ordering::Greater,
            (_, Value::Null) => Ordering::Less,
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Date(a), Value::Date(b)) => a.epoch_millis().cmp(&b.epoch_millis()),
            (Value::Duration(a), Value::Duration(b)) => a.cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::Str(a), Value::Str(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            (Value::Link(a), Value::Link(b)) => {
                a.display().to_lowercase().cmp(&b.display().to_lowercase())
            }
            _ => {
                // Try numeric coercion, else compare rendered strings.
                match (self.as_number(), other.as_number()) {
                    (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
                    _ => self
                        .display()
                        .to_lowercase()
                        .cmp(&other.display().to_lowercase()),
                }
            }
        }
    }
}

impl BaseLink {
    fn display(&self) -> String {
        self.display
            .clone()
            .unwrap_or_else(|| self.basename().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthiness_matches_obsidian() {
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Bool(false).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(!Value::Str(String::new()).is_truthy());
        assert!(!Value::List(vec![]).is_truthy());
        assert!(Value::Number(1.0).is_truthy());
        assert!(Value::Str("x".into()).is_truthy());
        assert!(Value::List(vec![Value::Null]).is_truthy());
    }

    #[test]
    fn number_formatting_drops_integer_decimal() {
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(3.25), "3.25");
        assert_eq!(format_number(-5.0), "-5");
    }

    #[test]
    fn link_basename_and_same_target() {
        let a = BaseLink::new("Categories/Books");
        let b = BaseLink::new("Books");
        assert_eq!(a.basename(), "Books");
        assert!(a.same_target(&b));
        let c = BaseLink::with_display("Categories/Books", "Books");
        assert_eq!(c.display, Some("Books".to_string()));
    }

    #[test]
    fn link_string_loose_equality_by_basename() {
        let link = Value::Link(BaseLink::new("Categories/Books"));
        assert!(link.loose_eq(&Value::Str("Books".into())));
        assert!(link.loose_eq(&Value::Str("Categories/Books".into())));
        assert!(!link.loose_eq(&Value::Str("Films".into())));
    }

    #[test]
    fn numeric_string_cross_equality() {
        assert!(Value::Number(3.0).loose_eq(&Value::Str("3".into())));
        assert!(!Value::Str("3".into()).loose_eq(&Value::Str("3.0".into())));
    }

    #[test]
    fn epoch_millis_reference_dates() {
        let epoch = BaseDate {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            has_time: false,
        };
        assert_eq!(epoch.epoch_millis(), 0);
        let d = BaseDate {
            year: 2000,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            has_time: false,
        };
        // 2000-01-01 is 946684800 seconds after epoch.
        assert_eq!(d.epoch_millis(), 946_684_800_000);
    }

    #[test]
    fn date_ordering() {
        let a = BaseDate {
            year: 2020,
            month: 5,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            has_time: false,
        };
        let b = BaseDate {
            year: 2021,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            has_time: false,
        };
        assert_eq!(a.partial_cmp(&b), Some(Ordering::Less));
    }

    #[test]
    fn null_sorts_last() {
        assert_eq!(
            Value::Null.loose_cmp(&Value::Number(1.0)),
            Ordering::Greater
        );
        assert_eq!(Value::Number(1.0).loose_cmp(&Value::Null), Ordering::Less);
    }
}
